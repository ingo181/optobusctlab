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
use octlab_lab::{ChannelUpdate, Lab, PollConfig, PollStats};
use octlab_protocol::{ChannelKey, ModuleAddress, SubChannel};
use octlab_transport::{BoardConnection, SimulatedConnection, TcpConnection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

pub mod channels;
pub use channels::{poll_config, ChannelSpec, CHANNELS};

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

/// Über `/ws` ans Frontend geschicktes JSON – bewusst getrennt von den
/// Lab-Typen, damit Protokoll- und Lab-Ebene nicht von serde abhängen
/// müssen (Layer-Trennung wie im CtLab-Library-Vorbild).
///
/// Vertrag (Spec 0005, rückwärtskompatibel zu Spec 0002): die Felder aus
/// 0002 (`address`, `subchannel`, `value`, `status_text`) plus `stale`.
/// `value` ist `null`, wenn der Kanal noch nie geantwortet hat - ein Client
/// nach Spec 0002 (`value: f64`) verwirft so eine Nachricht (dort AK3),
/// statt einen falschen Wert anzuzeigen. Ein veralteter Kanal mit bekanntem
/// Wert schickt diesen letzten Wert mit, ein 0002-Client behält ihn also.
/// `status_text` ist seit Spec 0005 immer `null` (Quittungen und
/// IDN-Antworten laufen nicht mehr über `/ws`), bleibt aber im Format.
#[derive(Debug, Serialize)]
struct MeasurementDto {
    address: u8,
    subchannel: u8,
    value: Option<f64>,
    status_text: Option<String>,
    stale: bool,
}

impl From<ChannelUpdate> for MeasurementDto {
    fn from(update: ChannelUpdate) -> Self {
        Self {
            address: update.key.address.0,
            subchannel: update.key.subchannel.0,
            value: update.value,
            status_text: None,
            stale: update.stale,
        }
    }
}

/// Eintrag von `GET /api/channels`.
#[derive(Debug, Serialize)]
struct ChannelDto {
    address: u8,
    subchannel: u8,
    name: &'static str,
    unit: &'static str,
}

/// Antwort von `GET /api/poll` (Spec 0005, AK10).
#[derive(Debug, Serialize)]
struct PollStatsDto {
    interval_ms: u128,
    last_cycle_ms: Option<u128>,
    completed_cycles: u64,
    overrun_episodes: u64,
}

impl From<PollStats> for PollStatsDto {
    fn from(stats: PollStats) -> Self {
        Self {
            interval_ms: stats.interval.as_millis(),
            last_cycle_ms: stats.last_cycle.map(|d| d.as_millis()),
            completed_cycles: stats.completed_cycles,
            overrun_episodes: stats.overrun_episodes,
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
    poll_interval: Duration,
) -> Result<Router, String> {
    let lab = spawn_lab(
        new_connection(connection, addr)?,
        poll_config(poll_interval),
    )
    .await?;
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
    poll_interval: Duration,
) -> Result<Router, String> {
    let lab = spawn_lab(
        new_connection(connection, addr)?,
        poll_config(poll_interval),
    )
    .await?;
    Ok(api_router(lab))
}

/// Wie [`build_app`], aber mit einer bereits fertig präparierten Verbindung
/// statt der `ConnectionKind`-Auswahl und frei wählbarer Poll-Konfiguration -
/// für Tests, die eine `SimulatedConnection` (Skript-Antworten oder
/// `SimBus`) hineingeben wollen. Tests mit FIFO-Skript (`push_reply`)
/// brauchen Intervall 0, sonst nehmen Poll-Abfragen ihnen die Antworten weg.
pub async fn build_app_with_connection(
    connection: Box<dyn BoardConnection>,
    frontend_dist: PathBuf,
    poll: PollConfig,
) -> Result<Router, String> {
    let lab = spawn_lab(connection, poll).await?;
    Ok(build_router(lab, frontend_dist))
}

/// Polling läuft auf JEDER Verbindungsart, auch in der Simulation (Spec
/// 0005, AK11) - dort antwortet niemand, alle Kanäle werden "veraltet".
fn new_connection(
    connection: ConnectionKind,
    addr: Option<String>,
) -> Result<Box<dyn BoardConnection>, String> {
    Ok(match connection {
        ConnectionKind::Simulation => Box::new(SimulatedConnection::new("dev-simulation")),
        ConnectionKind::Tcp => {
            let addr = addr.ok_or_else(|| "--addr ist bei --connection tcp Pflicht".to_string())?;
            Box::new(TcpConnection::new(addr))
        }
    })
}

async fn spawn_lab(
    connection: Box<dyn BoardConnection>,
    poll: PollConfig,
) -> Result<Arc<Lab>, String> {
    match tokio::time::timeout(
        Duration::from_secs(3),
        Lab::spawn_with_polling(connection, poll),
    )
    .await
    {
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
        .route("/api/channels", get(list_channels))
        .route("/api/poll", get(poll_stats))
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
/// abwarten, Kanal rücklesen - als EINE atomare Sequenz im Lab-Actor
/// (Spec 0003 AK4/AK5, Spec 0005 AK6). Generisch über Adresse/Subkanal -
/// die DDS-Frequenz ist nur der erste Nutzer.
///
/// Abweichend von Spec 0003 wird auch ohne Quittung zurückgelesen
/// (Stellglied-Regel: verbindlich ist nur das Rücklesen). Fehlt die
/// Quittung, ist die Antwort trotzdem kein 2xx (504), enthält aber den
/// Rücklesewert, falls einer kam (Entscheidung "Option A", 2026-10-07).
async fn set_channel(
    Path((addr, sub)): Path<(u8, u8)>,
    State(state): State<AppState>,
    Json(request): Json<SetChannelRequest>,
) -> impl IntoResponse {
    let key = ChannelKey {
        address: ModuleAddress(addr),
        subchannel: SubChannel(sub),
    };

    // VORLÄUFIG (Spec 0008, Schritt 2): Die Fehlerfälle "keine Verbindung"
    // und "Verbindung während des Setzens verloren" werden wie bisher als
    // "keine Antwort" (504 ohne Wert) gemeldet. Die richtige 503-Antwort
    // kommt mit Tests in Schritt 3.
    let result = match state.lab.set_and_read_back(key, request.value).await {
        Ok(result) => result,
        Err(_) => octlab_lab::SetReadBack {
            ack: None,
            readback: None,
        },
    };
    let value = result.readback;
    match result.ack {
        None => {
            let error = match value {
                Some(_) => "keine Quittung (Timeout), Rücklesewert liegt vor",
                None => "keine Antwort vom Modul (Timeout)",
            };
            (
                StatusCode::GATEWAY_TIMEOUT,
                Json(SetChannelResponse {
                    ack: None,
                    value,
                    error: Some(error.to_string()),
                }),
            )
        }
        // Quittungs-Code 0 = angenommen.
        Some(ack) if ack.code == 0.0 => {
            let ack = Some(ack.status_text.unwrap_or_else(|| "OK".to_string()));
            match value {
                Some(_) => (
                    StatusCode::OK,
                    Json(SetChannelResponse {
                        ack,
                        value,
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
        // Ablehnung (z.B. PARERR): Ist-Wert trotzdem mitliefern - die
        // Firmware kann den Wert trotz Fehlerquittung verändert haben
        // (Klemmung).
        Some(ack) => {
            let code = ack.code;
            let ack = Some(
                ack.status_text
                    .unwrap_or_else(|| format!("Fehlercode {code}")),
            );
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

/// `GET /api/channels`: die Kanalliste der Übersicht (Spec 0005).
async fn list_channels() -> Json<Vec<ChannelDto>> {
    Json(
        CHANNELS
            .iter()
            .map(|c| ChannelDto {
                address: c.address,
                subchannel: c.subchannel,
                name: c.name,
                unit: c.unit,
            })
            .collect(),
    )
}

/// `GET /api/poll`: gemessene Zykluszeit und weitere Kennzahlen (AK10).
async fn poll_stats(State(state): State<AppState>) -> Json<PollStatsDto> {
    Json(state.lab.poll_stats().into())
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

async fn ws_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| stream_measurements(socket, state))
}

/// `/ws`-Vertrag (Spec 0005): beim Verbinden der aktuelle Stand aller
/// schon abgefragten Kanäle der Kanalliste (Snapshot), danach NUR Änderungen
/// von Wert oder Veraltet-Markierung. Unaufgeforderte Zeilen und Quittungen
/// erscheinen hier nie - sie ändern den Kanalzustand nicht (AK9) und werden
/// im Lab-Actor geloggt.
async fn stream_measurements(mut socket: WebSocket, state: AppState) {
    let (snapshot, mut changes) = state.lab.subscribe_channels();
    for update in snapshot {
        if send_update(&mut socket, update).await.is_err() {
            return;
        }
    }

    loop {
        match changes.recv().await {
            Ok(update) => {
                if send_update(&mut socket, update).await.is_err() {
                    return; // Client hat die Verbindung geschlossen
                }
            }
            // Client war zu langsam und hat Meldungen verpasst: mit einem
            // frischen Snapshot wieder aufsetzen, statt still Zwischenstände
            // zu verlieren.
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!(missed, "WebSocket-Client hinterher - sende Snapshot neu");
                let (snapshot, fresh) = state.lab.subscribe_channels();
                changes = fresh;
                for update in snapshot {
                    if send_update(&mut socket, update).await.is_err() {
                        return;
                    }
                }
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

async fn send_update(socket: &mut WebSocket, update: ChannelUpdate) -> Result<(), axum::Error> {
    let dto: MeasurementDto = update.into();
    match serde_json::to_string(&dto) {
        Ok(json) => socket.send(WsMessage::Text(json)).await,
        Err(err) => {
            tracing::warn!(?err, "Serialisierung fehlgeschlagen");
            Ok(())
        }
    }
}
