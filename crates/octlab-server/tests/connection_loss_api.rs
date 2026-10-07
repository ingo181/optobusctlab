//! Spec 0008, Server-Ebene: Verbindungszustand in `GET /api/poll` (AK6) und
//! die Antworten von `POST /api/channel/...` bei Verbindungsverlust (AK3):
//! 503 "keine Verbindung zur Anlage" (nie gesendet) bzw. 503 "Verbindung
//! während des Setzens verloren ..." (Zustand unbekannt). 504 bleibt der
//! Fall "keine Quittung".
//!
//! Pausierte Tokio-Uhr, Requests per `oneshot` (keine echten Sockets), die
//! Module simuliert `SimBus` mit Abbruch und Wiederverbindung.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use octlab_lab::PollConfig;
use octlab_protocol::{ChannelKey, ModuleAddress, SubChannel};
use octlab_server::build_app_with_connection;
use octlab_transport::{SimBus, SimulatedConnection};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use tower::util::ServiceExt;

const LOST_TEXT: &str =
    "Verbindung während des Setzens verloren, Zustand unbekannt, nur Rücklesen ist verbindlich";

fn key(address: u8, subchannel: u8) -> ChannelKey {
    ChannelKey {
        address: ModuleAddress(address),
        subchannel: SubChannel(subchannel),
    }
}

async fn app_with_bus(bus: &SimBus, interval: Duration) -> Router {
    let connection = SimulatedConnection::new("sim").with_bus(bus.clone());
    let config = PollConfig {
        channels: vec![key(1, 0), key(4, 0)],
        interval,
    };
    build_app_with_connection(
        Box::new(connection),
        PathBuf::from("/nonexistent-frontend-dist"),
        config,
    )
    .await
    .unwrap()
}

async fn request(app: &Router, method: &str, uri: &str, body: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn bus_with_values() -> SimBus {
    let bus = SimBus::new();
    bus.set_value(1, 0, "0.0022");
    bus.set_value(4, 0, "1000.0");
    bus
}

// AK6: verbunden -> "connected", 0 Versuche
#[tokio::test(start_paused = true)]
async fn poll_meldet_verbunden() {
    let bus = bus_with_values();
    let app = app_with_bus(&bus, Duration::from_millis(1000)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (status, json) = request(&app, "GET", "/api/poll", "").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["connection"], "connected", "Antwort: {json}");
    assert_eq!(json["reconnect_attempts"], 0);
    // Bisherige Felder bleiben (Spec 0005 AK10).
    assert_eq!(json["interval_ms"], 1000);
}

// AK6: getrennt nach 3 erfolglosen Versuchen (bei 1, 3, 7 s) -> "disconnected", 3
#[tokio::test(start_paused = true)]
async fn poll_meldet_getrennt_mit_versuchen() {
    let bus = bus_with_values();
    let app = app_with_bus(&bus, Duration::from_millis(1000)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    bus.refuse_connections(true);
    bus.drop_connection();

    tokio::time::sleep(Duration::from_millis(8000)).await;
    let (_, json) = request(&app, "GET", "/api/poll", "").await;

    assert_eq!(json["connection"], "disconnected", "Antwort: {json}");
    assert_eq!(json["reconnect_attempts"], 3);
}

// AK3: Verbindung schon getrennt -> 503 "keine Verbindung zur Anlage", ohne Wert
#[tokio::test(start_paused = true)]
async fn setzen_ohne_verbindung_liefert_503() {
    let bus = bus_with_values();
    let app = app_with_bus(&bus, Duration::from_millis(1000)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    bus.refuse_connections(true);
    bus.drop_connection();
    tokio::time::sleep(Duration::from_millis(10)).await;

    let (status, json) = request(&app, "POST", "/api/channel/4/0", r#"{"value":2500}"#).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "Antwort: {json}");
    assert_eq!(
        json,
        json!({"ack": null, "value": null, "error": "keine Verbindung zur Anlage"})
    );
}

/// Startet das Setzen und bricht die Verbindung nach `drop_after` ab.
async fn set_with_drop(drop_after: Duration) -> (StatusCode, Value) {
    let bus = bus_with_values();
    // Jedes Modul antwortet nach 100 ms: Quittung bei 100 ms, Rücklesewert
    // bei 200 ms (gerechnet ab dem Setz-Kommando).
    bus.set_reply_delay(Duration::from_millis(100));
    // Polling aus, damit das Setzen sofort an der Reihe ist.
    let app = app_with_bus(&bus, Duration::ZERO).await;
    let dropper = bus.clone();
    let drop_later = async move {
        tokio::time::sleep(drop_after).await;
        dropper.drop_connection();
    };
    let (response, ()) = tokio::join!(
        request(&app, "POST", "/api/channel/4/0", r#"{"value":2500}"#),
        drop_later
    );
    response
}

// AK3: Abbruch nach dem Senden, vor der Quittung -> 503 "verloren", ohne Quittung
#[tokio::test(start_paused = true)]
async fn abbruch_vor_der_quittung_liefert_503_verloren_ohne_quittung() {
    let (status, json) = set_with_drop(Duration::from_millis(50)).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "Antwort: {json}");
    assert_eq!(
        json,
        json!({"ack": null, "value": null, "error": LOST_TEXT})
    );
}

// AK3: Abbruch nach der Quittung, vor dem Rücklesewert -> 503 "verloren", MIT Quittung
#[tokio::test(start_paused = true)]
async fn abbruch_nach_der_quittung_liefert_503_verloren_mit_quittung() {
    let (status, json) = set_with_drop(Duration::from_millis(150)).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "Antwort: {json}");
    assert_eq!(
        json,
        json!({"ack": "OK", "value": null, "error": LOST_TEXT})
    );
}
