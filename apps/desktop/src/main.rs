//! Tauri-Gerüst: startet `octlab-server`s API (Health/WS/Set-Channel) intern
//! (als Tokio-Task im selben Prozess, kein Kindprozess) und öffnet ein
//! natives WebView-Fenster auf dessen `/`.
//!
//! Das Frontend (Trunk-Build-Ausgabe von `apps/web`) wird HIER, nicht in
//! `octlab-server`, per `rust-embed` zur Compile-Zeit eingebettet (Spec
//! 0004) - kein `dist`-Ordner auf der Zielmaschine nötig. Bewusst NICHT als
//! Cargo-Feature in `octlab-server` gelöst (siehe dessen Doc-Kommentar an
//! `build_app_without_frontend`): das hätte Cargos Feature-Unification über
//! den ganzen Workspace ausgelöst und `octlab-server`s EIGENE
//! ServeDir-Tests unbemerkt kaputt gemacht, sobald `cargo test --workspace`
//! auch `apps/desktop` mitbaut. Diese App holt sich stattdessen nur den
//! fertigen API-Router (`build_app_without_frontend`) und hängt ihren
//! eigenen, eingebetteten Fallback selbst an - `rust-embed` ist dadurch
//! ausschließlich eine Abhängigkeit dieses Crates.
//!
//! `trunk build` muss vor diesem Build laufen; für `cargo tauri build`
//! erledigt das `beforeBuildCommand` in `tauri.conf.json` automatisch.
//!
//! Konfiguration bewusst minimal: `octlab-server` nimmt normalerweise
//! `--connection`/`--addr` über die Kommandozeile entgegen, aber eine
//! Desktop-App hat keine sinnvolle CLI. Statt eine eigene Config-Schicht
//! einzuziehen (verworfen, siehe CLAUDE.md YAGNI-Prinzip), liest diese App
//! zwei Env-Vars: `OCTLAB_CONNECTION` (`simulation` Default, oder `tcp`) und
//! `OCTLAB_ADDR` (Pflicht bei `tcp`). Ein Settings-UI ist eine spätere,
//! eigene Einheit (Spec 0004, "außerhalb des Scopes").

use axum::response::IntoResponse;
use octlab_server::ConnectionKind;
use tauri::{WebviewUrl, WebviewWindowBuilder};

/// Trunk-Build-Ausgabe von `apps/web`, zur Compile-Zeit dieses Binaries
/// eingebettet. Pfad relativ zu diesem Crate (`CARGO_MANIFEST_DIR` =
/// `apps/desktop`) - `trunk build` muss VOR diesem Build gelaufen sein,
/// sonst schlägt schon das Kompilieren fehl (siehe Modul-Doc-Kommentar).
#[derive(rust_embed::RustEmbed)]
#[folder = "../web/dist"]
struct FrontendAssets;

/// Fallback-Handler fürs eingebettete Frontend - liefert `index.html` für
/// `/`, sonst die Datei unter dem angefragten Pfad, ohne je das
/// Dateisystem der Zielmaschine anzufassen.
async fn embedded_frontend(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match FrontendAssets::get(path) {
        Some(file) => {
            let mime = file.metadata.mimetype().to_string();
            (
                axum::http::StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, mime)],
                file.data.into_owned(),
            )
                .into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            "Datei nicht im eingebetteten Frontend gefunden.",
        )
            .into_response(),
    }
}

fn connection_from_env() -> (ConnectionKind, Option<String>) {
    let connection = match std::env::var("OCTLAB_CONNECTION").as_deref() {
        Ok("tcp") => ConnectionKind::Tcp,
        _ => ConnectionKind::Simulation,
    };
    let addr = std::env::var("OCTLAB_ADDR").ok();
    (connection, addr)
}

fn main() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .setup(|app| {
            let (connection, addr) = connection_from_env();

            // Fail-fast wie bei octlab-server selbst: synchron auf
            // build_app_without_frontend() warten, BEVOR ein Fenster
            // entsteht, das sonst gegen einen nie startenden Server liefe.
            let api_router = tauri::async_runtime::block_on(
                octlab_server::build_app_without_frontend(connection, addr),
            )
            .map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;
            let app_router = api_router.fallback(embedded_frontend);

            let listener =
                tauri::async_runtime::block_on(tokio::net::TcpListener::bind("127.0.0.1:3000"))?;
            tauri::async_runtime::spawn(async move {
                axum::serve(listener, app_router)
                    .await
                    .expect("axum::serve beendet");
            });

            WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External("http://localhost:3000".parse().unwrap()),
            )
            .title("octlab-desktop")
            .inner_size(900.0, 600.0)
            .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri-App konnte nicht gestartet werden");
}
