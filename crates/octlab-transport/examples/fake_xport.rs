//! Protokoll-echter XPort-Simulator für Server-/Frontend-Sessions ohne
//! eingeschaltete Anlage - KEIN Teil der automatisierten Test-Suite
//! (wird aber von `cargo clippy --all-targets` mitkompiliert und verrostet
//! deshalb nicht; das Modulverhalten selbst ist über die `SimBus`-Tests in
//! `src/sim_bus.rs` abgedeckt). Aufruf:
//!
//! ```bash
//! cargo run --example fake_xport -p octlab-transport            # 127.0.0.1:15001
//! cargo run --example fake_xport -p octlab-transport -- 0.0.0.0:15001
//! cargo run --example fake_xport -p octlab-transport -- --mute 2  # DCG antwortet nicht
//! # dagegen dann:
//! cargo run -p octlab-server -- --connection tcp --addr 127.0.0.1:15001
//! ```
//!
//! Verhalten dem echten XPort nachempfunden (siehe "Verifizierte
//! Hardware-Fakten" in CLAUDE.md): rohes TCP, CR/LF-Zeilenenden, EINE
//! Session zu jeder Zeit (sequenzieller accept - ein zweiter Client wartet
//! hier allerdings im Backlog, statt wie der echte XPort abgewiesen zu
//! werden), adressierte Kommandos ohne Echo.
//!
//! Die Module dahinter liefert [`SimBus`] (Spec 0005): Er beantwortet die
//! Kanäle der Kanalliste aus `specs/0005-mehrkanal-ansicht.md` im
//! echten Draht-Format (`2:10?` → `#2:10=5.002`). Setz-Kommandos
//! (`4:0=2500!`) quittiert er mit `#4:255=0 [OK]` und übernimmt den Wert
//! fürs Rücklesen - für JEDEN bekannten Kanal, ohne die Klemmung oder
//! Rundung der echten Firmware. Unbekannte Kanäle und alles andere bleiben
//! unbeantwortet, wie bei einem Modul, das nicht antwortet (die per ESDM
//! modellierte Discovery-Semantik, kein Fehlerfall). `--mute <addr>`
//! (mehrfach möglich) schaltet ein ganzes Modul stumm, z.B. um "veraltet"
//! in der Übersicht vorzuführen.
//!
//! Die Werte sind FREI ERFUNDEN (plausible Größenordnung laut c't-Lab-Doku
//! von 2010), keine Messwerte. DIV 1:0 wandert als 20-Sekunden-Sinus über
//! 0..0.01 (untere Gauge-Hälfte, die Skala reicht bis 0.02) plus leichtem
//! Rauschen, damit der Zeiger sichtbar wandert UND zittert wie am echten
//! Gerät; alle anderen Werte bleiben stehen, bis jemand sie setzt.

use octlab_transport::SimBus;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// Kanalliste aus Spec 0005 mit erfundenen Startwerten:
/// (Adresse, Subkanal, Wert als Draht-Text).
const CHANNELS: &[(u8, u8, &str)] = &[
    (0, 10, "0.0021"),   // ADA AD16 Kanal 0
    (0, 11, "1.2500"),   // ADA AD16 Kanal 1
    (0, 12, "-3.3000"),  // ADA AD16 Kanal 2
    (0, 13, "9.9990"),   // ADA AD16 Kanal 3
    (1, 0, "0.0050000"), // DIV Messwert (wird vom Sinus überschrieben)
    (1, 19, "0"),        // DIV Messbereich (Code)
    (2, 10, "5.002"),    // DCG U Ist
    (2, 11, "0.0193"),   // DCG I Ist
    (2, 0, "5.0"),       // DCG U Soll
    (2, 1, "0.02"),      // DCG I Soll
    (4, 0, "1000.0"),    // DDS Frequenz
    (4, 1, "775"),       // DDS Pegel
];

/// Momentaner "Messwert": Sinus über die untere Gauge-Hälfte, plus
/// Pseudo-Rauschen aus dem Nanosekunden-Anteil der Uhr - gut genug für
/// sichtbares Zeiger-Zittern, ohne eine `rand`-Dependency einzuschleppen.
fn div_value(since_start: Duration) -> f64 {
    let t = since_start.as_secs_f64();
    let sine = 0.005 + 0.005 * (t * std::f64::consts::TAU / 20.0).sin();
    let noise = f64::from(since_start.subsec_nanos() % 1000) / 1000.0 * 0.0004 - 0.0002;
    (sine + noise).clamp(0.0, 0.01)
}

/// Kommandozeile: optional eine Lausch-Adresse (Default `127.0.0.1:15001`)
/// und beliebig viele `--mute <addr>`. Bewusst ohne `clap` - das ist ein
/// Beispiel in `octlab-transport`, keine neue Dependency wert.
fn parse_args() -> (String, Vec<u8>) {
    let mut listen = "127.0.0.1:15001".to_string();
    let mut muted = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--mute" {
            let addr = args
                .next()
                .and_then(|a| a.parse().ok())
                .expect("--mute braucht eine Moduladresse 0..7");
            muted.push(addr);
        } else {
            listen = arg;
        }
    }
    (listen, muted)
}

/// Frischer Bus pro Session: jede neue Verbindung beginnt mit den
/// Startwerten und ohne Antworten, die noch für die vorige Session
/// eingeplant waren.
fn new_bus(muted: &[u8]) -> SimBus {
    let bus = SimBus::new();
    for &(address, subchannel, value) in CHANNELS {
        bus.set_value(address, subchannel, value);
    }
    for &address in muted {
        bus.mute(address);
    }
    bus
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    let (addr, muted) = parse_args();
    let listener = TcpListener::bind(&addr).await?;
    println!("Fake-XPort lauscht auf {addr} (stumm: {muted:?})");
    println!("octlab-server dagegen: cargo run -p octlab-server -- --connection tcp --addr {addr}");
    let start = Instant::now();

    loop {
        let (socket, peer) = listener.accept().await?;
        println!("Session von {peer}");
        let bus = new_bus(&muted);

        // DIV-Sinus: ein eigener Task schreibt alle 100 ms einen neuen Wert
        // in den Bus. `bus.clone()` ist ein zweites Handle auf DENSELBEN
        // Bus (geteiltes `Arc` innen), der Task bekommt es per `move`.
        // Erster Wert sofort, nicht erst beim ersten Tick - sonst sieht
        // eine Abfrage direkt nach dem Connect noch den statischen Startwert.
        bus.set_value(1, 0, format!("{:.7}", div_value(start.elapsed())));
        let ticker_bus = bus.clone();
        let ticker = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                interval.tick().await;
                ticker_bus.set_value(1, 0, format!("{:.7}", div_value(start.elapsed())));
            }
        });

        let (read_half, mut write_half) = socket.into_split();
        let mut lines = BufReader::new(read_half).lines();
        loop {
            // Beide Zweige sind abbruchsicher: `next_line()` laut
            // tokio-Doku, `next_incoming()` per Konstruktion (siehe dort).
            tokio::select! {
                read = lines.next_line() => match read {
                    // `lines()` trennt an \n - das \r vom CR/LF-Zeilenende
                    // des Clients hängt noch dran und geht im trim() mit weg.
                    Ok(Some(line)) => bus.on_line_sent(line.trim()),
                    _ => break, // Client weg - Session beenden, nicht den Simulator
                },
                reply = bus.next_incoming() => {
                    if write_half.write_all(format!("{reply}\r\n").as_bytes()).await.is_err() {
                        break;
                    }
                }
            }
        }
        ticker.abort();
        println!("Session beendet");
    }
}
