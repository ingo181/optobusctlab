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
        Path, State,
    },
    handler::HandlerWithoutStateExt,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use octlab_lab::{Lab, SetOutcome};
use octlab_protocol::{ChannelKey, Message as LabMessage, ModuleAddress, SubChannel};
use octlab_transport::{BoardConnection, SimulatedConnection, TcpConnection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tower_http::services::ServeDir;

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
pub async fn build_app(
    connection: ConnectionKind,
    addr: Option<String>,
    frontend_dist: PathBuf,
) -> Result<Router, String> {
    let boxed_connection: Box<dyn BoardConnection> = match connection {
        ConnectionKind::Simulation => Box::new(SimulatedConnection::new("dev-simulation")),
        ConnectionKind::Tcp => {
            let addr = addr.ok_or_else(|| "--addr ist bei --connection tcp Pflicht".to_string())?;
            Box::new(TcpConnection::new(addr))
        }
    };

    let lab = spawn_lab(boxed_connection).await?;

    // PROVISORIUM: nur wenn wirklich Hardware dranhängt, macht ein Poll
    // überhaupt Sinn (SimulatedConnection hat sowieso keine Warteschlange
    // gefüllt) - siehe Doc-Kommentar an `poll_div_provisional`.
    if connection == ConnectionKind::Tcp {
        tokio::spawn(poll_div_provisional(lab.clone()));
    }

    Ok(build_router(lab, frontend_dist))
}

/// Wie [`build_app`], aber ohne Frontend-Fallback - für Aufrufer, die die
/// Frontend-Auslieferung selbst bestimmen (Spec 0004: `apps/desktop`
/// bettet die Trunk-Build-Ausgabe zur Compile-Zeit per `rust-embed` in SEIN
/// EIGENES Binary ein und hängt dafür einen eigenen `.fallback(...)` an den
/// zurückgegebenen Router). Bewusst NICHT als Cargo-Feature in diesem Crate
/// gelöst (erster Versuch, verworfen): ein Feature, das bestehendes
/// Verhalten ERSETZT statt rein additiv zu erweitern, wird von Cargos
/// Feature-Unification über den gesamten Workspace hinweg unifiziert -
/// `cargo test --workspace` hätte `octlab-server`s EIGENE Tests unbemerkt
/// mit dem von `apps/desktop` gewünschten Feature kompiliert (weil beide im
/// selben Build-Graph landen), und genau das brach `frontend_dist.rs`
/// (ServeDir-Tests liefen plötzlich gegen eingebettete statt Platten-Inhalte).
/// Deshalb bleibt `rust-embed` eine reine `apps/desktop`-Abhängigkeit, dieser
/// Crate liefert nur den unfertigen Router.
pub async fn build_app_without_frontend(
    connection: ConnectionKind,
    addr: Option<String>,
) -> Result<Router, String> {
    let boxed_connection: Box<dyn BoardConnection> = match connection {
        ConnectionKind::Simulation => Box::new(SimulatedConnection::new("dev-simulation")),
        ConnectionKind::Tcp => {
            let addr = addr.ok_or_else(|| "--addr ist bei --connection tcp Pflicht".to_string())?;
            Box::new(TcpConnection::new(addr))
        }
    };

    let lab = spawn_lab(boxed_connection).await?;

    if connection == ConnectionKind::Tcp {
        tokio::spawn(poll_div_provisional(lab.clone()));
    }

    Ok(api_router(lab))
}

/// Wie [`build_app`], aber mit einer bereits fertig präparierten Verbindung
/// statt der `ConnectionKind`-Auswahl - für Tests, die eine
/// `SimulatedConnection` mit Skript-Antworten (`push_reply`) hineingeben
/// wollen. Startet bewusst KEINEN Provisoriums-Poll.
pub async fn build_app_with_connection(
    connection: Box<dyn BoardConnection>,
    frontend_dist: PathBuf,
) -> Result<Router, String> {
    let lab = spawn_lab(connection).await?;
    Ok(build_router(lab, frontend_dist))
}

async fn spawn_lab(connection: Box<dyn BoardConnection>) -> Result<Arc<Lab>, String> {
    match tokio::time::timeout(Duration::from_secs(3), Lab::spawn(connection)).await {
        Ok(Ok(lab)) => Ok(Arc::new(lab)),
        Ok(Err(err)) => Err(format!("c't-Lab-Verbindung fehlgeschlagen: {err}")),
        Err(_) => Err("c't-Lab-Verbindung fehlgeschlagen: Timeout nach 3s".to_string()),
    }
}

/// Health/WS/API-Routen ohne Frontend-Fallback - gemeinsame Basis für
/// [`build_router`] (ServeDir-Fallback) und [`build_app_without_frontend`]
/// (Fallback bleibt Sache des Aufrufers).
fn api_router(lab: Arc<Lab>) -> Router {
    let state = AppState { lab };

    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .route("/api/channel/:addr/:sub", post(set_channel))
        .with_state(state)
}

fn build_router(lab: Arc<Lab>, frontend_dist: PathBuf) -> Router {
    // Alles, was keine API-Route ist, kommt aus der Trunk-Build-Ausgabe von
    // `apps/web` (`ServeDir` liefert für `/` automatisch die `index.html`).
    // Fehlt das Verzeichnis bzw. die Datei, erklärt der Fallback die Abhilfe,
    // statt kommentarlos 404 zu antworten - der häufigste Stolperer ist ein
    // frisch geklontes Repo, in dem `trunk build` schlicht noch nie lief.
    let frontend = ServeDir::new(frontend_dist).not_found_service(missing_frontend.into_service());
    api_router(lab).fallback_service(frontend)
}

/// Request-Body für `POST /api/channel/{addr}/{sub}`.
#[derive(Debug, Deserialize)]
struct SetChannelRequest {
    value: f64,
}

/// Antwort auf einen Setz-Request - DTO bleibt bewusst in `octlab-server`
/// (Layer-Trennung, wie `MeasurementDto`).
#[derive(Debug, Serialize)]
struct SetChannelResponse {
    /// Quittungs-Statustext der Anlage (z.B. "OK", "PARERR"), falls eine
    /// Quittung kam.
    ack: Option<String>,
    /// Nach der Quittung zurückgelesener Ist-Wert. Wegen des am realen
    /// Gerät verifizierten Klemm-Verhaltens (Spec 0003: PARERR heißt NICHT
    /// "Wert unverändert") auch im Ablehnungsfall gefüllt, wenn das
    /// Rücklesen klappte.
    value: Option<f64>,
    /// Fehlerbeschreibung, falls kein regulärer Quittung+Rücklesen-Ablauf
    /// zustande kam.
    error: Option<String>,
}

/// Setzt einen Kanalwert: Set-Kommando senden, Quittung (Subkanal 255)
/// abwarten, Kanal rücklesen (Spec 0003, AK4/AK5). Generisch über
/// Adresse/Subkanal - die DDS-Frequenz ist nur der erste Nutzer.
async fn set_channel(
    Path((addr, sub)): Path<(u8, u8)>,
    State(state): State<AppState>,
    Json(request): Json<SetChannelRequest>,
) -> impl IntoResponse {
    let key = ChannelKey {
        address: ModuleAddress(addr),
        subchannel: SubChannel(sub),
    };

    match state.lab.set(key, request.value).await {
        SetOutcome::NoReply => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(SetChannelResponse {
                ack: None,
                value: None,
                error: Some("keine Antwort vom Modul (Timeout)".to_string()),
            }),
        ),
        SetOutcome::Confirmed { status_text } => {
            let ack = Some(status_text.unwrap_or_else(|| "OK".to_string()));
            match state.lab.query(key).await {
                Some(value) => (
                    StatusCode::OK,
                    Json(SetChannelResponse {
                        ack,
                        value: Some(value),
                        error: None,
                    }),
                ),
                // Quittiert, aber Rücklesen ohne Antwort: der tatsächliche
                // Zustand ist unbekannt - das ist KEIN Erfolg (Spec 0003:
                // nur das Rücklesen bestätigt).
                None => (
                    StatusCode::GATEWAY_TIMEOUT,
                    Json(SetChannelResponse {
                        ack,
                        value: None,
                        error: Some("quittiert, aber Rücklesen ohne Antwort (Timeout)".to_string()),
                    }),
                ),
            }
        }
        SetOutcome::Rejected { code, status_text } => {
            let ack = Some(status_text.unwrap_or_else(|| format!("Fehlercode {code}")));
            // Auch bei Ablehnung rücklesen: die Firmware kann den Wert
            // trotz Fehlerquittung verändert haben (Klemmung).
            let value = state.lab.query(key).await;
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(SetChannelResponse {
                    ack,
                    value,
                    error: None,
                }),
            )
        }
    }
}

async fn missing_frontend() -> impl IntoResponse {
    (
        axum::http::StatusCode::NOT_FOUND,
        "Frontend-Build nicht gefunden. Einmal `trunk build` in apps/web ausführen \
         (bzw. --frontend-dist auf die Trunk-Ausgabe zeigen lassen).",
    )
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
