//! Führt die Polling-Features unter `tests/polling_features/` aus (Spec 0005).
//!
//! Eigener Runner, getrennt von `tests/cucumber.rs`: Hier läuft die
//! Tokio-Uhr PAUSIERT (`start_paused = true`). Die Zeit springt
//! automatisch vorwärts, sobald alle Tasks warten - ein Test über "20000 ms"
//! dauert so nur Millisekunden, und alle Zeitpunkte sind exakt
//! reproduzierbar. Die pausierte Uhr gilt für die GANZE Runtime; die
//! bestehenden Features (echte Zeit) bleiben deshalb in ihrem eigenen
//! Runner. `start_paused` braucht den `current_thread`-Flavor und das
//! tokio-Feature `test-util` (nur unter `[dev-dependencies]`).
//!
//! Szenarien laufen nacheinander (`max_concurrent_scenarios(1)`): Mit
//! virtueller Zeit würden sie sich zwar nicht verfälschen, aber
//! Fehlermeldungen bleiben so eindeutig einem Szenario zuzuordnen.
//!
//! Die Module simuliert `octlab_transport::SimBus`; dessen Sende-Log mit
//! Zeitstempeln ist die Grundlage fast aller Then-Schritte.

use cucumber::{given, then, when, World};
use octlab_lab::{
    Ack, ChannelUpdate, ConnectionState, Lab, PollConfig, SetReadBack, SetReadBackError,
};
use octlab_protocol::{ChannelKey, Message, ModuleAddress, SubChannel};
use octlab_transport::{ConnectionEvent, IoAttempt, SimBus, SimulatedConnection};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::Instant;

/// Obergrenze (virtuelle Zeit) für Schritte, die auf einen Zustand warten -
/// verhindert, dass ein roter Test endlos läuft.
const WAIT_LIMIT: Duration = Duration::from_secs(60);
/// Schrittweite beim Warten auf einen Zustand. Exakte Zeitpunkte werden
/// danach aus dem Sende-Log abgelesen, nicht aus dieser Schrittweite.
const WAIT_STEP: Duration = Duration::from_millis(1);
/// Query-Timeout des Lab-Actors (Spec 0005 / CLAUDE.md).
const QUERY_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(cucumber::World)]
struct PollWorld {
    bus: SimBus,
    /// `Some` bis zum Start des Lab-Actors, dann wandert sie hinein.
    connection: Option<SimulatedConnection>,
    channels: Vec<ChannelKey>,
    /// `None` = Default-Intervall (`PollConfig::new`).
    interval: Option<Duration>,
    lab: Option<Lab>,
    /// Zeitfenster des letzten "vergehen"/"abgelaufen"-Schritts: [start, end).
    window: Option<(Instant, Instant)>,
    changes: Option<broadcast::Receiver<ChannelUpdate>>,
    change_log: Vec<ChannelUpdate>,
    lab_updates: Option<broadcast::Receiver<Message>>,
    snapshot: Vec<ChannelUpdate>,
    set_requested_at: Option<Instant>,
    set_finished_at: Option<Instant>,
    set_result: Option<Result<SetReadBack, SetReadBackError>>,
    timeout_at: Option<Instant>,
    /// Zeitpunkt des letzten Verbindungsabbruchs durch die Gegenseite.
    dropped_at: Option<Instant>,
    /// Ergebnis eines expliziten Starts (AK8 Spec 0008): Fehlertext bei Err.
    spawn_error: Option<String>,
}

impl Default for PollWorld {
    fn default() -> Self {
        let bus = SimBus::new();
        let connection = SimulatedConnection::new("polling-sim").with_bus(bus.clone());
        Self {
            bus,
            connection: Some(connection),
            channels: Vec::new(),
            interval: None,
            lab: None,
            window: None,
            changes: None,
            change_log: Vec::new(),
            lab_updates: None,
            snapshot: Vec::new(),
            set_requested_at: None,
            set_finished_at: None,
            set_result: None,
            timeout_at: None,
            dropped_at: None,
            spawn_error: None,
        }
    }
}

impl std::fmt::Debug for PollWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PollWorld")
            .field("channels", &self.channels)
            .field("interval", &self.interval)
            .field("set_result", &self.set_result)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Hilfsfunktionen
// ---------------------------------------------------------------------------

/// `"2:10"` → `ChannelKey { address: 2, subchannel: 10 }`.
fn key(text: &str) -> ChannelKey {
    let (address, subchannel) = text.trim().split_once(':').expect("Kanal als \"a:s\"");
    ChannelKey {
        address: ModuleAddress(address.parse().expect("Adresse")),
        subchannel: SubChannel(subchannel.parse().expect("Subkanal")),
    }
}

fn query_line(key: ChannelKey) -> String {
    format!("{}:{}?", key.address.0, key.subchannel.0)
}

