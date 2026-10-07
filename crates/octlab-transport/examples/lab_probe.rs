//! Labor-Werkzeug für die Messpunkte der Specs 0005 und 0008 - **rein
//! lesend**, KEIN Teil des Servers. Aufruf (Server vorher stoppen: der
//! XPort erlaubt nur eine Session):
//!
//! ```bash
//! cargo run --example lab_probe -p octlab-transport -- query 1:0 2:10 4:1
//! cargo run --example lab_probe -p octlab-transport -- timing 1:0 20
//! cargo run --example lab_probe -p octlab-transport -- reconnect 10
//! cargo run --example lab_probe -p octlab-transport -- --addr 127.0.0.1:15001 query 1:0
//! ```
//!
//! - `query <addr:sub>...`: je Kanal eine Abfrage `<addr>:<sub>?`, Rohantwort
//!   und Zeit vom Senden bis zur Antwort.
//! - `timing <addr:sub> <n>`: n Abfragen desselben Kanals, jede einzeln plus
//!   Minimum/Median/Maximum in ms.
//! - `reconnect <n>`: n-mal Session schließen und im 100-ms-Takt neu
//!   verbinden, bis der XPort wieder annimmt; Wartezeit je Runde plus
//!   Minimum/Median/Maximum (Spec 0008, Messpunkt 1). **Auflösung 100 ms**:
//!   Der erste Versuch kommt 100 ms nach dem Schließen - ein sofort
//!   annehmender Partner (z.B. `fake_xport`) ergibt also gut 100 ms.
//!
//! **Nur Abfragen:** Kanäle werden ausschließlich als `<addr>:<sub>`
//! angegeben, das Werkzeug baut daraus selbst `<addr>:<sub>?`. Alles, was
//! nach Setzen aussieht (`=`, `!`), Broadcasts (`*`) oder Mnemonics werden
//! abgelehnt, bevor irgendetwas gesendet wird - schreibend ist im Labor nur
//! die verifizierte DDS-Frequenz erlaubt, und die setzt man über das
//! Frontend bzw. `dds_probe`.
//!
//! **Eine Session pro Lauf** (`query`, `timing`); `reconnect` öffnet und
//! schließt Sessions nacheinander, nie zwei gleichzeitig. Am Ende wird die
//! Session immer geschlossen.
//!
//! Ausgabe: tabulatorgetrennt, direkt ins Messprotokoll übernehmbar -
//! Zeitstempel (UTC, Zeitpunkt des Sendens bzw. Schließens, nicht der
//! Ausgabe), Befehl, Rohantwort, Millisekunden.

use octlab_transport::{BoardConnection, RawLine, TcpConnection};
use std::time::{Duration, SystemTime};
use tokio::time::Instant;

/// Default-Adresse des XPort (CLAUDE.md, "Verifizierte Hardware-Fakten").
const DEFAULT_ADDR: &str = "192.168.1.104:10001";
/// Wie lange auf eine Antwort gewartet wird (großzügiger als das 500-ms-
/// Timeout des Servers - hier wird gemessen, nicht gepollt).
const ANSWER_TIMEOUT: Duration = Duration::from_millis(1000);
/// Takt der Verbindungsversuche bei `reconnect`.
const RECONNECT_STEP: Duration = Duration::from_millis(100);
/// Obergrenze je `reconnect`-Runde.
const RECONNECT_LIMIT: Duration = Duration::from_secs(30);

/// Ein abzufragender Kanal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Target {
    address: u8,
    subchannel: u8,
}

impl Target {
    fn query_line(self) -> String {
        format!("{}:{}?", self.address, self.subchannel)
    }

    fn answer_prefix(self) -> String {
        format!("#{}:{}=", self.address, self.subchannel)
    }
}

#[derive(Debug, PartialEq)]
enum ProbeError {
    /// Eingabe ist kein reiner Kanal `<addr>:<sub>` (z.B. ein Setz-Befehl).
    Rejected(String),
}

