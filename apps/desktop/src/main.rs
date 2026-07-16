//! Minimales Tauri-Gerüst: startet `octlab-server` intern (als Tokio-Task
//! im selben Prozess, kein Kindprozess) und öffnet ein natives WebView-
//! Fenster auf dessen `/` - dieselbe Wegwerf-HTML-Seite wie beim
//! eigenständigen `octlab-server`-Prozess (siehe dort, `static/index.html`).
//!
//! Beweist nur das Architektur-Muster (Server-Embedding + WebView), keine
//! eigene UI. Sobald `apps/web` (Leptos) steht, wird das hier umgebaut, um
//! dessen gebündeltes Frontend zu zeigen statt der Wegwerf-Seite - siehe
//! CLAUDE.md, Abschnitt "Nächste Schritte".
//!
//! Konfiguration bewusst minimal: `octlab-server` nimmt normalerweise
//! `--connection`/`--addr` über die Kommandozeile entgegen, aber eine
//! Desktop-App hat keine sinnvolle CLI. Statt eine eigene Config-Schicht
//! einzuziehen (verworfen, siehe CLAUDE.md YAGNI-Prinzip - das fliegt
//! ohnehin raus, sobald `apps/web` kommt), liest dieses Provisorium zwei
//! Env-Vars: `OCTLAB_CONNECTION` (`simulation` Default, oder `tcp`) und
//! `OCTLAB_ADDR` (Pflicht bei `tcp`).

use octlab_server::ConnectionKind;
use tauri::{WebviewUrl, WebviewWindowBuilder};

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
            // build_app() warten, BEVOR ein Fenster entsteht, das sonst
            // gegen einen nie startenden Server liefe.
            let app_router =
                tauri::async_runtime::block_on(octlab_server::build_app(connection, addr))
                    .map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;

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
            .title("octlab-desktop (Provisorium)")
            .inner_size(900.0, 600.0)
            .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri-App konnte nicht gestartet werden");
}