fn list(text: &str) -> impl Iterator<Item = &str> {
    text.split(',').map(str::trim).filter(|s| !s.is_empty())
}

impl PollWorld {
    /// Startet den Lab-Actor genau einmal, beim ersten Schritt, der ihn braucht.
    fn lab(&self) -> &Lab {
        self.lab.as_ref().expect("Lab nicht gestartet")
    }

    async fn ensure_spawned(&mut self) {
        // Schon gestartet - oder ein expliziter Start ist gescheitert (Spec
        // 0008 AK8): dann nicht noch einmal verbinden.
        if self.lab.is_some() || self.spawn_error.is_some() {
            return;
        }
        let connection = self.connection.take().expect("Connection verbraucht");
        let mut config = PollConfig::new(self.channels.clone());
        if let Some(interval) = self.interval {
            config.interval = interval;
        }
        self.lab = Some(
            Lab::spawn_with_polling(Box::new(connection), config)
                .await
                .expect("SimulatedConnection::connect() schlägt nie fehl"),
        );
    }

    fn state(&self, channel: ChannelKey) -> Option<ChannelUpdate> {
        self.lab()
            .subscribe_channels()
            .0
            .into_iter()
            .find(|update| update.key == channel)
    }

    /// Wartet (virtuell) bis `done` gilt; Panik nach `WAIT_LIMIT`.
    async fn wait_until(&self, what: &str, done: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + WAIT_LIMIT;
        while !done(self) {
            assert!(
                Instant::now() < deadline,
                "Zeitlimit beim Warten auf: {what}"
            );
            tokio::time::sleep(WAIT_STEP).await;
        }
    }

    /// Zeitpunkte aller gesendeten Zeilen `line` innerhalb [from, to).
    fn sent_times(&self, line: &str, from: Instant, to: Instant) -> Vec<Instant> {
        self.bus
            .sent()
            .into_iter()
            .filter(|(at, sent)| sent == line && *at >= from && *at < to)
            .map(|(at, _)| at)
            .collect()
    }

    fn window(&self) -> (Instant, Instant) {
        self.window
            .expect("kein Zeitfenster - vorher \"... vergehen\" o.ä.")
    }

    fn drain_changes(&mut self) {
        if let Some(rx) = self.changes.as_mut() {
            while let Ok(update) = rx.try_recv() {
                self.change_log.push(update);
            }
        }
    }

    fn set_line_index(&self, set_line: &str) -> (usize, Vec<(Instant, String)>) {
        let sent = self.bus.sent();
        let index = sent
            .iter()
            .rposition(|(_, line)| line == set_line)
            .unwrap_or_else(|| panic!("{set_line:?} nicht gesendet: {sent:?}"));
        (index, sent)
    }
}

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "die Module liefern {string}")]
fn given_values(world: &mut PollWorld, values: String) {
    set_values(world, &values);
}

#[when(expr = "die Module ab jetzt {string} liefern")]
fn when_values(world: &mut PollWorld, values: String) {
    set_values(world, &values);
}

fn set_values(world: &mut PollWorld, values: &str) {
    for entry in list(values) {
        let (channel, value) = entry.split_once('=').expect("Eintrag als \"a:s=wert\"");
        let channel = key(channel);
        world
            .bus
            .set_value(channel.address.0, channel.subchannel.0, value.trim());
    }
}

#[given(expr = "die Kanalliste {string}")]
fn given_channel_list(world: &mut PollWorld, channels: String) {
    world.channels = list(&channels).map(key).collect();
}

#[given(expr = "ein Poll-Intervall von {int} ms")]
fn given_interval(world: &mut PollWorld, ms: u64) {
    world.interval = Some(Duration::from_millis(ms));
}

#[given("kein Poll-Intervall ist konfiguriert")]
fn given_default_interval(world: &mut PollWorld) {
    world.interval = None;
}

#[given(expr = "das Modul an Adresse {int} antwortet nicht")]
fn given_muted(world: &mut PollWorld, address: u8) {
    world.bus.mute(address);
}

#[given(expr = "das Modul an Adresse {int} quittiert nicht")]
fn given_acks_dropped(world: &mut PollWorld, address: u8) {
    world.bus.drop_acks(address);
}

#[given(expr = "jedes Modul antwortet erst nach {int} ms")]
fn given_delay(world: &mut PollWorld, ms: u64) {
    world.bus.set_reply_delay(Duration::from_millis(ms));
}

#[when(expr = "jedes Modul ab jetzt erst nach {int} ms antwortet")]
fn when_delay(world: &mut PollWorld, ms: u64) {
    world.bus.set_reply_delay(Duration::from_millis(ms));
}

#[when("jedes Modul ab jetzt sofort antwortet")]
fn when_no_delay(world: &mut PollWorld) {
    world.bus.set_reply_delay(Duration::ZERO);
}

