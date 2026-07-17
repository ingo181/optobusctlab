//! Protokoll-echter XPort-Simulator für Server-/Frontend-Sessions ohne
//! eingeschaltete Anlage - KEIN Teil der automatisierten Test-Suite
//! (wird aber von `cargo clippy --all-targets` mitkompiliert und verrostet
//! deshalb nicht). Aufruf:
//!
//! ```bash
//! cargo run --example fake_xport -p octlab-transport            # 127.0.0.1:15001
//! cargo run --example fake_xport -p octlab-transport -- 0.0.0.0:15001
//! # dagegen dann:
//! cargo run -p octlab-server -- --connection tcp --addr 127.0.0.1:15001
//! ```
//!
//! Verhalten dem echten XPort nachempfunden (siehe "Verifizierte
//! Hardware-Fakten" in CLAUDE.md): rohes TCP, CR/LF-Zeilenenden, EINE
//! Session zu jeder Zeit (sequenzieller accept - ein zweiter Client wartet
//! hier allerdings im Backlog, statt wie der echte XPort abgewiesen zu
//! werden), adressierte Kommandos ohne Echo. Antwortet auf `1:0?` (DIV,
//! Adresse 1, Subkanal 0 - das echte Draht-Format aus `Command::to_wire()`)
//! mit einem 20-Sekunden-Sinus über die Gauge-Skala 0..0.01 plus leichtem
//! Rauschen, damit der Zeiger sichtbar wandert UND zittert wie am echten
//! Gerät. Alle anderen Kommandos werden ignoriert, wie von einem Modul,
//! das nicht antwortet (das ist die per ESDM modellierte
//! Discovery-Semantik, kein Fehlerfall).

use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// Momentaner "Messwert": Sinus über die volle Gauge-Skala, plus
/// Pseudo-Rauschen aus dem Nanosekunden-Anteil der Uhr - gut genug für
/// sichtbares Zeiger-Zittern, ohne eine `rand`-Dependency einzuschleppen.
fn div_value(since_start: std::time::Duration) -> f64 {
    let t = since_start.as_secs_f64();
    let sine = 0.005 + 0.005 * (t * std::f64::consts::TAU / 20.0).sin();
    let noise = f64::from(since_start.subsec_nanos() % 1000) / 1000.0 * 0.0004 - 0.0002;
    (sine + noise).clamp(0.0, 0.01)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:15001".to_string());
    let listener = TcpListener::bind(&addr).await?;
    println!("Fake-XPort lauscht auf {addr}");
    println!("octlab-server dagegen: cargo run -p octlab-server -- --connection tcp --addr {addr}");
    let start = Instant::now();

    loop {
        let (socket, peer) = listener.accept().await?;
        println!("Session von {peer}");
        let (read_half, mut write_half) = socket.into_split();
        let mut lines = BufReader::new(read_half).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // `lines()` trennt an \n - das \r vom CR/LF-Zeilenende des
            // Clients hängt noch dran und geht im trim() mit weg.
            if line.trim() == "1:0?" {
                let reply = format!("#1:0={:.7}\r\n", div_value(start.elapsed()));
                if write_half.write_all(reply.as_bytes()).await.is_err() {
                    break; // Client weg - Session beenden, nicht den Simulator
                }
            }
        }
        println!("Session beendet");
    }
}
