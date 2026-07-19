//! Spec 0003 AK4/AK5: `POST /api/channel/{addr}/{sub}` setzt einen
//! Kanalwert, wartet die Quittung (Subkanal 255) ab und liest den Kanal
//! zurück. Die Fehlerfälle (Ablehnung, keine Antwort) sind über den
//! HTTP-Status unterscheidbar; wegen des am realen Gerät verifizierten
//! Klemm-Verhaltens (PARERR + trotzdem veränderter Wert) enthält auch die
//! Ablehnungs-Antwort den zurückgelesenen Ist-Wert.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use octlab_server::build_app_with_connection;
use octlab_transport::SimulatedConnection;
use std::path::PathBuf;
use tower::util::ServiceExt;

/// Schickt einen Setz-Request gegen einen Router mit der übergebenen,
/// vorpräparierten Verbindung und liefert Status + geparstes JSON zurück.
async fn post_set(
    connection: SimulatedConnection,
    uri: &str,
    value: f64,
) -> (StatusCode, serde_json::Value) {
    // Frontend-Verzeichnis ist für die API-Routen irrelevant - ein nicht
    // existierender Pfad reicht (der Fallback wird hier nie angefragt).
    let dist = PathBuf::from("/nonexistent-frontend-dist");
    let app = build_app_with_connection(Box::new(connection), dist)
        .await
        .expect("build_app_with_connection mit Simulation darf nicht fehlschlagen");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"value":{value}}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

// AK4: Server-Endpoint setzt und liest zurück
#[tokio::test]
async fn ok_quittung_liefert_status_und_ruecklesewert() {
    let mut conn = SimulatedConnection::new("test-sim");
    // Sequenz wie am realen Gerät beobachtet: Set-Kommando -> Quittung auf
    // Subkanal 255, dann Query -> Messwert. push_reply gibt jede Antwort
    // erst mit dem zugehörigen gesendeten Kommando frei.
    conn.push_reply("#4:255=0 [OK]");
    conn.push_reply("#4:0=2500.0");

    let (status, json) = post_set(conn, "/api/channel/4/0", 2500.0).await;

    assert_eq!(status, StatusCode::OK, "Antwort war: {json}");
    assert_eq!(json["ack"], "OK");
    assert_eq!(json["value"], 2500.0);
}

// AK5, Fall 1: Ablehnung (mit Klemmung) -> kein 2xx, Statustext + Ist-Wert
#[tokio::test]
async fn ablehnung_liefert_statustext_und_ist_wert() {
    let mut conn = SimulatedConnection::new("test-sim");
    // Real beobachtet: 4:0=2000000! -> PARERR, aber der Wert wurde auf das
    // Maximum geklemmt - das Rücklesen zeigt 999999.8.
    conn.push_reply("#4:255=5 [PARERR]");
    conn.push_reply("#4:0=999999.8");

    let (status, json) = post_set(conn, "/api/channel/4/0", 2000000.0).await;

    assert!(!status.is_success(), "Ablehnung darf kein 2xx sein: {json}");
    assert_eq!(json["ack"], "PARERR");
    assert_eq!(json["value"], 999999.8);
}

// AK5, Fall 2: keine Antwort -> kein 2xx, Grund nennt den Timeout
#[tokio::test]
async fn keine_antwort_liefert_timeout_fehler() {
    let conn = SimulatedConnection::new("test-sim"); // keine Antworten präpariert

    let (status, json) = post_set(conn, "/api/channel/4/0", 2500.0).await;

    assert!(
        !status.is_success(),
        "keine Antwort darf kein 2xx sein: {json}"
    );
    let error = json["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("Timeout") || error.contains("keine Antwort"),
        "Fehlertext nennt den Grund nicht: {json}"
    );
}
