//! Eigenständiger Prozess-Einstiegspunkt: CLI-Parsing, dann [`octlab_server::build_app`].
//!
//! Läuft unverändert an zwei Orten:
//! - als eigenständiger Prozess auf dem Raspberry Pi (systemd-Unit),
//!   Browser im Kiosk-Modus zeigt die (später hier angebundene) Leptos-UI
//! - eingebettet in der Tauri-Desktop-App (`apps/desktop` ruft
//!   `octlab_server::build_app` direkt auf, ohne über dieses CLI zu gehen -
//!   siehe dort)
//!
//! Default ist die `SimulatedConnection`, damit `cargo run` sofort ohne
//! c't-Lab funktioniert. Echte Hardware nur explizit über
//! `--connection tcp --addr <host:port>` (siehe [`Cli`]).

use clap::Parser;
use octlab_server::{build_app, ConnectionKind};

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

    /// Verzeichnis mit der Trunk-Build-Ausgabe des Frontends. Der Default
    /// passt für `cargo run` aus dem Repo-Root, nachdem in `apps/web`
    /// einmal `trunk build` gelaufen ist.
    #[arg(long, default_value = "apps/web/dist")]
    frontend_dist: std::path::PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let app = match build_app(cli.connection, cli.addr, cli.frontend_dist).await {
        Ok(app) => app,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Port 3000 konnte nicht gebunden werden");
    tracing::info!("octlab-server läuft auf http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