#[given("ich abonniere den Änderungsstrom")]
async fn given_subscribe_changes(world: &mut PollWorld) {
    world.ensure_spawned().await;
    world.changes = Some(world.lab().subscribe_channels().1);
}

#[given("ich abonniere die Lab-Updates")]
async fn given_subscribe_lab(world: &mut PollWorld) {
    world.ensure_spawned().await;
    world.lab_updates = Some(world.lab().subscribe());
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "{int} ms vergehen")]
async fn when_time_passes(world: &mut PollWorld, ms: u64) {
    world.ensure_spawned().await;
    let start = Instant::now();
    tokio::time::sleep(Duration::from_millis(ms)).await;
    world.window = Some((start, start + Duration::from_millis(ms)));
}

#[when(expr = "{int} Poll-Zyklus abgelaufen ist")]
async fn when_one_cycle(world: &mut PollWorld, cycles: u64) {
    wait_cycles(world, cycles).await;
}

#[when(expr = "{int} Poll-Zyklen abgelaufen sind")]
async fn when_cycles(world: &mut PollWorld, cycles: u64) {
    wait_cycles(world, cycles).await;
}

/// Wartet, bis `cycles` WEITERE Zyklen abgeschlossen sind (relativ zum
/// aktuellen Stand der Statistik). Fenster: [Start, Ende] inklusive.
async fn wait_cycles(world: &mut PollWorld, cycles: u64) {
    world.ensure_spawned().await;
    let start = Instant::now();
    let target = world.lab().poll_stats().completed_cycles + cycles;
    world
        .wait_until(&format!("{target} abgeschlossene Zyklen"), |w| {
            w.lab().poll_stats().completed_cycles >= target
        })
        .await;
    world.window = Some((start, Instant::now() + Duration::from_nanos(1)));
}

#[when(expr = "die Abfrage von {string} ins Timeout läuft")]
async fn when_query_times_out(world: &mut PollWorld, channel: String) {
    world.ensure_spawned().await;
    let channel = key(&channel);
    world
        .wait_until("Kanal veraltet", |w| {
            w.state(channel).is_some_and(|state| state.stale)
        })
        .await;
    // Exakter Timeout-Zeitpunkt: letzte Abfrage des Kanals + Query-Timeout.
    let last_query = world
        .bus
        .sent()
        .into_iter()
        .rev()
        .find(|(_, line)| *line == query_line(channel))
        .expect("Kanal wurde nie abgefragt")
        .0;
    world.timeout_at = Some(last_query + QUERY_TIMEOUT);
}

#[when(expr = "das Modul an Adresse {int} verstummt")]
fn when_mute(world: &mut PollWorld, address: u8) {
    world.bus.mute(address);
}

#[when(expr = "das Modul an Adresse {int} wieder antwortet")]
fn when_unmute(world: &mut PollWorld, address: u8) {
    world.bus.unmute(address);
}

#[when(expr = "der Kanal {string} einen Wert geliefert hat")]
async fn when_channel_has_value(world: &mut PollWorld, channel: String) {
    wait_fresh_value(world, &channel).await;
}

#[when(expr = "der Kanal {string} wieder einen Wert geliefert hat")]
async fn when_channel_has_value_again(world: &mut PollWorld, channel: String) {
    wait_fresh_value(world, &channel).await;
}

async fn wait_fresh_value(world: &mut PollWorld, channel: &str) {
    world.ensure_spawned().await;
    let channel = key(channel);
    world
        .wait_until("aktueller Wert", |w| {
            w.state(channel)
                .is_some_and(|state| state.value.is_some() && !state.stale)
        })
        .await;
}

#[when(expr = "ich {string} auf {float} setze und zurücklese")]
async fn when_set_and_read_back(world: &mut PollWorld, channel: String, value: f64) {
    world.ensure_spawned().await;
    world.set_requested_at = Some(Instant::now());
    let result = world.lab().set_and_read_back(key(&channel), value).await;
    world.set_finished_at = Some(Instant::now());
    world.set_result = Some(result);
}

#[when("ein neuer Abonnent den Änderungsstrom abonniert")]
fn when_new_subscriber(world: &mut PollWorld) {
    world.snapshot = world.lab().subscribe_channels().0;
}

#[when(expr = "das Modul unaufgefordert {string} sendet")]
async fn when_inject(world: &mut PollWorld, line: String) {
    world.ensure_spawned().await;
    world.bus.inject(line);
}

// ---------------------------------------------------------------------------
// Then: Abfragen im Sende-Log
// ---------------------------------------------------------------------------

