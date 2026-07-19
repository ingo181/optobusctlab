//! Führt die Gherkin-Features unter `tests/features/` aus.
//!
//! WICHTIG (Lernpunkt Tokio-Scheduling): Wir erzwingen bewusst den
//! `current_thread`-Runtime-Flavor, nicht das Default von `#[tokio::main]`
//! (multi-thread). Grund: In den Given-Schritten wird eine Antwort in die
//! `SimulatedConnection`-Warteschlange gelegt, BEVOR `Lab::spawn()` die
//! Actor-Task startet. Auf einem Single-Thread-Runtime wird die gespawnte
//! Task garantiert erst beim nächsten `.await`-Punkt der aufrufenden Task
//! ausgeführt - dadurch ist sichergestellt, dass `Lab::query()` seinen
//! `pending`-Eintrag UND den ausgehenden Befehl bereits synchron gesetzt
//! hat, bevor die Actor-Task das erste Mal läuft. Auf einem echten
//! Multi-Thread-Runtime könnte die Actor-Task auf einem anderen OS-Thread
//! sofort loslaufen und die Antwort verarbeiten, bevor die Anfrage
//! überhaupt registriert ist - ein klassisches Race, das nur bei diesem
//! Mock auftreten kann (echte Hardware kann nicht antworten, bevor sie
//! gefragt wurde). Siehe auch die Kommentare in `ctlab-transport`.
//!
//! `Lab::spawn()` ist inzwischen selbst `async` (verbindet fail-fast VOR dem
//! `tokio::spawn`, siehe `octlab-lab::Lab::spawn`-Doc). Das ändert an obiger
//! Garantie nichts: `SimulatedConnection::connect()` liefert `Ok(())` beim
//! ersten Poll, ohne je `Pending` zurückzugeben - ein `.await` auf so eine
//! Future gibt die Kontrolle nie an den Scheduler ab, ist also für die
//! Race-Betrachtung oben transparent wie ein normaler synchroner Aufruf.

use cucumber::{given, then, when, World};
use octlab_lab::{Lab, SetOutcome};
use octlab_protocol::{ChannelKey, ModuleAddress, SubChannel};
use octlab_transport::SimulatedConnection;
use std::sync::{Arc, Mutex};

// cucumber 0.21 ersetzt das ältere `#[derive(WorldInit)]` + handgeschriebenes
// `impl World` durch `#[derive(World)]`, das `impl World` (Konstruktion via
// `Default::default()`) und `impl WorldInventory` (Step-Registrierung für
// `#[given]`/`#[when]`/`#[then]`) selbst generiert.
#[derive(cucumber::World)]
struct LabWorld {
    /// `Some` bis `ensure_spawned()` sie in den Actor überführt.
    connection: Option<SimulatedConnection>,
    lab: Option<Lab>,
    /// Zweites Handle auf das Sende-Log der Connection - MUSS vor dem
    /// `Lab::spawn()` gezogen werden, weil die Connection dabei komplett in
    /// den Actor wandert (siehe Doc-Kommentar an
    /// `SimulatedConnection::sent_handle`).
    sent_log: Arc<Mutex<Vec<String>>>,
    last_query_result: Option<Option<f64>>,
    last_broadcast_value: Option<f64>,
    last_set_outcome: Option<SetOutcome>,
}

impl std::fmt::Debug for LabWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LabWorld")
            .field("last_query_result", &self.last_query_result)
            .field("last_broadcast_value", &self.last_broadcast_value)
            .field("last_set_outcome", &self.last_set_outcome)
            .finish()
    }
}

impl Default for LabWorld {
    fn default() -> Self {
        let connection = SimulatedConnection::new("cucumber-sim");
        let sent_log = connection.sent_handle();
        Self {
            connection: Some(connection),
            lab: None,
            sent_log,
            last_query_result: None,
            last_broadcast_value: None,
            last_set_outcome: None,
        }
    }
}

/// Startet den Lab-Actor genau einmal, lazy beim ersten When-Schritt.
async fn ensure_spawned(world: &mut LabWorld) {
    if world.lab.is_none() {
        let connection = world
            .connection
            .take()
            .expect("Connection bereits verbraucht");
        world.lab = Some(
            Lab::spawn(Box::new(connection))
                .await
                .expect("SimulatedConnection::connect() schlägt nie fehl"),
        );
    }
}

#[given(expr = "ein simuliertes Modul an Adresse {int}")]
fn given_module(_world: &mut LabWorld, _address: u8) {
    // Die Connection ist schon in World::default() vorbereitet; die
    // Adresse wird erst im When-Schritt gebraucht. Dieser Schritt dient
    // vor allem der Lesbarkeit der Spec.
}

#[given(expr = "das Modul antwortet auf die nächste Anfrage mit {string}")]
fn given_queued_response(world: &mut LabWorld, response: String) {
    world
        .connection
        .as_mut()
        .expect("Connection schon gespawnt - Given-Schritte müssen vor dem ersten When kommen")
        .push_response(response);
}

#[given(expr = "das Modul sendet unaufgefordert {string}")]
fn given_unsolicited(world: &mut LabWorld, response: String) {
    world
        .connection
        .as_mut()
        .expect("Connection schon gespawnt - Given-Schritte müssen vor dem ersten When kommen")
        .push_response(response);
}

