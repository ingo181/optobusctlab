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
//! - Abbruch und Wiederverbindung (Spec 0008): [`SimBus::drop_connection`]
//!   bildet das gemessene TCP-Verhalten nach - danach liefern Lesen und
//!   Senden sofort und bei JEDEM Aufruf `Disconnected`. Ein gelungenes
//!   `connect()` startet eine frische Session (Modulwerte bleiben, für die
//!   alte Session unterwegs gewesene Antworten nicht).
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
    /// Adressen, die Setz-Kommandos ohne Quittung übernehmen.
    acks_dropped: HashSet<u8>,
    reply_delay: Duration,
    /// Eingeplante eingehende Zeilen, aufsteigend nach Fälligkeit sortiert.
    incoming: VecDeque<(Instant, String)>,
    sent: Vec<(Instant, String)>,
    /// Session getrennt (Abbruch durch die Gegenseite oder `disconnect()`
    /// des Clients). `false` = verbunden - der Bus startet verbunden, auch
    /// ohne vorheriges `connect()` (so nutzen ihn `fake_xport` und ältere
    /// Tests).
    disconnected: bool,
    /// Solange `true`, scheitert `connect()` ("Connection refused").
    refuse_connections: bool,
    /// Nach einem `disconnect()` des Clients so lange ablehnen (XPort:
    /// alte Session noch nicht abgebaut).
    refuse_after_disconnect: Duration,
    last_client_disconnect: Option<Instant>,
    connection_log: Vec<(Instant, ConnectionEvent)>,
    io_while_disconnected: Vec<(Instant, IoAttempt)>,
    /// Leseversuche seit dem letzten Verbindungswechsel (für den Wächter;
    /// das Protokoll oben bleibt dagegen vollständig).
    reads_since_change: usize,
}