#[then(expr = "wurde jeder der Kanäle {string} genau einmal abgefragt")]
fn then_each_queried_once(world: &mut PollWorld, channels: String) {
    let (from, to) = world.window();
    for channel in list(&channels).map(key) {
        let times = world.sent_times(&query_line(channel), from, to);
        assert_eq!(times.len(), 1, "{channel:?} abgefragt zu {times:?}");
    }
}

#[then(expr = "wurde der Kanal {string} {int} Mal abgefragt")]
fn then_queried_n_times(world: &mut PollWorld, channel: String, n: usize) {
    let (from, to) = world.window();
    let times = world.sent_times(&query_line(key(&channel)), from, to);
    assert_eq!(times.len(), n, "Abfragen von {channel} zu {times:?}");
}

#[then(expr = "wurden die Kanäle {string} je {int} Mal abgefragt")]
fn then_channels_queried_n_times(world: &mut PollWorld, channels: String, n: usize) {
    let (from, to) = world.window();
    for channel in list(&channels).map(key) {
        let times = world.sent_times(&query_line(channel), from, to);
        assert_eq!(times.len(), n, "{channel:?} abgefragt zu {times:?}");
    }
}

#[then(expr = "wurde {string} mindestens {int} Mal abgefragt")]
fn then_queried_at_least(world: &mut PollWorld, channel: String, n: usize) {
    let (from, to) = world.window();
    let times = world.sent_times(&query_line(key(&channel)), from, to);
    assert!(times.len() >= n, "nur {} Abfragen: {times:?}", times.len());
}

#[then("wurde keine Abfrage gesendet")]
fn then_nothing_sent(world: &mut PollWorld) {
    let sent = world.bus.sent();
    assert!(sent.is_empty(), "gesendet wurde: {sent:?}");
}

#[then(expr = "wird {string} in den folgenden {int} ms nicht erneut abgefragt")]
async fn then_not_queried_during_backoff(world: &mut PollWorld, channel: String, ms: u64) {
    let timeout_at = world.timeout_at.expect("kein Timeout beobachtet");
    let until = timeout_at + Duration::from_millis(ms);
    tokio::time::sleep_until(until).await;
    let times = world.sent_times(&query_line(key(&channel)), timeout_at, until);
    assert!(times.is_empty(), "Abfragen im Backoff: {times:?}");
}

#[then(expr = "lag zwischen Timeout und erneuter Abfrage von {string} mindestens {int} ms")]
fn then_retry_after_backoff(world: &mut PollWorld, channel: String, ms: u64) {
    let timeout_at = world.timeout_at.expect("kein Timeout beobachtet");
    let retry = world
        .sent_times(&query_line(key(&channel)), timeout_at, Instant::now())
        .first()
        .copied()
        .expect("keine erneute Abfrage");
    assert!(
        retry - timeout_at >= Duration::from_millis(ms),
        "erneute Abfrage schon nach {:?}",
        retry - timeout_at
    );
}

#[then(expr = "wird {string} in den nächsten {int} Zyklen jeweils abgefragt")]
async fn then_queried_each_cycle(world: &mut PollWorld, channel: String, cycles: u32) {
    let interval = world.lab().poll_stats().interval;
    let from = Instant::now();
    let to = from + interval * cycles;
    tokio::time::sleep_until(to).await;
    let times = world.sent_times(&query_line(key(&channel)), from, to);
    assert_eq!(times.len(), cycles as usize, "Abfragen: {times:?}");
}

#[then(
    expr = "liegen zwischen aufeinanderfolgenden Abfragen von {string} jeweils mindestens {int} ms"
)]
fn then_min_gap_for_channel(world: &mut PollWorld, channel: String, ms: u64) {
    let (from, to) = world.window();
    let times = world.sent_times(&query_line(key(&channel)), from, to);
    for pair in times.windows(2) {
        assert!(
            pair[1] - pair[0] >= Duration::from_millis(ms),
            "Abstand {:?} in {times:?}",
            pair[1] - pair[0]
        );
    }
}

#[then(expr = "wurde {string} bei {string} ms abgefragt")]
fn then_queried_at(world: &mut PollWorld, channel: String, offsets: String) {
    let (from, to) = world.window();
    let actual: Vec<u128> = world
        .sent_times(&query_line(key(&channel)), from, to)
        .into_iter()
        .map(|at| (at - from).as_millis())
        .collect();
    let expected: Vec<u128> = list(&offsets).map(|o| o.parse().unwrap()).collect();
    assert_eq!(actual, expected);
}

#[then(expr = "lagen zwischen zwei Abfragen jeweils mindestens {int} ms")]
fn then_min_gap_all(world: &mut PollWorld, ms: u64) {
    let (from, to) = world.window();
    let times: Vec<Instant> = world
        .bus
        .sent()
        .into_iter()
        .filter(|(at, _)| *at >= from && *at < to)
        .map(|(at, _)| at)
        .collect();
    for pair in times.windows(2) {
        assert!(
            pair[1] - pair[0] >= Duration::from_millis(ms),
            "überlappende Abfragen: Abstand {:?}",
            pair[1] - pair[0]
        );
    }
}