// Bewusst eigener Step statt given_unsolicited() wiederzuverwenden: intern
// identisch (push_response), aber "unaufgefordert" verspricht einen
// parsebaren Wert - das wäre hier semantisch falsch, siehe
// malformed_input.feature.
#[given(expr = "das Modul sendet eine Zeile ohne gültiges Nachrichtenformat {string}")]
fn given_malformed_line(world: &mut LabWorld, raw_line: String) {
    world
        .connection
        .as_mut()
        .expect("Connection schon gespawnt - Given-Schritte müssen vor dem ersten When kommen")
        .push_response(raw_line);
}

#[when(expr = "ich Subkanal {int} an Adresse {int} abfrage")]
async fn when_query(world: &mut LabWorld, subchannel: u8, address: u8) {
    ensure_spawned(world).await;
    let key = ChannelKey {
        address: ModuleAddress(address),
        subchannel: SubChannel(subchannel),
    };
    let result = world.lab.as_ref().unwrap().query(key).await;
    world.last_query_result = Some(result);
}

#[given(expr = "das Modul quittiert das nächste Kommando mit {string}")]
fn given_scripted_ack(world: &mut LabWorld, reply: String) {
    // push_reply statt push_response: die Quittung darf erst NACH dem
    // zugehörigen Set-Kommando eintreffen (Request/Response-Sequenz),
    // nicht schon beim Start des Actors abrufbar sein.
    world
        .connection
        .as_mut()
        .expect("Connection schon gespawnt - Given-Schritte müssen vor dem ersten When kommen")
        .push_reply(reply);
}

#[when(expr = "ich Subkanal {int} an Adresse {int} auf den Wert {float} setze")]
async fn when_set(world: &mut LabWorld, subchannel: u8, address: u8, value: f64) {
    ensure_spawned(world).await;
    let key = ChannelKey {
        address: ModuleAddress(address),
        subchannel: SubChannel(subchannel),
    };
    let outcome = world.lab.as_ref().unwrap().set(key, value).await;
    world.last_set_outcome = Some(outcome);
}

#[when(expr = "ich die Live-Updates des Labs abonniere")]
async fn when_subscribe(world: &mut LabWorld) {
    ensure_spawned(world).await;
    // subscribe() ist synchron (kein .await davor) - das ist wichtig,
    // siehe Modul-Kommentar oben: die Registrierung muss passieren,
    // bevor die Actor-Task zum ersten Mal läuft.
    let mut rx = world.lab.as_ref().unwrap().subscribe();
    let msg = rx.recv().await.expect("kein Update empfangen");
    world.last_broadcast_value = Some(msg.value);
}

#[then(expr = "erhalte ich den Wert {float}")]
fn then_value(world: &mut LabWorld, expected: f64) {
    assert_eq!(
        world.last_query_result,
        Some(Some(expected)),
        "erwartete Some({expected}), Query lieferte {:?}",
        world.last_query_result
    );
}

#[then(expr = "läuft die Abfrage in einen Timeout")]
fn then_timeout(world: &mut LabWorld) {
    assert_eq!(
        world.last_query_result,
        Some(None),
        "erwartete Timeout (None), Query lieferte {:?}",
        world.last_query_result
    );
}

#[then(expr = "empfange ich einen Wert von {float}")]
fn then_broadcast_value(world: &mut LabWorld, expected: f64) {
    assert_eq!(world.last_broadcast_value, Some(expected));
}

#[then(expr = "wurde das Kommando {string} gesendet")]
fn then_command_sent(world: &mut LabWorld, expected: String) {
    let sent = world.sent_log.lock().unwrap();
    assert!(
        sent.contains(&expected),
        "Kommando {expected:?} nicht im Sende-Log, gesendet wurde: {sent:?}"
    );
}

#[then(expr = "meldet das Setzen Erfolg mit Status {string}")]
fn then_set_confirmed(world: &mut LabWorld, expected_status: String) {
    assert_eq!(
        world.last_set_outcome,
        Some(SetOutcome::Confirmed {
            status_text: Some(expected_status.clone())
        }),
        "erwartete Confirmed({expected_status:?}), Setzen lieferte {:?}",
        world.last_set_outcome
    );
}

#[then(expr = "meldet das Setzen Ablehnung mit Status {string}")]
fn then_set_rejected(world: &mut LabWorld, expected_status: String) {
    match &world.last_set_outcome {
        Some(SetOutcome::Rejected { status_text, .. })
            if status_text.as_deref() == Some(expected_status.as_str()) => {}
        other => panic!("erwartete Rejected({expected_status:?}), Setzen lieferte {other:?}"),
    }
}

#[then(expr = "meldet das Setzen keine Antwort")]
fn then_set_no_reply(world: &mut LabWorld) {
    assert_eq!(
        world.last_set_outcome,
        Some(SetOutcome::NoReply),
        "erwartete NoReply, Setzen lieferte {:?}",
        world.last_set_outcome
    );
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let features = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/features");
    // fail_on_skipped(): cucumber-rs kennt kein eigenes "undefiniert" - ein
    // Step ohne Regex-Treffer erzeugt exakt dasselbe Skipped-Event wie ein
    // bewusst nicht erreichter Step (siehe cucumber::event::Step::Skipped-
    // Doc). Ohne fail_on_skipped() ist das für den Prozess-Exit-Code
    // folgenlos - LabWorld::run() (= cucumber().run_and_exit()) endet mit
    // 0, selbst wenn Szenarien nie über ihren ersten Step hinauskommen.
    LabWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit(features)
        .await;
}