/// Akzeptiert NUR `<addr>:<sub>` mit Adresse 0..7 und Subkanal 0..255.
fn parse_target(text: &str) -> Result<Target, ProbeError> {
    let reject = || ProbeError::Rejected(text.to_string());
    let (address, subchannel) = text.split_once(':').ok_or_else(reject)?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(address) || !digits(subchannel) {
        return Err(reject());
    }
    let address: u8 = address.parse().map_err(|_| reject())?;
    let subchannel: u8 = subchannel.parse().map_err(|_| reject())?;
    if address > 7 {
        return Err(reject());
    }
    Ok(Target {
        address,
        subchannel,
    })
}

/// Ergebnis einer Abfrage.
#[derive(Debug, Clone, PartialEq)]
struct QueryResult {
    /// Zeitpunkt des Sendens (für das Messprotokoll), siehe [`Clock`].
    at: SystemTime,
    command: String,
    /// Die zum Kanal passende Antwortzeile, falls sie kam.
    answer: Option<RawLine>,
    /// Andere Zeilen, die währenddessen kamen (Echo, unaufgefordert).
    other: Vec<RawLine>,
    elapsed: Duration,
}

/// Wanduhr für die Zeitstempel: Startzeit plus verstrichene Tokio-Zeit.
/// So stimmt der Zeitstempel je Messung (nicht erst bei der Ausgabe) und
/// ist mit pausierter Uhr testbar.
#[derive(Clone, Copy)]
struct Clock {
    wall: SystemTime,
    start: Instant,
}

impl Clock {
    fn new(wall: SystemTime) -> Self {
        Self {
            wall,
            start: Instant::now(),
        }
    }

    fn now(&self) -> SystemTime {
        self.wall + self.start.elapsed()
    }
}

/// Fragt die Kanäle nacheinander über EINE Verbindung ab.
async fn run_query<C: BoardConnection + ?Sized>(
    conn: &mut C,
    targets: &[&str],
    clock: Clock,
) -> Result<Vec<QueryResult>, ProbeError> {
    // Erst ALLE prüfen, dann senden: ein abgelehnter Eintrag verhindert
    // den ganzen Lauf, nicht nur sich selbst.
    let targets: Vec<Target> = targets
        .iter()
        .map(|t| parse_target(t))
        .collect::<Result<_, _>>()?;

    let mut results = Vec::new();
    for target in targets {
        let command = target.query_line();
        let prefix = target.answer_prefix();
        let start = Instant::now();
        let at = clock.now();
        let mut answer = None;
        let mut other = Vec::new();
        if let Err(err) = conn.send_line(&command).await {
            other.push(format!("Sendefehler: {err}"));
        } else {
            let deadline = start + ANSWER_TIMEOUT;
            loop {
                tokio::select! {
                    line = conn.recv_line() => match line {
                        Ok(line) if line.starts_with(&prefix) => {
                            answer = Some(line);
                            break;
                        }
                        Ok(line) => other.push(line),
                        Err(err) => {
                            other.push(format!("Lesefehler: {err}"));
                            break;
                        }
                    },
                    () = tokio::time::sleep_until(deadline) => break,
                }
            }
        }
        results.push(QueryResult {
            at,
            command,
            answer,
            other,
            elapsed: start.elapsed(),
        });
    }
    Ok(results)
}

/// Minimum, Median, Maximum (ms) und Zahl der Abfragen ohne Antwort.
#[derive(Debug, Clone, PartialEq)]
struct Stats {
    min_ms: f64,
    median_ms: f64,
    max_ms: f64,
    missing: usize,
}

fn stats(samples_ms: &[f64], missing: usize) -> Option<Stats> {
    if samples_ms.is_empty() {
        return None;
    }
    let mut sorted = samples_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let median_ms = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    Some(Stats {
        min_ms: sorted[0],
        median_ms,
        max_ms: sorted[n - 1],
        missing,
    })
}

/// Misst die Wartezeit nach dem Schließen bis zur nächsten angenommenen
/// Session, `rounds`-mal. `None` = innerhalb von [`RECONNECT_LIMIT`] keine.
async fn run_reconnect<C: BoardConnection + ?Sized>(
    conn: &mut C,
    rounds: usize,
    clock: Clock,
) -> Vec<(SystemTime, Option<Duration>)> {
    let mut results = Vec::new();
    for _ in 0..rounds {
        // Session schließen - erst danach wird neu verbunden, nie zwei
        // Sessions gleichzeitig.
        let _ = conn.disconnect().await;
        let closed = Instant::now();
        let closed_at = clock.now();
        let waited = loop {
            tokio::time::sleep(RECONNECT_STEP).await;
            if closed.elapsed() > RECONNECT_LIMIT {
                break None;
            }
            if conn.connect().await.is_ok() {
                break Some(closed.elapsed());
            }
        };
        results.push((closed_at, waited));
        if waited.is_none() {
            break; // XPort nimmt gar nicht mehr an - weitere Runden sinnlos
        }
    }
    results
}