// ---------------------------------------------------------------------------
// Then: Kanalzustand
// ---------------------------------------------------------------------------

#[then(expr = "hat der Kanal {string} den Wert {float}")]
fn then_value(world: &mut PollWorld, channel: String, value: f64) {
    let state = world.state(key(&channel));
    assert_eq!(
        state.as_ref().and_then(|s| s.value),
        Some(value),
        "Zustand {state:?}"
    );
}

#[then(expr = "hat der Kanal {string} keinen Wert")]
fn then_no_value(world: &mut PollWorld, channel: String) {
    let state = world.state(key(&channel)).expect("Kanal ohne Zustand");
    assert_eq!(state.value, None);
}

#[then(expr = "ist der Kanal {string} als veraltet markiert")]
fn then_stale(world: &mut PollWorld, channel: String) {
    let state = world.state(key(&channel));
    assert!(state.as_ref().is_some_and(|s| s.stale), "Zustand {state:?}");
}

#[then(expr = "ist der Kanal {string} nicht als veraltet markiert")]
fn then_not_stale(world: &mut PollWorld, channel: String) {
    let state = world.state(key(&channel));
    assert!(
        state
            .as_ref()
            .is_some_and(|s| !s.stale && s.value.is_some()),
        "Zustand {state:?}"
    );
}

#[then("ist kein Kanal als veraltet markiert")]
fn then_none_stale(world: &mut PollWorld) {
    let states = world.lab().subscribe_channels().0;
    assert!(states.iter().all(|s| !s.stale), "Zustände {states:?}");
}

// ---------------------------------------------------------------------------
// Then: Änderungsstrom, Snapshot, Lab-Updates
// ---------------------------------------------------------------------------

#[then(expr = "enthält der Änderungsstrom genau {int} Meldung(en) für {string}")]
fn then_change_count(world: &mut PollWorld, n: usize, channel: String) {
    world.drain_changes();
    let channel = key(&channel);
    let updates: Vec<_> = world
        .change_log
        .iter()
        .filter(|u| u.key == channel)
        .collect();
    assert_eq!(updates.len(), n, "Meldungen {updates:?}");
}

#[then(expr = "enthält der Änderungsstrom eine Meldung für {string} mit Wert {float}")]
fn then_change_with_value(world: &mut PollWorld, channel: String, value: f64) {
    world.drain_changes();
    let channel = key(&channel);
    assert!(
        world
            .change_log
            .iter()
            .any(|u| u.key == channel && u.value == Some(value)),
        "Meldungen {:?}",
        world.change_log
    );
}

#[then(expr = "enthält der Änderungsstrom keine Meldung mit Wert {float}")]
fn then_no_change_with_value(world: &mut PollWorld, value: f64) {
    world.drain_changes();
    assert!(
        world.change_log.iter().all(|u| u.value != Some(value)),
        "Meldungen {:?}",
        world.change_log
    );
}

/// `"12.0, 12.0 veraltet, 12.0"` → [(Some(12.0), false), (Some(12.0), true), ...]
#[then(expr = "meldete der Änderungsstrom für {string} nacheinander {string}")]
fn then_change_sequence(world: &mut PollWorld, channel: String, expected: String) {
    world.drain_changes();
    let channel = key(&channel);
    let expected: Vec<(Option<f64>, bool)> = list(&expected)
        .map(|entry| match entry.strip_suffix(" veraltet") {
            Some(value) => (Some(value.parse().unwrap()), true),
            None => (Some(entry.parse().unwrap()), false),
        })
        .collect();
    let actual: Vec<(Option<f64>, bool)> = world
        .change_log
        .iter()
        .filter(|u| u.key == channel)
        .map(|u| (u.value, u.stale))
        .collect();
    assert_eq!(actual, expected);
}

#[then(expr = "enthält sein Snapshot für {string} den Wert {float}")]
fn then_snapshot_value(world: &mut PollWorld, channel: String, value: f64) {
    let channel = key(&channel);
    let entry = world.snapshot.iter().find(|u| u.key == channel);
    assert_eq!(
        entry.map(|u| (u.value, u.stale)),
        Some((Some(value), false)),
        "Snapshot {:?}",
        world.snapshot
    );
}

#[then(expr = "enthält sein Snapshot für {string} keinen Wert und die Markierung veraltet")]
fn then_snapshot_stale_without_value(world: &mut PollWorld, channel: String) {
    let channel = key(&channel);
    let entry = world.snapshot.iter().find(|u| u.key == channel);
    assert_eq!(
        entry.map(|u| (u.value, u.stale)),
        Some((None, true)),
        "Snapshot {:?}",
        world.snapshot
    );
}

