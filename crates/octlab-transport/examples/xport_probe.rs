//! Manuelles Diagnose-Tool gegen den echten XPort - KEIN Teil der
//! automatisierten Test-Suite (Hardware ist im CI/Dev-Container nicht
//! verfügbar, `cargo test` rührt diese Datei nicht an). Aufruf:
//!
//! ```bash
//! cargo run --example xport_probe -p octlab-transport
//! ```
//!
//! Sendet `*:IDN?` (Broadcast, sollte alle Module + ggf. ein Echo liefern)
//! und danach `1:VAL 0?` (DIV-Messwert) an die per CLAUDE.md verifizierte
//! Hardware-Adresse `192.168.1.104:10001` und druckt jede über recv_line()
//! empfangene Zeile roh aus. Ruft jede Zeile einzeln ab, mit kurzem Timeout
//! zwischen den Zeilen statt einer festen Anzahl - das zeigt, was
//! tatsächlich ankommt (inklusive eines eventuellen Echos), statt es
//! wegzufiltern. Nützlich, um `TcpConnection` schnell manuell gegen die
//! reale Anlage zu prüfen, analog zu `curl localhost:3000/health` für
//! `octlab-server`.

use octlab_transport::{BoardConnection, TcpConnection};
use std::time::Duration;

async fn drain_lines(conn: &mut TcpConnection) {
    let mut i = 0;
    loop {
        match tokio::time::timeout(Duration::from_millis(500), conn.recv_line()).await {
            Ok(Ok(line)) => {
                println!("  [{i}] {line:?}");
                i += 1;
            }
            Ok(Err(err)) => {
                println!("  Fehler: {err}");
                break;
            }
            Err(_) => break, // 500ms ohne weitere Zeile -> fertig
        }
    }
}

#[tokio::main]
async fn main() {
    let mut conn = TcpConnection::new("192.168.1.104:10001");
    conn.connect().await.expect("connect fehlgeschlagen");

    println!("=== *:IDN? ===");
    conn.send_line("*:IDN?").await.expect("send fehlgeschlagen");
    drain_lines(&mut conn).await;

    println!("=== 1:VAL 0? ===");
    conn.send_line("1:VAL 0?")
        .await
        .expect("send fehlgeschlagen");
    drain_lines(&mut conn).await;
}
