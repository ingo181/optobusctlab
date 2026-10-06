//! Simulierter OptoBus mit "lebenden" Modulen (Spec 0005, Schritt 3a).
//!
//! Ergänzt die rein skriptgesteuerte [`SimulatedConnection`](crate::SimulatedConnection)
//! (FIFO-Antworten, siehe `push_response`/`push_reply`) um Module, die auf
//! das echte Draht-Format reagieren - nötig, sobald ein Poll-Zyklus viele
//! Kanäle in nicht vorab festgelegter Reihenfolge abfragt und Tests Ausfall,
//! Wiederkehr und Antwortzeiten steuern wollen:
//!
//! - `<addr>:<sub>?` → `#<addr>:<sub>=<wert>`, falls der Kanal bekannt ist
//!   (sonst keine Antwort - wie ein Modul ohne diesen Subkanal).
//! - `<addr>:<sub>=<wert>!` → Quittung `#<addr>:255=0 [OK]` und der Wert
//!   wird übernommen (Muster Setzen → Quittung → Rücklesen aus Spec 0003).
//!   Der Wert wird als Text so übernommen, wie er gesendet wurde - die
//!   Formatierung der echten Firmware (z.B. eine Nachkommastelle bei DDS)
//!   bildet die Simulation bewusst nicht nach.
//! - Stummgeschaltete Adressen antworten auf nichts (Timeout-Szenarien).
//! - Optionale Antwortverzögerung (Zyklusüberlauf-Szenarien).
//!
//! Wird außerdem vom Beispiel `fake_xport` verwendet ([`Self::on_line_sent`] +
//! [`Self::next_incoming`] direkt, ohne `SimulatedConnection`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::Instant;

/// Handle auf einen simulierten OptoBus.
///
/// `Clone` liefert ein zweites Handle auf DENSELBEN Bus (geteilter Zustand
/// per `Arc`): Die Verbindung wandert beim `Lab::spawn()` komplett in den
/// Actor (Ownership-Übergabe), der Test steuert die Module danach über
/// sein eigenes Handle weiter. `Mutex` schützt den Zustand gegen
/// gleichzeitige Zugriffe von Test und Actor; er wird nie über ein
/// `.await` hinweg gehalten (sonst könnte ein `std::sync::Mutex` den
/// Runtime-Thread blockieren).
#[derive(Clone, Default)]
pub struct SimBus {
    state: Arc<Mutex<BusState>>,
    /// Weckt ein in [`Self::next_incoming`] wartendes `recv_line()`, wenn
    /// neue Zeilen eingeplant wurden.
    wake: Arc<Notify>,
}

#[derive(Default)]
struct BusState {
    /// Aktueller Wert je (Adresse, Subkanal), als Text wie auf dem Draht.
    values: HashMap<(u8, u8), String>,
    muted: HashSet<u8>,
    reply_delay: Duration,
    /// Eingeplante eingehende Zeilen, aufsteigend nach Fälligkeit sortiert.
    incoming: VecDeque<(Instant, String)>,
    sent: Vec<(Instant, String)>,
}

impl BusState {
    /// Sortiert ein, hinter allen Einträgen mit gleicher Fälligkeit
    /// (Reihenfolge gleichzeitig fälliger Zeilen bleibt erhalten).
    fn schedule(&mut self, due: Instant, line: String) {
        let pos = self.incoming.partition_point(|(at, _)| *at <= due);
        self.incoming.insert(pos, (due, line));
    }
}