/// Ab so vielen Leseversuchen auf einer getrennten Verbindung gilt das als
/// Dauerschleife (siehe [`SimBus::note_io_while_disconnected`]).
const BUSY_LOOP_READS: usize = 10_000;

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

    /// Modul an `address` übernimmt Setz-Kommandos weiterhin, schickt aber
    /// keine Quittung mehr ("verlorene Quittung", Spec 0005 AK6).
    /// Abfragen beantwortet es normal.
    pub fn drop_acks(&self, address: u8) {
        self.lock().acks_dropped.insert(address);
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
                if !state.acks_dropped.contains(&address) {
                    state.schedule(due, format!("#{address}:255=0 [OK]"));
                }
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

    // --- Spec 0008: Abbruch und Wiederverbindung -------------------------

    /// Die Gegenseite bricht die Verbindung ab (wie ein beendeter
    /// `fake_xport` oder ein XPort, der die Session schließt). Für die alte
    /// Session unterwegs gewesene Antworten verfallen; ein wartendes
    /// `recv_line()` wird geweckt und liefert `Disconnected`.
    pub fn drop_connection(&self) {
        let mut state = self.lock();
        state.disconnected = true;
        state.incoming.clear();
        state.reads_since_change = 0;
        state
            .connection_log
            .push((Instant::now(), ConnectionEvent::Dropped));
        drop(state);
        self.wake.notify_one();
    }

    /// Solange `true`, scheitert jedes `connect()` (z.B. XPort noch nicht
    /// wieder da oder alte Session noch nicht abgebaut).
    pub fn refuse_connections(&self, refuse: bool) {
        self.lock().refuse_connections = refuse;
    }

    /// Nach jedem `disconnect()` des Clients werden neue Verbindungen für
    /// `duration` abgelehnt - nachgebildet nach dem XPort, der eine neue
    /// Session erst annimmt, wenn die alte abgebaut ist (CLAUDE.md). Für
    /// `lab_probe reconnect` (Spec 0008, Messpunkt 1).
    pub fn refuse_after_disconnect(&self, duration: Duration) {
        self.lock().refuse_after_disconnect = duration;
    }

    /// Alle Verbindungsereignisse mit Zeitpunkt.
    pub fn connection_log(&self) -> Vec<(Instant, ConnectionEvent)> {
        self.lock().connection_log.clone()
    }

    /// Alle Lese- und Sendeversuche, die auf einer getrennten Verbindung
    /// stattfanden (Spec 0008, AK1: davon soll es zwischen zwei
    /// Verbindungsversuchen keine geben).
    pub fn io_while_disconnected(&self) -> Vec<(Instant, IoAttempt)> {
        self.lock().io_while_disconnected.clone()
    }

    /// `connect()` des Clients: `true` = angenommen, frische Session.
    pub(crate) fn try_connect(&self) -> bool {
        let mut state = self.lock();
        let now = Instant::now();
        let still_closing = state
            .last_client_disconnect
            .is_some_and(|at| now < at + state.refuse_after_disconnect);
        let accepted = !state.refuse_connections && !still_closing;
        if accepted {
            state.disconnected = false;
            state.incoming.clear();
            state.reads_since_change = 0;
        }
        state
            .connection_log
            .push((Instant::now(), ConnectionEvent::Connect { accepted }));
        accepted
    }

    /// `disconnect()` des Clients: Session geschlossen.
    pub(crate) fn on_disconnect(&self) {
        let mut state = self.lock();
        state.disconnected = true;
        state.incoming.clear();
        state.reads_since_change = 0;
        state.last_client_disconnect = Some(Instant::now());
        state
            .connection_log
            .push((Instant::now(), ConnectionEvent::Disconnect));
        drop(state);
        self.wake.notify_one();
    }

    pub(crate) fn is_disconnected(&self) -> bool {
        self.lock().disconnected
    }

    /// Protokolliert einen Lese- oder Sendeversuch auf der getrennten
    /// Verbindung.
    ///
    /// Wächter gegen hängende Tests: Mit pausierter Tokio-Uhr läuft die
    /// Zeit nur weiter, wenn kein Task bereit ist. Liest ein Client nach
    /// einem Abbruch ohne Pause weiter (die Dauerschleife aus dem Befund zu
    /// Spec 0008), ist er IMMER bereit - der Test würde ewig hängen statt
    /// zu scheitern. Ab [`BUSY_LOOP_READS`] Leseversuchen bricht der Bus
    /// deshalb mit einer klaren Meldung ab.
    pub(crate) fn note_io_while_disconnected(&self, attempt: IoAttempt) {
        let mut state = self.lock();
        state.io_while_disconnected.push((Instant::now(), attempt));
        if attempt == IoAttempt::Recv {
            state.reads_since_change += 1;
        }
        let reads = state.reads_since_change;
        drop(state);
        assert!(
            reads <= BUSY_LOOP_READS,
            "SimBus: Dauerschleife - {reads} Leseversuche auf getrennter Verbindung"
        );
    }

    /// Wie [`Self::next_incoming`], endet aber mit `None`, sobald die
    /// Session getrennt ist (auch wenn das erst während des Wartens
    /// passiert). Ebenso abbruchsicher.
    pub(crate) async fn next_incoming_in_session(&self) -> Option<String> {
        loop {
            let next_due = {
                let mut state = self.lock();
                if state.disconnected {
                    return None;
                }
                match state.incoming.front() {
                    Some((due, _)) if *due <= Instant::now() => {
                        return Some(state.incoming.pop_front().expect("front() war Some").1);
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

/// Ereignis im Verbindungsprotokoll des Busses (Spec 0008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionEvent {
    /// `connect()` des Clients; `accepted = false` bei Ablehnung.
    Connect { accepted: bool },
    /// `disconnect()` des Clients (Session vom Client geschlossen).
    Disconnect,
    /// Abbruch durch die Gegenseite ([`SimBus::drop_connection`]).
    Dropped,
}

/// Lese- oder Sendeversuch auf einer getrennten Verbindung (Spec 0008, AK1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoAttempt {
    Recv,
    Send,
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

    #[tokio::test(start_paused = true)]
    async fn modul_ohne_quittung_uebernimmt_den_wert_und_antwortet_auf_ruecklesen() {
        let (mut conn, bus) = connection_with_bus();
        bus.set_value(4, 0, "1000.0");
        bus.drop_acks(4);

        conn.send_line("4:0=2500!").await.unwrap();
        assert_eq!(recv_within(&mut conn, Duration::from_secs(1)).await, None);

        conn.send_line("4:0?").await.unwrap();
        assert_eq!(
            recv_within(&mut conn, Duration::from_secs(1))
                .await
                .as_deref(),
            Some("#4:0=2500")
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

    // --- Spec 0008: Abbruch und Wiederverbindung -------------------------

    use crate::TransportError;

    #[tokio::test(start_paused = true)]
    async fn ohne_abbruch_keine_fehler() {
        let (mut conn, bus) = connection_with_bus();
        bus.set_value(1, 0, "0.5");

        conn.connect().await.unwrap();
        conn.send_line("1:0?").await.unwrap();

        assert_eq!(
            recv_within(&mut conn, Duration::from_secs(1))
                .await
                .as_deref(),
            Some("#1:0=0.5")
        );
        assert!(bus.io_while_disconnected().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn nach_abbruch_liefert_lesen_sofort_und_wiederholt_disconnected() {
        let (mut conn, bus) = connection_with_bus();
        bus.drop_connection();

        for _ in 0..3 {
            let result = tokio::time::timeout(Duration::ZERO, conn.recv_line()).await;
            assert!(
                matches!(result, Ok(Err(TransportError::Disconnected))),
                "erwartet sofort Disconnected, kam: {result:?}"
            );
        }
        let reads = bus
            .io_while_disconnected()
            .iter()
            .filter(|(_, io)| *io == IoAttempt::Recv)
            .count();
        assert_eq!(reads, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn nach_abbruch_scheitert_senden_und_nichts_kommt_an() {
        let (mut conn, bus) = connection_with_bus();
        let client_log = conn.sent_handle();
        bus.drop_connection();

        let result = conn.send_line("4:0=2500!").await;

        assert!(
            matches!(result, Err(TransportError::Disconnected)),
            "kam: {result:?}"
        );
        assert!(bus.sent().is_empty(), "Bus hat empfangen: {:?}", bus.sent());
        assert!(client_log.lock().unwrap().is_empty());
        assert_eq!(
            bus.io_while_disconnected()
                .iter()
                .map(|(_, io)| *io)
                .collect::<Vec<_>>(),
            vec![IoAttempt::Send]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn wartendes_lesen_wird_durch_abbruch_beendet() {
        let (mut conn, bus) = connection_with_bus();
        let dropper = bus.clone();
        let drop_later = async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            dropper.drop_connection();
        };

        let (result, ()) = tokio::join!(
            tokio::time::timeout(Duration::from_secs(1), conn.recv_line()),
            drop_later
        );

        assert!(
            matches!(result, Ok(Err(TransportError::Disconnected))),
            "kam: {result:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn abgelehnte_verbindung_scheitert_und_wird_protokolliert() {
        let (mut conn, bus) = connection_with_bus();
        bus.drop_connection();
        bus.refuse_connections(true);

        assert!(conn.connect().await.is_err());

        bus.refuse_connections(false);
        conn.connect().await.unwrap();

        let events: Vec<ConnectionEvent> =
            bus.connection_log().into_iter().map(|(_, e)| e).collect();
        assert_eq!(
            events,
            vec![
                ConnectionEvent::Dropped,
                ConnectionEvent::Connect { accepted: false },
                ConnectionEvent::Connect { accepted: true },
            ]
        );
    }

    /// Eine Wiederverbindung ist eine neue Session: Antworten, die für die
    /// alte Session unterwegs waren, kommen nicht mehr an; Modulwerte
    /// bleiben (die Module selbst laufen weiter).
    #[tokio::test(start_paused = true)]
    async fn wiederverbindung_startet_eine_frische_session() {
        let (mut conn, bus) = connection_with_bus();
        bus.set_value(1, 0, "0.5");
        bus.set_reply_delay(Duration::from_millis(400));
        conn.send_line("1:0?").await.unwrap(); // Antwort wäre bei 400 ms fällig

        tokio::time::sleep(Duration::from_millis(100)).await;
        bus.drop_connection();
        conn.disconnect().await.unwrap();
        conn.connect().await.unwrap();
        bus.set_reply_delay(Duration::ZERO);

        // Die alte Antwort darf nicht mehr kommen ...
        assert_eq!(recv_within(&mut conn, Duration::from_secs(1)).await, None);
        // ... eine neue Abfrage wird normal beantwortet.
        conn.send_line("1:0?").await.unwrap();
        assert_eq!(
            recv_within(&mut conn, Duration::from_secs(1))
                .await
                .as_deref(),
            Some("#1:0=0.5")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn verbindungsprotokoll_haelt_zeitpunkte_fest() {
        let (mut conn, bus) = connection_with_bus();
        let start = Instant::now();

        tokio::time::sleep(Duration::from_secs(1)).await;
        bus.drop_connection();
        tokio::time::sleep(Duration::from_secs(1)).await;
        conn.disconnect().await.unwrap();
        bus.refuse_connections(true);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let _ = conn.connect().await;
        bus.refuse_connections(false);
        tokio::time::sleep(Duration::from_secs(2)).await;
        conn.connect().await.unwrap();

        let log: Vec<(Duration, ConnectionEvent)> = bus
            .connection_log()
            .into_iter()
            .map(|(at, e)| (at - start, e))
            .collect();
        assert_eq!(
            log,
            vec![
                (Duration::from_secs(1), ConnectionEvent::Dropped),
                (Duration::from_secs(2), ConnectionEvent::Disconnect),
                (
                    Duration::from_secs(3),
                    ConnectionEvent::Connect { accepted: false }
                ),
                (
                    Duration::from_secs(5),
                    ConnectionEvent::Connect { accepted: true }
                ),
            ]
        );
    }

    /// Wächter gegen hängende Tests: Liest ein Client auf der getrennten
    /// Verbindung ohne Pause weiter (Dauerschleife wie im Befund zu Spec
    /// 0008), würde ein Test mit pausierter Uhr sonst ewig hängen, statt
    /// zu scheitern - die Zeit läuft nie weiter, solange ein Task bereit
    /// ist.
    #[tokio::test(start_paused = true)]
    #[should_panic(expected = "Dauerschleife")]
    async fn waechter_bricht_dauerlesen_auf_getrennter_verbindung_ab() {
        let (mut conn, bus) = connection_with_bus();
        bus.drop_connection();
        // Schutz gegen Hängen, falls der Wächter fehlt: nach 10 s virtueller
        // Zeit endet der Test OHNE Panic und scheitert damit.
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let _ = conn.recv_line().await;
            }
        })
        .await;
    }

    /// Wie am XPort beobachtet (CLAUDE.md): Direkt nach dem Schließen einer
    /// Session wird eine neue abgelehnt, bis die alte abgebaut ist.
    #[tokio::test(start_paused = true)]
    async fn nach_disconnect_werden_verbindungen_eine_weile_abgelehnt() {
        let (mut conn, bus) = connection_with_bus();
        bus.refuse_after_disconnect(Duration::from_millis(350));
        conn.connect().await.unwrap();
        conn.disconnect().await.unwrap();

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            conn.connect().await.is_err(),
            "nach 300 ms schon angenommen"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        conn.connect().await.unwrap();
    }
}