/// UTC-Zeitstempel `YYYY-MM-DDThh:mm:ss.mmmZ` ohne Zusatz-Crate
/// (Kalenderrechnung nach Howard Hinnant, "days_from_civil" rückwärts).
fn format_utc(time: SystemTime) -> String {
    let since = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = since.as_secs();
    let millis = since.subsec_millis();
    let (hh, mm, ss) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    // Tage seit 1970-01-01 -> (Jahr, Monat, Tag)
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}.{millis:03}Z")
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn print_row(at: SystemTime, command: &str, answer: &str, elapsed: Option<Duration>) {
    let ms_text = elapsed.map_or("-".to_string(), |d| format!("{:.1}", ms(d)));
    println!("{}\t{command}\t{answer}\t{ms_text}", format_utc(at));
}

fn print_stats(label: &str, stats: Option<Stats>) {
    match stats {
        Some(s) => println!(
            "{}\t{label}\tmin {:.1} / median {:.1} / max {:.1} ms\tohne Antwort: {}",
            format_utc(SystemTime::now()),
            s.min_ms,
            s.median_ms,
            s.max_ms,
            s.missing
        ),
        None => println!(
            "{}\t{label}\tkeine Messwerte",
            format_utc(SystemTime::now())
        ),
    }
}

fn usage() -> ! {
    eprintln!(
        "Aufruf: lab_probe [--addr HOST:PORT] query <addr:sub>... | timing <addr:sub> <n> | reconnect <n>"
    );
    std::process::exit(2)
}

