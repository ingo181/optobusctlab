//! Spec 0005: Kanalliste als Server-Konfiguration (`GET /api/channels`),
//! Poll-Statistik (`GET /api/poll`, AK10) und Polling auf jeder
//! Verbindungsart (AK11).
//!
//! Läuft mit pausierter Tokio-Uhr (`start_paused`, tokio-Feature `test-util`
//! nur unter `[dev-dependencies]`): Ein voller Zyklus über 12 nicht
//! antwortende Kanäle dauert 12 x 500 ms - virtuell vergeht das sofort.
//! Ohne echte Sockets (Requests per `oneshot`), damit die automatisch
//! vorlaufende Uhr nicht mit echtem Netzwerk-I/O konkurriert.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use octlab_server::{build_app, build_app_with_connection, poll_config, ConnectionKind};
use octlab_transport::SimulatedConnection;
use std::path::PathBuf;
use std::time::Duration;
use tower::util::ServiceExt;

fn no_frontend() -> PathBuf {
    PathBuf::from("/nonexistent-frontend-dist")
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// Die Kanalliste aus Spec 0005 ("Kanalliste für diese Spec"), 12 Einträge.
#[tokio::test(start_paused = true)]
async fn kanalliste_liefert_die_12_kanaele_aus_spec_0005() {
    let app = build_app_with_connection(
        Box::new(SimulatedConnection::new("sim")),
        no_frontend(),
        poll_config(Duration::ZERO),
    )
    .await
    .unwrap();

    let (status, json) = get_json(&app, "/api/channels").await;

    assert_eq!(status, StatusCode::OK);
    let expected = serde_json::json!([
        {"address": 0, "subchannel": 10, "name": "ADA AD16 Kanal 0", "unit": "V"},
        {"address": 0, "subchannel": 11, "name": "ADA AD16 Kanal 1", "unit": "V"},
        {"address": 0, "subchannel": 12, "name": "ADA AD16 Kanal 2", "unit": "V"},
        {"address": 0, "subchannel": 13, "name": "ADA AD16 Kanal 3", "unit": "V"},
        {"address": 1, "subchannel": 0, "name": "DIV Messwert", "unit": "abhängig von RNG"},
        {"address": 1, "subchannel": 19, "name": "DIV Messbereich", "unit": "Code"},
        {"address": 2, "subchannel": 10, "name": "DCG U Ist", "unit": "V"},
        {"address": 2, "subchannel": 11, "name": "DCG I Ist", "unit": "A"},
        {"address": 2, "subchannel": 0, "name": "DCG U Soll", "unit": "V"},
        {"address": 2, "subchannel": 1, "name": "DCG I Soll", "unit": "A"},
        {"address": 4, "subchannel": 0, "name": "DDS Frequenz", "unit": "Hz"},
        {"address": 4, "subchannel": 1, "name": "DDS Pegel", "unit": "mVeff"}
    ]);
    assert_eq!(json, expected);
}

/// AK11: Polling läuft auch in der Simulation - auf einer Verbindung, auf
/// der niemand antwortet, wird trotzdem jeder Kanal der Liste abgefragt,
/// und die Poll-Statistik ist abrufbar (AK10).
#[tokio::test(start_paused = true)]
async fn simulation_pollt_alle_kanaele_und_statistik_ist_abrufbar() {
    let connection = SimulatedConnection::new("sim");
    let sent = connection.sent_handle();
    let app = build_app_with_connection(
        Box::new(connection),
        no_frontend(),
        poll_config(Duration::from_millis(1000)),
    )
    .await
    .unwrap();

    // Erster Zyklus: 12 Timeouts à 500 ms.
    tokio::time::sleep(Duration::from_millis(6500)).await;

    let sent = sent.lock().unwrap().clone();
    for channel in octlab_server::CHANNELS {
        let query = format!("{}:{}?", channel.address, channel.subchannel);
        assert!(sent.contains(&query), "{query} nicht gesendet: {sent:?}");
    }

    let (status, json) = get_json(&app, "/api/poll").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["interval_ms"], 1000);
    assert_eq!(json["last_cycle_ms"], 6000);
    assert_eq!(json["completed_cycles"], 1);
    // 6000 ms > 1000 ms Intervall: genau eine Überlauf-Episode.
    assert_eq!(json["overrun_episodes"], 1);
}

/// AK11 über den echten Einstieg `build_app(Simulation, ...)`: auch dort
/// läuft das Polling (früher nur bei `--connection tcp`).
#[tokio::test(start_paused = true)]
async fn build_app_mit_simulation_startet_das_polling() {
    let app = build_app(
        ConnectionKind::Simulation,
        None,
        no_frontend(),
        Duration::from_millis(1000),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(6500)).await;

    let (_, json) = get_json(&app, "/api/poll").await;
    assert_eq!(json["interval_ms"], 1000);
    assert_eq!(json["completed_cycles"], 1);
}

/// AK11 / AK2: Intervall 0 schaltet das Polling ab.
#[tokio::test(start_paused = true)]
async fn intervall_0_sendet_keine_abfragen() {
    let connection = SimulatedConnection::new("sim");
    let sent = connection.sent_handle();
    let app = build_app_with_connection(
        Box::new(connection),
        no_frontend(),
        poll_config(Duration::ZERO),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(5000)).await;

    assert!(sent.lock().unwrap().is_empty());
    let (_, json) = get_json(&app, "/api/poll").await;
    assert_eq!(json["interval_ms"], 0);
    assert_eq!(json["completed_cycles"], 0);
    assert_eq!(json["last_cycle_ms"], serde_json::Value::Null);
}
