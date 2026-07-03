//! Dünne HTTP/WebSocket-Schicht über `octlab-lab`.
//!
//! Läuft unverändert an zwei Orten:
//! - als eigenständiger Prozess auf dem Raspberry Pi (systemd-Unit),
//!   Browser im Kiosk-Modus zeigt die (später hier angebundene) Leptos-UI
//! - eingebettet in der Tauri-Desktop-App (Tauri startet diesen Server
//!   intern und zeigt dieselbe UI in seiner eigenen WebView)
//!
//! Absichtlich noch OHNE echte Hardware-Anbindung: Default ist die
//! `SimulatedConnection`, damit `cargo run` sofort ohne c't-Lab funktioniert.
//! Die TCP-Verbindung zum XPort (Port 10001) ist der nächste Schritt.

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use octlab_lab::Lab;
use octlab_protocol::Message as LabMessage;
use octlab_transport::SimulatedConnection;
use serde::Serialize;
use std::sync::Arc;

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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // TODO: hier später per Config/CLI-Flag zwischen SimulatedConnection,
    // TcpConnection (XPort) und SerialConnection wählen.
    let connection = SimulatedConnection::new("dev-simulation");
    let lab = Arc::new(Lab::spawn(Box::new(connection)));

    let state = AppState { lab };

    let app = Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Port 3000 konnte nicht gebunden werden");
    tracing::info!("octlab-server läuft auf http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
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
