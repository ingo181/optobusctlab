//! Spec 0002 AK6/AK7: Der Server serviert die Trunk-Build-Ausgabe
//! (`apps/web/dist`) statt des früheren eingebetteten Provisoriums-HTML.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use octlab_server::{build_app, ConnectionKind};
use std::path::PathBuf;
use tower::util::ServiceExt;

/// Legt ein frisches, eindeutiges Wegwerf-Verzeichnis unterhalb des
/// System-Tempdirs an (bewusst ohne `tempfile`-Dependency - für zwei Tests
/// reicht Prozess-ID + Marker als Eindeutigkeit).
fn temp_dist(marker: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "octlab-frontend-test-{}-{marker}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("Temp-Verzeichnis anlegen");
    dir
}

async fn get_root(frontend_dist: PathBuf) -> (StatusCode, String) {
    let app = build_app(ConnectionKind::Simulation, None, frontend_dist)
        .await
        .expect("build_app mit Simulation darf nicht fehlschlagen");
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

// AK6: Server liefert das gebaute Frontend aus
#[tokio::test]
async fn liefert_index_html_aus_dist_verzeichnis() {
    let dist = temp_dist("ak6");
    let marker = "<!-- octlab-testmarker-ak6 -->";
    std::fs::write(dist.join("index.html"), format!("<html>{marker}</html>"))
        .expect("index.html schreiben");

    let (status, body) = get_root(dist.clone()).await;
    std::fs::remove_dir_all(&dist).ok();

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(marker), "Body war: {body}");
}

// AK7: Fehlendes Frontend-Build erklärt sich selbst
#[tokio::test]
async fn fehlendes_dist_verzeichnis_nennt_trunk_build_als_abhilfe() {
    let dist = std::env::temp_dir().join("octlab-frontend-test-gibt-es-nicht");
    assert!(!dist.exists());

    let (status, body) = get_root(dist).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("trunk build"), "Body war: {body}");
}
