//! Dünne HTTP/WebSocket-Schicht über `octlab-lab`.
//!
//! Läuft unverändert an zwei Orten:
//! - als eigenständiger Prozess auf dem Raspberry Pi (systemd-Unit),
//!   Browser im Kiosk-Modus zeigt die (später hier angebundene) Leptos-UI
//! - eingebettet in der Tauri-Desktop-App (Tauri startet diesen Server
//!   intern und zeigt dieselbe UI in seiner eigenen WebView)
//!
//! Default ist die `SimulatedConnection`, damit `cargo run` sofort ohne
//! c't-Lab funktioniert. Echte Hardware nur explizit über
//! `--connection tcp --addr <host:port>` (siehe [`Cli`]).

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use clap::Parser;
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
enum ConnectionKind {
    Simulation,
    Tcp,
}

#[derive(Debug, Parser)]
#[command(about = "octlab-server – Steuer-/Mess-Server für das c't-Lab")]
struct Cli {
    /// Verbindungsebene: `simulation` (Default, hardware-frei) oder `tcp` (XPort).
    #[arg(long, value_enum, default_value_t = ConnectionKind::Simulation)]
    connection: ConnectionKind,

    /// TCP-Adresse des XPort, z.B. `192.168.1.104:10001`. Nur bei
    /// `--connection tcp` nötig (und dann auch erforderlich).
    #[arg(long, required_if_eq("connection", "tcp"))]
    addr: Option<String>,
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let connection: Box<dyn BoardConnection> = match cli.connection {
        ConnectionKind::Simulation => Box::new(SimulatedConnection::new("dev-simulation")),
        ConnectionKind::Tcp => {
            // `addr` ist dank `required_if_eq` in Cli garantiert gesetzt.
            let addr = cli.addr.expect("--addr ist bei --connection tcp Pflicht");
            Box::new(TcpConnection::new(addr))
        }
    };

    // Fail-fast: `Lab::spawn()` verbindet VOR dem Zurückkehren (siehe dessen
    // Doc-Kommentar) - ein unerreichbares XPort beendet den Prozess hier
    // sofort mit klarer Fehlermeldung, statt still einen Actor zu starten,
    // dessen Verbindung im Hintergrund scheitert und dessen Queries dann nur
    // endlos timeouten würden. Die zusätzliche `timeout()` hier fängt den
    // Fall ab, dass `connect()` selbst nicht zeitnah scheitert (z.B. ein
    // gefiltertes statt aktiv abgelehntes TCP-SYN hängt sonst an den
    // OS-Timeouts, die deutlich über 3s liegen können).
    let lab = match tokio::time::timeout(Duration::from_secs(3), Lab::spawn(connection)).await {
        Ok(Ok(lab)) => Arc::new(lab),
        Ok(Err(err)) => {
            eprintln!("c't-Lab-Verbindung fehlgeschlagen: {err}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("c't-Lab-Verbindung fehlgeschlagen: Timeout nach 3s");
            std::process::exit(1);
        }
    };

    // PROVISORIUM: nur wenn wirklich Hardware dranhängt, macht ein Poll
    // überhaupt Sinn (SimulatedConnection hat sowieso keine Warteschlange
    // gefüllt) - siehe Doc-Kommentar an `poll_div_provisional`.
    if cli.connection == ConnectionKind::Tcp {
        tokio::spawn(poll_div_provisional(lab.clone()));
    }

    let state = AppState { lab };

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Port 3000 konnte nicht gebunden werden");
    tracing::info!("octlab-server läuft auf http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}

// PROVISORIUM: statische Seite ohne Framework, nur zum Live-Beweis der
// TcpConnection-Anbindung. Fliegt komplett raus, sobald apps/web (Leptos)
// steht - siehe CLAUDE.md, Abschnitt "Nächste Schritte".
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