#[then(expr = "empfängt der Lab-Abonnent den Wert {float}")]
async fn then_lab_subscriber_value(world: &mut PollWorld, value: f64) {
    let rx = world.lab_updates.as_mut().expect("nicht abonniert");
    let found = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match rx.recv().await {
                Ok(msg) if msg.value == value => return true,
                Ok(_) => continue,
                Err(_) => return false,
            }
        }
    })
    .await
    .unwrap_or(false);
    assert!(found, "Wert {value} kam nicht beim Lab-Abonnenten an");
}

// ---------------------------------------------------------------------------
// Then: Setz-Sequenz
// ---------------------------------------------------------------------------

#[then(expr = "wurde zwischen Setz-Anforderung und {string} keine Abfrage gesendet")]
fn then_set_overtakes(world: &mut PollWorld, set_line: String) {
    let requested = world.set_requested_at.expect("kein Setzen angefordert");
    let (index, sent) = world.set_line_index(&set_line);
    let overtaken: Vec<_> = sent[..index]
        .iter()
        .filter(|(at, _)| *at >= requested)
        .collect();
    assert!(
        overtaken.is_empty(),
        "vor dem Setzen gesendet: {overtaken:?}"
    );
}

#[then(expr = "folgt im Sende-Log auf {string} direkt {string} nach {int} ms")]
fn then_readback_follows(world: &mut PollWorld, set_line: String, next: String, ms: u64) {
    let (index, sent) = world.set_line_index(&set_line);
    let following = sent
        .get(index + 1)
        .unwrap_or_else(|| panic!("nichts nach {set_line:?}"));
    assert_eq!(following.1, next, "Sende-Log {sent:?}");
    assert_eq!(following.0 - sent[index].0, Duration::from_millis(ms));
}

#[then(expr = "dauerte die Setz-Sequenz ab {string} genau {int} ms")]
fn then_set_duration(world: &mut PollWorld, set_line: String, ms: u64) {
    let (index, sent) = world.set_line_index(&set_line);
    let finished = world.set_finished_at.expect("Setzen nicht beendet");
    assert_eq!(finished - sent[index].0, Duration::from_millis(ms));
}

#[then(expr = "meldet die Setz-Sequenz Quittung {string} und Rücklesewert {float}")]
fn then_set_ok(world: &mut PollWorld, status: String, value: f64) {
    let result = world
        .set_result
        .clone()
        .expect("kein Setz-Ergebnis")
        .expect("Setz-Sequenz meldete einen Verbindungsfehler");
    let ack = result.ack.as_ref().expect("keine Quittung");
    assert_eq!(ack.status_text.as_deref(), Some(status.as_str()));
    assert_eq!(result.readback, Some(value));
}

#[then(expr = "meldet die Setz-Sequenz keine Quittung und Rücklesewert {float}")]
fn then_set_no_ack_with_readback(world: &mut PollWorld, value: f64) {
    let result = world
        .set_result
        .clone()
        .expect("kein Setz-Ergebnis")
        .expect("Setz-Sequenz meldete einen Verbindungsfehler");
    assert_eq!(result.ack, None);
    assert_eq!(result.readback, Some(value));
}

#[then("meldet die Setz-Sequenz keine Quittung und keinen Rücklesewert")]
fn then_set_nothing(world: &mut PollWorld) {
    let result = world
        .set_result
        .clone()
        .expect("kein Setz-Ergebnis")
        .expect("Setz-Sequenz meldete einen Verbindungsfehler");
    assert_eq!(result.ack, None);
    assert_eq!(result.readback, None);
}

#[then("wird danach weiter gepollt")]
async fn then_polling_continues(world: &mut PollWorld) {
    let finished = world.set_finished_at.expect("Setzen nicht beendet");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let polls_after = world
        .bus
        .sent()
        .into_iter()
        .filter(|(at, line)| *at > finished && line.ends_with('?'))
        .count();
    assert!(polls_after > 0, "nach dem Setzen keine Poll-Abfrage mehr");
}

// ---------------------------------------------------------------------------
// Then: Poll-Statistik
// ---------------------------------------------------------------------------

#[then(expr = "zählt die Poll-Statistik {int} Überlauf-Episode(n)")]
fn then_overrun_episodes(world: &mut PollWorld, n: u64) {
    assert_eq!(world.lab().poll_stats().overrun_episodes, n);
}

#[then(
    expr = "meldet die Poll-Statistik eine letzte Zykluszeit von {int} ms und ein Intervall von {int} ms"
)]
fn then_cycle_time(world: &mut PollWorld, cycle_ms: u64, interval_ms: u64) {
    let stats = world.lab().poll_stats();
    assert_eq!(stats.last_cycle, Some(Duration::from_millis(cycle_ms)));
    assert_eq!(stats.interval, Duration::from_millis(interval_ms));
}