impl SimBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Legt den Wert eines Kanals fest (bzw. macht den Kanal erst bekannt).
    pub fn set_value(&self, address: u8, subchannel: u8, value: impl Into<String>) {
        self.lock()
            .values
            .insert((address, subchannel), value.into());
    }

    /// Modul an `address` antwortet ab sofort auf nichts mehr.
    pub fn mute(&self, address: u8) {
        self.lock().muted.insert(address);
    }

    pub fn unmute(&self, address: u8) {
        self.lock().muted.remove(&address);
    }

    /// Verzögerung zwischen Kommando und Antwort, für alle Module.
    pub fn set_reply_delay(&self, delay: Duration) {
        self.lock().reply_delay = delay;
    }

    /// Speist eine unaufgeforderte Zeile ein (z.B. Panel-Bedienung), sofort
    /// fällig.
    pub fn inject(&self, line: impl Into<String>) {
        self.lock().schedule(Instant::now(), line.into());
        self.wake.notify_one();
    }

    /// Alle bisher gesendeten Zeilen mit Sendezeitpunkt.
    pub fn sent(&self) -> Vec<(Instant, String)> {
        self.lock().sent.clone()
    }

    /// Verarbeitet eine vom Client gesendete Zeile: protokollieren und ggf.
    /// die Antwort(en) einplanen.
    pub fn on_line_sent(&self, line: &str) {
        let now = Instant::now();
        let mut state = self.lock();
        state.sent.push((now, line.to_string()));

        let Some((address, subchannel, request)) = parse_request(line) else {
            return;
        };
        if state.muted.contains(&address) {
            return;
        }
        let due = now + state.reply_delay;
        match request {
            Request::Query => {
                if let Some(value) = state.values.get(&(address, subchannel)).cloned() {
                    state.schedule(due, format!("#{address}:{subchannel}={value}"));
                }
            }
            Request::Set(value) => {
                state.values.insert((address, subchannel), value);
                state.schedule(due, format!("#{address}:255=0 [OK]"));
            }
        }
        drop(state);
        self.wake.notify_one();
    }

    /// Wartet auf die nächste fällige eingehende Zeile. Bleibt pending,
    /// solange nichts eingeplant ist - NIE ein sofortiger Fehler (siehe
    /// CLAUDE.md: sonst busy-loopt der Lab-Actor).
    ///
    /// Abbruchsicher (der Lab-Actor ruft das in `tokio::select!` auf): Eine
    /// Zeile wird erst nach dem letzten `.await` und ohne weiteres `.await`
    /// aus der Warteschlange genommen - wird die Future vorher verworfen,
    /// geht nichts verloren.
    pub async fn next_incoming(&self) -> String {
        loop {
            let next_due = {
                let mut state = self.lock();
                match state.incoming.front() {
                    Some((due, _)) if *due <= Instant::now() => {
                        return state.incoming.pop_front().expect("front() war Some").1;
                    }
                    Some((due, _)) => Some(*due),
                    None => None,
                }
            };
            match next_due {
                Some(due) => {
                    tokio::select! {
                        _ = tokio::time::sleep_until(due) => {}
                        _ = self.wake.notified() => {}
                    }
                }
                None => self.wake.notified().await,
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BusState> {
        self.state.lock().expect("SimBus-Mutex vergiftet")
    }
}

enum Request {
    Query,
    Set(String),
}

/// Erkennt `<addr>:<sub>?` und `<addr>:<sub>=<wert>!` (numerische
/// Subkanäle, wie `Command::to_wire()` sie erzeugt). Alles andere - z.B.
/// Mnemonics oder Broadcasts - ignoriert die Simulation.
fn parse_request(line: &str) -> Option<(u8, u8, Request)> {
    let (address, rest) = line.trim().split_once(':')?;
    let address = address.parse().ok()?;
    if let Some(subchannel) = rest.strip_suffix('?') {
        return Some((address, subchannel.parse().ok()?, Request::Query));
    }
    let (subchannel, value) = rest.strip_suffix('!')?.split_once('=')?;
    Some((
        address,
        subchannel.parse().ok()?,
        Request::Set(value.to_string()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoardConnection, SimulatedConnection};

    /// Verbindung mit angeschlossenem Bus - der Test behält ein zweites
    /// Handle (`bus`), die Verbindung wandert in der Praxis in den Actor.
    fn connection_with_bus() -> (SimulatedConnection, SimBus) {
        let bus = SimBus::new();
        let conn = SimulatedConnection::new("sim-bus-test").with_bus(bus.clone());
        (conn, bus)
    }

    /// Wartet höchstens `limit` auf die nächste Zeile; `None` = nichts kam.
    async fn recv_within(conn: &mut SimulatedConnection, limit: Duration) -> Option<String> {
        tokio::time::timeout(limit, conn.recv_line())
            .await
            .ok()
            .map(|line| line.expect("recv_line der Simulation liefert nie Err"))
    }

    #[tokio::test(start_paused = true)]
    async fn abfrage_eines_bekannten_kanals_wird_beantwortet() {
        let (mut conn, bus) = connection_with_bus();
        bus.set_value(2, 10, "12.0");

        conn.send_line("2:10?").await.unwrap();

        assert_eq!(
            recv_within(&mut conn, Duration::from_secs(1))
                .await
                .as_deref(),
            Some("#2:10=12.0")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn abfrage_eines_unbekannten_kanals_bleibt_unbeantwortet() {
        let (mut conn, bus) = connection_with_bus();
        bus.set_value(2, 10, "12.0");

        conn.send_line("2:11?").await.unwrap();

        assert_eq!(recv_within(&mut conn, Duration::from_secs(1)).await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn stummgeschaltetes_modul_antwortet_nicht_bis_es_wieder_eingeschaltet_wird() {
        let (mut conn, bus) = connection_with_bus();
        bus.set_value(2, 10, "12.0");
        bus.mute(2);

        conn.send_line("2:10?").await.unwrap();
        assert_eq!(recv_within(&mut conn, Duration::from_secs(1)).await, None);

        bus.unmute(2);
        conn.send_line("2:10?").await.unwrap();
        assert_eq!(
            recv_within(&mut conn, Duration::from_secs(1))
                .await
                .as_deref(),
            Some("#2:10=12.0")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn setz_kommando_wird_quittiert_und_ist_rueckzulesen() {
        let (mut conn, bus) = connection_with_bus();
        bus.set_value(4, 0, "1000.0");

        conn.send_line("4:0=2500!").await.unwrap();
        assert_eq!(
            recv_within(&mut conn, Duration::from_secs(1))
                .await
                .as_deref(),
            Some("#4:255=0 [OK]")
        );

        conn.send_line("4:0?").await.unwrap();
        assert_eq!(
            recv_within(&mut conn, Duration::from_secs(1))
                .await
                .as_deref(),
            Some("#4:0=2500")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn stummgeschaltetes_modul_quittiert_nicht_und_uebernimmt_keinen_wert() {
        let (mut conn, bus) = connection_with_bus();
        bus.set_value(4, 0, "1000.0");
        bus.mute(4);

        conn.send_line("4:0=2500!").await.unwrap();
        assert_eq!(recv_within(&mut conn, Duration::from_secs(1)).await, None);

        bus.unmute(4);
        conn.send_line("4:0?").await.unwrap();
        assert_eq!(
            recv_within(&mut conn, Duration::from_secs(1))
                .await
                .as_deref(),
            Some("#4:0=1000.0")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn antwort_kommt_erst_nach_der_eingestellten_verzoegerung() {
        let (mut conn, bus) = connection_with_bus();
        bus.set_value(1, 0, "0.0022");
        bus.set_reply_delay(Duration::from_millis(400));

        let start = Instant::now();
        conn.send_line("1:0?").await.unwrap();
        let reply = recv_within(&mut conn, Duration::from_secs(1)).await;

        assert_eq!(reply.as_deref(), Some("#1:0=0.0022"));
        assert_eq!(start.elapsed(), Duration::from_millis(400));
    }

    #[tokio::test(start_paused = true)]
    async fn zur_laufzeit_eingespeiste_zeile_weckt_ein_wartendes_recv() {
        let (mut conn, bus) = connection_with_bus();
        let injector = bus.clone();

        // recv_line() wartet bereits, wenn die Zeile 100 ms später eintrifft
        // - prüft, dass das Einspeisen den Wartenden wirklich aufweckt.
        let inject_later = async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            injector.inject("#2:255=65");
        };
        let (reply, ()) =
            tokio::join!(recv_within(&mut conn, Duration::from_secs(1)), inject_later);

        assert_eq!(reply.as_deref(), Some("#2:255=65"));
    }

    #[tokio::test(start_paused = true)]
    async fn sende_log_haelt_zeitpunkte_fest() {
        let (mut conn, bus) = connection_with_bus();
        let start = Instant::now();

        conn.send_line("1:0?").await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        conn.send_line("2:10?").await.unwrap();

        let sent: Vec<(Duration, String)> = bus
            .sent()
            .into_iter()
            .map(|(at, line)| (at - start, line))
            .collect();
        assert_eq!(
            sent,
            vec![
                (Duration::ZERO, "1:0?".to_string()),
                (Duration::from_secs(5), "2:10?".to_string()),
            ]
        );
    }

    /// CLAUDE.md-Design-Entscheidung: bei leerer Warteschlange bleibt
    /// `recv_line()` pending statt sofort `Err` zu liefern (sonst busy-loopt
    /// der Lab-Actor). Gilt mit angeschlossenem Bus genauso.
    #[tokio::test(start_paused = true)]
    async fn ohne_eingehende_zeilen_bleibt_recv_pending_statt_fehler() {
        let (mut conn, _bus) = connection_with_bus();

        let result = tokio::time::timeout(Duration::from_secs(5), conn.recv_line()).await;

        assert!(
            result.is_err(),
            "recv_line() muss pending bleiben, kam: {result:?}"
        );
    }

    /// Die bisherige FIFO-Skriptung (`push_response`) bleibt nutzbar und hat
    /// Vorrang vor dem Bus - bestehende Tests (Cucumber, set_channel) hängen
    /// daran.
    #[tokio::test(start_paused = true)]
    async fn fifo_antworten_bleiben_neben_dem_bus_nutzbar() {
        let (mut conn, bus) = connection_with_bus();
        bus.set_value(1, 0, "0.5");
        conn.push_response("#0:0=1.23456");

        conn.send_line("1:0?").await.unwrap();

        assert_eq!(
            recv_within(&mut conn, Duration::from_secs(1))
                .await
                .as_deref(),
            Some("#0:0=1.23456")
        );
        assert_eq!(
            recv_within(&mut conn, Duration::from_secs(1))
                .await
                .as_deref(),
            Some("#1:0=0.5")
        );
    }
}
