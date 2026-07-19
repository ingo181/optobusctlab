//! Manuelle Hardware-Verifikation der DDS-Subkanal-Zuordnung (Adresse 4,
//! FW 3.71) - KEIN Teil der automatisierten Test-Suite, analog zu
//! `xport_probe`. Aufruf:
//!
//! ```bash
//! cargo run --example dds_probe -p octlab-transport
//! ```
//!
//! Hintergrund: `octlab-devices::Dds` nimmt FREQUENCY=Subkanal 0 aus dem
//! c't-Artikel von 2007 an, ohne dass das je gegen die reale Firmware
//! verifiziert wurde. Diese Probe liest die Frequenz (`4:0?`), setzt einen
//! Testwert (`4:0=1234.5!`), liest zurück, fragt den Status-Subkanal 255 ab
//! und stellt am Ende den ursprünglichen Wert wieder her. Jede empfangene
//! Zeile wird roh ausgedruckt.

use octlab_transport::{BoardConnection, TcpConnection};
use std::time::Duration;

async fn exchange(conn: &mut TcpConnection, cmd: &str) -> Vec<String> {
    println!("=== {cmd} ===");
    conn.send_line(cmd).await.expect("send fehlgeschlagen");
    let mut lines = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_millis(500), conn.recv_line()).await {
            Ok(Ok(line)) => {
                println!("  [{}] {line:?}", lines.len());
                lines.push(line);
            }
            Ok(Err(err)) => {
                println!("  Fehler: {err}");
                break;
            }
            Err(_) => break, // 500ms ohne weitere Zeile -> fertig
        }
    }
    lines
}

/// Zieht aus einer Antwortzeile wie `#4:0=1000.00 [OK]` den Wert hinter `=`.
fn parse_value(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|l| {
        let rest = l.strip_prefix("#4:0=")?;
        Some(rest.split_whitespace().next().unwrap_or(rest).to_string())
    })
}

#[tokio::main]
async fn main() {
    let mut conn = TcpConnection::new("192.168.1.104:10001");
    conn.connect().await.expect("connect fehlgeschlagen");

    exchange(&mut conn, "4:IDN?").await;

    let before = exchange(&mut conn, "4:0?").await;
    let original = parse_value(&before);
    println!("--> gelesener Originalwert: {original:?}");

    exchange(&mut conn, "4:0=1234.5!").await;
    exchange(&mut conn, "4:0?").await;
    exchange(&mut conn, "4:255?").await;

    match original {
        Some(v) => {
            exchange(&mut conn, &format!("4:0={v}!")).await;
            exchange(&mut conn, "4:0?").await;
        }
        None => println!("--> Originalwert nicht lesbar, KEIN Restore gesendet"),
    }
}