// ---------------------------------------------------------------------------
// Spec 0008: Verbindungsverlust
// ---------------------------------------------------------------------------

#[when("die Gegenseite die Verbindung abbricht")]
async fn when_peer_drops(world: &mut PollWorld) {
    world.ensure_spawned().await;
    world.dropped_at = Some(Instant::now());
    world.bus.drop_connection();
    // Dem Actor Gelegenheit geben, den Abbruch zu bemerken.
    tokio::time::sleep(Duration::from_millis(1)).await;
}

#[given("die Gegenseite nimmt keine Verbindung an")]
fn given_refuse(world: &mut PollWorld) {
    world.bus.refuse_connections(true);
}

#[when("die Gegenseite keine Verbindung mehr annimmt")]
fn when_refuse(world: &mut PollWorld) {
    world.bus.refuse_connections(true);
}

#[when("die Gegenseite wieder Verbindungen annimmt")]
fn when_accept(world: &mut PollWorld) {
    world.bus.refuse_connections(false);
}

#[when("das Lab gestartet wird")]
async fn when_lab_started(world: &mut PollWorld) {
    let connection = world.connection.take().expect("Connection verbraucht");
    let config = PollConfig::new(world.channels.clone());
    match Lab::spawn_with_polling(Box::new(connection), config).await {
        Ok(lab) => world.lab = Some(lab),
        Err(err) => world.spawn_error = Some(err.to_string()),
    }
}

#[when(
    expr = "ich {string} auf {float} setze und zurücklese, während die Verbindung nach {int} ms abbricht"
)]
async fn when_set_with_drop(world: &mut PollWorld, channel: String, value: f64, ms: u64) {
    world.ensure_spawned().await;
    let bus = world.bus.clone();
    let lab = world.lab.as_ref().unwrap();
    world.set_requested_at = Some(Instant::now());
    let drop_later = async {
        tokio::time::sleep(Duration::from_millis(ms)).await;
        bus.drop_connection();
        Instant::now()
    };
    let (result, dropped_at) =
        tokio::join!(lab.set_and_read_back(key(&channel), value), drop_later);
    world.set_finished_at = Some(Instant::now());
    world.dropped_at = Some(dropped_at);
    world.set_result = Some(result);
}

#[when("die Verbindung wiederhergestellt ist")]
async fn when_reconnected(world: &mut PollWorld) {
    world
        .wait_until("Verbindung wiederhergestellt", |w| {
            w.lab().poll_stats().connection == ConnectionState::Connected
        })
        .await;
}

#[then(expr = "erfolgten Verbindungsversuche bei {string} s nach dem Abbruch")]
fn then_connect_times(world: &mut PollWorld, offsets: String) {
    let dropped = world.dropped_at.expect("kein Abbruch");
    let actual: Vec<u64> = world
        .bus
        .connection_log()
        .into_iter()
        .filter(|(at, e)| *at >= dropped && matches!(e, ConnectionEvent::Connect { .. }))
        .map(|(at, _)| (at - dropped).as_secs())
        .collect();
    let expected: Vec<u64> = list(&offsets).map(|o| o.parse().unwrap()).collect();
    assert_eq!(actual, expected);
}

#[then(expr = "der erste Verbindungsversuch nach dem Abbruch erfolgte nach {int} s")]
fn then_first_attempt_after(world: &mut PollWorld, secs: u64) {
    let dropped = world.dropped_at.expect("kein Abbruch");
    let first = world
        .bus
        .connection_log()
        .into_iter()
        .find(|(at, e)| *at >= dropped && matches!(e, ConnectionEvent::Connect { .. }))
        .expect("kein Verbindungsversuch")
        .0;
    assert_eq!(first - dropped, Duration::from_secs(secs));
}

#[then("wurde vor jedem Verbindungsversuch die vorige Session geschlossen")]
fn then_disconnect_before_connect(world: &mut PollWorld) {
    let log = world.bus.connection_log();
    let mut session_open = false;
    for (at, event) in &log {
        match event {
            ConnectionEvent::Connect { accepted } => {
                assert!(
                    !session_open,
                    "Connect bei {at:?} ohne vorheriges Schließen: {log:?}"
                );
                session_open = *accepted;
            }
            ConnectionEvent::Disconnect => session_open = false,
            ConnectionEvent::Dropped => {}
        }
    }
}

#[then(expr = "ist der Verbindungszustand {string} mit {int} Versuchen")]
fn then_connection_state(world: &mut PollWorld, state: String, attempts: u32) {
    let stats = world.lab().poll_stats();
    let expected = match state.as_str() {
        "getrennt" => ConnectionState::Disconnected,
        "verbunden" => ConnectionState::Connected,
        other => panic!("unbekannter Zustand {other}"),
    };
    assert_eq!(
        (stats.connection, stats.reconnect_attempts),
        (expected, attempts)
    );
}

