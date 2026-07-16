//! Wiederverwendbare HTTP/WebSocket-Bausteine über `octlab-lab`.
//!
//! Getrennt von `main.rs`, damit `apps/desktop` (Tauri) denselben
//! Router-Aufbau nutzen kann wie der eigenständige `octlab-server`-Prozess,
//! ohne dessen CLI-Parsing (`clap`) mitzuschleppen - Tauri startet den
//! Server intern, es gibt dort keine Kommandozeile. `main.rs` bleibt ein
//! dünner Wrapper: `Cli::parse()`, dann [`build_app`] aufrufen.

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use octlab_lab::Lab;
use octlab_protocol::{ChannelKey, Message as LabMessage, ModuleAddress, SubChannel};
use octlab_transport::{BoardConnection, SimulatedConnection, TcpConnection};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

/// Wahl der Verbindungsebene beim Start. Default `Simulation` – sicher,
/// läuft überall ohne angeschlossenes c't-Lab. `Tcp` ist bewusst nur
/// explizit wählbar, nie automatisch erraten (z.B. per Auto-Discovery),
/// damit auf dem Pi kein Server versehentlich gegen echte Hardware sendet,
/// wenn eigentlich nur ein UI-Test gemeint war.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ConnectionKind {
    Simulation,
    Tcp,
}

/// Über die Leitung ans Frontend geschicktes JSON – bewusst getrennt von
/// `octlab_protocol::Message`, damit die Protokoll-Ebene nicht von serde
/// abhängen muss (Layer-Trennung wie im CtLab-Library-Vorbild).
#[derive(Debug, Serialize)]
struct MeasurementDto {
    address: u8,
    subchannel: u8,
    value: f64,
    status_text: Option<String>,
}

impl From<LabMessage> for MeasurementDto {
    fn from(msg: LabMessage) -> Self {
        Self {
            address: msg.key.address.0,
            subchannel: msg.key.subchannel.0,
            value: msg.value,
            status_text: msg.status_text,
        }
    }
}

#[derive(Clone)]
struct AppState {
    lab: Arc<Lab>,
}

/// Baut Lab-Actor + Router fertig auf. Fail-fast: verbindet VOR der
/// Rückgabe (`Lab::spawn` selbst, siehe dessen Doc-Kommentar in
/// `octlab-lab`, plus ein zusätzliches Timeout hier für den Fall, dass
/// `connect()` nicht zeitnah scheitert) - ein unerreichbares XPort liefert
/// hier einen `Err` statt einen Router zurückzugeben, dessen Queries dann
/// nur endlos timeouten würden. Aufrufer (`main.rs`, `apps/desktop`)
/// entscheiden selbst, wie sie den Fehler melden (Prozess beenden vs.
/// Tauri-Fehlerdialog).
pub async fn build_app(connection: ConnectionKind, addr: Option<String>) -> Result<Router, String> {
    let boxed_connection: Box<dyn BoardConnection> = match connection {
        ConnectionKind::Simulation => Box::new(SimulatedConnection::new("dev-simulation")),
        ConnectionKind::Tcp => {
            let addr = addr.ok_or_else(|| "--addr ist bei --connection tcp Pflicht".to_string())?;
            Box::new(TcpConnection::new(addr))
        }
    };

    let lab = match tokio::time::timeout(Duration::from_secs(3), Lab::spawn(boxed_connection)).await
    {
        Ok(Ok(lab)) => Arc::new(lab),
        Ok(Err(err)) => return Err(format!("c't-Lab-Verbindung fehlgeschlagen: {err}")),
        Err(_) => return Err("c't-Lab-Verbindung fehlgeschlagen: Timeout nach 3s".to_string()),
    };

    // PROVISORIUM: nur wenn wirklich Hardware dranhängt, macht ein Poll
    // überhaupt Sinn (SimulatedConnection hat sowieso keine Warteschlange
    // gefüllt) - siehe Doc-Kommentar an `poll_div_provisional`.
    if connection == ConnectionKind::Tcp {
        tokio::spawn(poll_div_provisional(lab.clone()));
    }

    let state = AppState { lab };

    Ok(Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .with_state(state))
}

// PROVISORIUM: statische Seite ohne Framework, nur zum Live-Beweis der
// TcpConnection-Anbindung (jetzt auch für apps/desktop). Fliegt komplett
// raus, sobald apps/web (Leptos) steht - siehe CLAUDE.md, Abschnitt
// "Nächste Schritte".
async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

/// PROVISORIUM: pollt einen einzelnen fest verdrahteten Kanal (DIV, Adresse
/// 1, Subkanal 0 - siehe "Verifizierte Hardware-Fakten" in CLAUDE.md) alle
/// 500ms, rein damit die statische Seite unter `/` überhaupt Live-Werte zu
/// sehen bekommt. `query()`s Ergebnis wird bewusst ignoriert (`let _ =`) -
/// die eigentliche Zustellung an WebSocket-Clients passiert unabhängig
/// davon in `Lab::dispatch`, das JEDE eingehende Nachricht broadcastet,
/// nicht nur Query-Antworten. Fliegt raus, sobald `apps/web` eine echte
/// Subscription-/Sweep-Logik mitbringt - siehe CLAUDE.md, Abschnitt
/// "Nächste Schritte".
async fn poll_div_provisional(lab: Arc<Lab>) {
    let key = ChannelKey {
        address: ModuleAddress(1),
        subchannel: SubChannel(0),
    };
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    loop {
        interval.tick().await;
        let _ = lab.query(key).await;
    }
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| stream_measurements(socket, state))
}

/// Streamt jede vom Lab empfangene Nachricht sofort als JSON-Zeile an den
/// verbundenen Browser – kein Polling, echtes Push via `broadcast::Receiver`.
async fn stream_measurements(mut socket: WebSocket, state: AppState) {
    let mut updates = state.lab.subscribe();

    while let Ok(msg) = updates.recv().await {
        let dto: MeasurementDto = msg.into();
        let payload = match serde_json::to_string(&dto) {
            Ok(json) => json,
            Err(err) => {
                tracing::warn!(?err, "Serialisierung fehlgeschlagen");
                continue;
            }
        };
        if socket.send(WsMessage::Text(payload)).await.is_err() {
            break; // Client hat die Verbindung geschlossen
        }
    }
}