#[cfg_attr(test, allow(dead_code))]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut addr = DEFAULT_ADDR.to_string();
    if args.first().map(String::as_str) == Some("--addr") {
        if args.len() < 2 {
            usage();
        }
        addr = args.remove(1);
        args.remove(0);
    }
    let Some(command) = args.first().cloned() else {
        usage();
    };
    let rest: Vec<&str> = args[1..].iter().map(String::as_str).collect();

    // Erst alle Eingaben prüfen, dann verbinden: Ein abgelehnter Befehl
    // öffnet gar keine Session.
    match command.as_str() {
        "query" if !rest.is_empty() => {}
        "timing" if rest.len() == 2 => {}
        "reconnect" if rest.len() == 1 => {}
        _ => usage(),
    }
    if command != "reconnect" {
        let targets = if command == "timing" {
            &rest[..1]
        } else {
            &rest[..]
        };
        for t in targets {
            if let Err(ProbeError::Rejected(text)) = parse_target(t) {
                eprintln!("abgelehnt (nur Abfragen <addr>:<sub> erlaubt): {text}");
                std::process::exit(2);
            }
        }
    }

    let mut conn = TcpConnection::new(addr.clone());
    if let Err(err) = conn.connect().await {
        eprintln!("Verbindung zu {addr} fehlgeschlagen: {err} (läuft noch ein Server oder eine andere Session?)");
        std::process::exit(1);
    }
    println!("# lab_probe {command} gegen {addr} (Zeitstempel UTC; Spalten: Zeit, Befehl, Rohantwort, ms)");
    let clock = Clock::new(SystemTime::now());

    match command.as_str() {
        "query" => {
            let results = run_query(&mut conn, &rest, clock)
                .await
                .expect("vorher geprüft");
            for r in results {
                for other in &r.other {
                    print_row(r.at, &r.command, &format!("(andere Zeile) {other}"), None);
                }
                let answer = r.answer.as_deref().unwrap_or("(keine Antwort)");
                print_row(
                    r.at,
                    &r.command,
                    answer,
                    r.answer.as_ref().map(|_| r.elapsed),
                );
            }
        }
        "timing" => {
            let n: usize = rest[1].parse().unwrap_or_else(|_| usage());
            let targets = vec![rest[0]; n];
            let results = run_query(&mut conn, &targets, clock)
                .await
                .expect("vorher geprüft");
            let mut samples = Vec::new();
            let mut missing = 0;
            for r in &results {
                let answer = r.answer.as_deref().unwrap_or("(keine Antwort)");
                print_row(
                    r.at,
                    &r.command,
                    answer,
                    r.answer.as_ref().map(|_| r.elapsed),
                );
                match r.answer {
                    Some(_) => samples.push(ms(r.elapsed)),
                    None => missing += 1,
                }
            }
            print_stats(
                &format!("timing {} x{n}", rest[0]),
                stats(&samples, missing),
            );
        }
        "reconnect" => {
            let n: usize = rest[0].parse().unwrap_or_else(|_| usage());
            let rounds = run_reconnect(&mut conn, n, clock).await;
            let mut samples = Vec::new();
            let mut missing = 0;
            for (i, (at, round)) in rounds.iter().enumerate() {
                let text = match round {
                    Some(_) => "neue Session angenommen".to_string(),
                    None => format!("keine Session innerhalb {} s", RECONNECT_LIMIT.as_secs()),
                };
                print_row(*at, &format!("reconnect Runde {}", i + 1), &text, *round);
                match round {
                    Some(d) => samples.push(ms(*d)),
                    None => missing += 1,
                }
            }
            print_stats(&format!("reconnect x{n}"), stats(&samples, missing));
        }
        _ => usage(),
    }
    let _ = conn.disconnect().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use octlab_transport::{SimBus, SimulatedConnection};

    /// Wanduhr ab der Unix-Epoche, damit Zeitstempel im Test exakt sind.
    fn test_clock() -> Clock {
        Clock::new(SystemTime::UNIX_EPOCH)
    }

    fn sim() -> (SimulatedConnection, SimBus) {
        let bus = SimBus::new();
        bus.set_value(1, 0, "0.0022");
        bus.set_value(4, 0, "1000.0");
        (
            SimulatedConnection::new("lab-probe-test").with_bus(bus.clone()),
            bus,
        )
    }

    #[test]
    fn kanal_wird_als_addr_sub_gelesen() {
        assert_eq!(
            parse_target("4:0"),
            Ok(Target {
                address: 4,
                subchannel: 0
            })
        );
        assert_eq!(
            parse_target("1:254").map(Target::query_line),
            Ok("1:254?".to_string())
        );
    }

    /// Alles außer einer reinen Abfrage wird abgelehnt - insbesondere
    /// Setz-Befehle, egal in welcher Schreibweise.
    #[test]
    fn schreibbefehle_und_sonstiges_werden_abgelehnt() {
        for text in [
            "4:0=2500!",
            "4:0=2500",
            "4:0!",
            "*:IDN?",
            "1:0?",
            "1:VAL 0",
            "2:DCV=5",
            "8:0",
            "1:256",
            "",
            "1",
            "a:b",
        ] {
            assert!(
                matches!(parse_target(text), Err(ProbeError::Rejected(_))),
                "nicht abgelehnt: {text:?}"
            );
        }
    }

    /// Ein abgelehnter Befehl erreicht den Bus NICHT - auch nicht teilweise
    /// (vorher geprüfte Kanäle werden in diesem Lauf ebenfalls nicht
    /// gesendet).
    #[tokio::test(start_paused = true)]
    async fn schreibbefehl_wird_nicht_gesendet() {
        let (mut conn, bus) = sim();
        conn.connect().await.unwrap();

        let result = run_query(&mut conn, &["1:0", "4:0=2500!"], test_clock()).await;

        assert!(
            matches!(result, Err(ProbeError::Rejected(_))),
            "kam: {result:?}"
        );
        assert!(bus.sent().is_empty(), "gesendet wurde: {:?}", bus.sent());
    }

    #[tokio::test(start_paused = true)]
    async fn abfrage_liefert_rohantwort_und_zeit() {
        let (mut conn, bus) = sim();
        bus.set_reply_delay(Duration::from_millis(7));
        conn.connect().await.unwrap();

        let results = run_query(&mut conn, &["1:0", "4:0"], test_clock())
            .await
            .unwrap();

        assert_eq!(
            results,
            vec![
                QueryResult {
                    at: SystemTime::UNIX_EPOCH,
                    command: "1:0?".into(),
                    answer: Some("#1:0=0.0022".into()),
                    other: vec![],
                    elapsed: Duration::from_millis(7),
                },
                QueryResult {
                    // Zeitstempel je Abfrage (Senden), nicht je Lauf.
                    at: SystemTime::UNIX_EPOCH + Duration::from_millis(7),
                    command: "4:0?".into(),
                    answer: Some("#4:0=1000.0".into()),
                    other: vec![],
                    elapsed: Duration::from_millis(7),
                },
            ]
        );
        assert_eq!(
            bus.sent().into_iter().map(|(_, l)| l).collect::<Vec<_>>(),
            vec!["1:0?", "4:0?"]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn ohne_antwort_nach_einer_sekunde_weiter() {
        let (mut conn, bus) = sim();
        bus.mute(2);
        conn.connect().await.unwrap();

        let results = run_query(&mut conn, &["2:10"], test_clock()).await.unwrap();

        assert_eq!(results[0].answer, None);
        assert_eq!(results[0].elapsed, ANSWER_TIMEOUT);
    }

    /// Andere Zeilen (Echo, Panel-Bedienung) werden festgehalten, beenden
    /// das Warten aber nicht.
    #[tokio::test(start_paused = true)]
    async fn andere_zeilen_werden_festgehalten() {
        let (mut conn, bus) = sim();
        bus.set_reply_delay(Duration::from_millis(5));
        conn.connect().await.unwrap();
        bus.inject("#2:255=65");

        let results = run_query(&mut conn, &["1:0"], test_clock()).await.unwrap();

        assert_eq!(results[0].answer.as_deref(), Some("#1:0=0.0022"));
        assert_eq!(results[0].other, vec!["#2:255=65".to_string()]);
    }

    #[test]
    fn statistik_min_median_max() {
        assert_eq!(
            stats(&[3.0, 1.0, 2.0], 1),
            Some(Stats {
                min_ms: 1.0,
                median_ms: 2.0,
                max_ms: 3.0,
                missing: 1
            })
        );
        assert_eq!(
            stats(&[4.0, 1.0, 3.0, 2.0], 0).map(|s| s.median_ms),
            Some(2.5)
        );
        assert_eq!(stats(&[], 3), None);
    }

    /// XPort-Verhalten im Simulator nachgebildet: nach dem Schließen 350 ms
    /// lang Ablehnung. Versuche im 100-ms-Takt -> angenommen bei 400 ms.
    #[tokio::test(start_paused = true)]
    async fn wiederverbindung_misst_die_wartezeit_je_runde() {
        let (mut conn, bus) = sim();
        bus.refuse_after_disconnect(Duration::from_millis(350));
        conn.connect().await.unwrap();

        let rounds = run_reconnect(&mut conn, 3, test_clock()).await;

        let waits: Vec<Option<Duration>> = rounds.iter().map(|(_, w)| *w).collect();
        assert_eq!(waits, vec![Some(Duration::from_millis(400)); 3]);
        // Zeitstempel je Runde = Zeitpunkt des Schließens: 0, 400, 800 ms.
        let times: Vec<SystemTime> = rounds.iter().map(|(at, _)| *at).collect();
        assert_eq!(
            times,
            vec![
                SystemTime::UNIX_EPOCH,
                SystemTime::UNIX_EPOCH + Duration::from_millis(400),
                SystemTime::UNIX_EPOCH + Duration::from_millis(800),
            ]
        );
        // Nie zwei Sessions gleichzeitig: vor jedem Connect ist die vorige
        // Session geschlossen (Disconnect), angenommen wird nur einmal je Runde.
        let accepted = bus
            .connection_log()
            .into_iter()
            .filter(|(_, e)| *e == octlab_transport::ConnectionEvent::Connect { accepted: true })
            .count();
        assert_eq!(accepted, 1 + 3);
    }

    #[test]
    fn utc_zeitstempel() {
        assert_eq!(
            format_utc(SystemTime::UNIX_EPOCH),
            "1970-01-01T00:00:00.000Z"
        );
        // 2026-10-07T21:14:16.436Z
        let t = SystemTime::UNIX_EPOCH + Duration::from_millis(1_791_407_656_436);
        assert_eq!(format_utc(t), "2026-10-07T21:14:16.436Z");
    }
}