#[then("meldet die Setz-Sequenz \"keine Verbindung\" ohne Wartezeit")]
fn then_set_not_connected(world: &mut PollWorld) {
    let result = world.set_result.clone().expect("kein Setz-Ergebnis");
    assert_eq!(result, Err(SetReadBackError::NotConnected));
    let waited = world.set_finished_at.unwrap() - world.set_requested_at.unwrap();
    assert_eq!(waited, Duration::ZERO, "Setzen wartete {waited:?}");
}

#[then("meldet die Setz-Sequenz Verbindungsverlust ohne Quittung")]
fn then_set_lost_without_ack(world: &mut PollWorld) {
    let result = world.set_result.clone().expect("kein Setz-Ergebnis");
    assert_eq!(result, Err(SetReadBackError::ConnectionLost { ack: None }));
}

#[then(expr = "meldet die Setz-Sequenz Verbindungsverlust mit Quittung {string}")]
fn then_set_lost_with_ack(world: &mut PollWorld, status: String) {
    let result = world.set_result.clone().expect("kein Setz-Ergebnis");
    assert_eq!(
        result,
        Err(SetReadBackError::ConnectionLost {
            ack: Some(Ack {
                code: 0.0,
                status_text: Some(status),
            })
        })
    );
}

#[then(expr = "wurde {string} insgesamt {int} Mal gesendet")]
fn then_sent_times_total(world: &mut PollWorld, line: String, n: usize) {
    let sent = world.bus.sent();
    let count = sent.iter().filter(|(_, l)| *l == line).count();
    assert_eq!(count, n, "Sende-Log: {sent:?}");
}

#[then("ist jeder Kanal als veraltet markiert und behält seinen Wert")]
fn then_all_stale_keep_values(world: &mut PollWorld) {
    let states = world.lab().subscribe_channels().0;
    assert!(!states.is_empty());
    for state in &states {
        assert!(state.stale && state.value.is_some(), "Zustand {state:?}");
    }
}

#[then(expr = "meldete der Änderungsstrom für {string} zuletzt {string}")]
fn then_last_change(world: &mut PollWorld, channel: String, expected: String) {
    world.drain_changes();
    let channel = key(&channel);
    let last = world
        .change_log
        .iter()
        .rev()
        .find(|u| u.key == channel)
        .map(|u| (u.value, u.stale))
        .expect("keine Meldung");
    let expected = match expected.strip_suffix(" veraltet") {
        Some(v) => (Some(v.parse().unwrap()), true),
        None => (Some(expected.parse().unwrap()), false),
    };
    assert_eq!(last, expected);
}

#[then("ist der Start mit einem Verbindungsfehler gescheitert")]
fn then_spawn_failed(world: &mut PollWorld) {
    assert!(world.spawn_error.is_some(), "Start gelang unerwartet");
}

#[then(expr = "gab es insgesamt genau {int} Verbindungsversuch(e)")]
fn then_connect_count(world: &mut PollWorld, n: usize) {
    let log = world.bus.connection_log();
    let connects = log
        .iter()
        .filter(|(_, e)| matches!(e, ConnectionEvent::Connect { .. }))
        .count();
    assert_eq!(connects, n, "Protokoll: {log:?}");
}

/// Zählt UND prüft Zeitpunkte: Mit pausierter Uhr läuft eine
/// Dauerschleife komplett im selben virtuellen Zeitpunkt ab - ein reiner
/// Zeitvergleich würde sie übersehen. Erlaubt ist genau der eine Lesevorgang,
/// der den Abbruch bemerkt.
#[then("wurde die getrennte Verbindung höchstens einmal gelesen, beim Abbruch selbst")]
fn then_io_only_at_drop(world: &mut PollWorld) {
    let dropped = world.dropped_at.expect("kein Abbruch");
    let io: Vec<(Instant, IoAttempt)> = world.bus.io_while_disconnected();
    assert!(
        io.len() <= 1,
        "{} Lese-/Sendeversuche auf getrennter Verbindung",
        io.len()
    );
    assert!(
        io.iter().all(|(at, _)| *at == dropped),
        "Lese-/Sendeversuche nach dem Abbruch: {:?}",
        io.iter()
            .map(|(at, a)| (*at - dropped, *a))
            .collect::<Vec<_>>()
    );
}

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    let features = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/polling_features");
    // fail_on_skipped(): siehe tests/cucumber.rs - ein Step ohne Treffer
    // würde sonst unbemerkt als "übersprungen" durchgehen.
    PollWorld::cucumber()
        .max_concurrent_scenarios(1)
        .fail_on_skipped()
        .run_and_exit(features)
        .await;
}
