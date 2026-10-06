//! Umgebungsebene ("Environment Layer") – der zentrale Einstiegspunkt, wie
//! JLabs `Lab`-Klasse bzw. die "Umgebungsebene" der C#-CtLab-Library.
//!
//! Übersetzt JLabs Thread-basiertes Sync/Async-Muster (siehe JLab1.doc:
//! Sender-Thread hinterlegt sich beim Model, Empfänger-Thread weckt ihn via
//! `notify`, 500ms-Timeout, danach Rückgabewert 0.0) in async Rust:
//!
//! - `query()`  entspricht JLabs `queryValue()` (blockierend, mit 500ms-Timeout)
//! - `send_set()` entspricht JLabs `sendCommand()` (fire-and-forget)
//! - `subscribe()` entspricht JLabs Observer-Registrierung am Model – jeder
//!   Interessent (z.B. ein WebSocket-Client im späteren `octlab-server`)
//!   bekommt einen eigenen `broadcast::Receiver` und sieht ALLE eingehenden
//!   Werte, auch unaufgefordert gesendete (Panel-Bedienung, Trigger).
//!
//! Architektur-Entscheidung: Die serielle/TCP-Verbindung zum c't-Lab ist
//! physikalisch ein einziger, geteilter Bus (siehe "Elektrischer Aufbau und
//! Verdrahtung": ein OptoBus-Kabel für alle Module in der Kette). Deshalb
//! gibt es hier genau EINE Task ("Actor"), die die Verbindung exklusiv
//! besitzt; alle Wünsche laufen als Aufträge ([`Job`]) über einen
//! `mpsc`-Kanal zu ihr rein, empfangene Nachrichten verteilt sie über
//! `broadcast` wieder raus. Das bildet die reale Hardware-Topologie 1:1 ab,
//! statt sie zu verstecken.
//!
//! Seit Spec 0005 arbeitet der Actor die Aufträge EINZELN ab: Ein Auftrag
//! (Abfrage, Setzen, Setz-Sequenz mit Rücklesen) läuft vollständig durch,
//! inklusive Warten auf die Antwort, bevor der nächste beginnt. Aufträge
//! von außen haben Vorrang vor den Abfragen des eigenen Poll-Zyklus
//! ([`Lab::spawn_with_polling`]). Damit ist die Setz-Sequenz atomar und
//! Poll-Zyklen können nicht überlappen - ohne Sperre (Mutex) um den Bus
//! außerhalb des Actors.

use octlab_protocol::{parse_message, ChannelKey, Command, Message, STATUS_SUBCHANNEL};
use octlab_transport::{BoardConnection, RawLine, TransportError};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::Instant;

/// Wie lange auf eine Antwort (Messwert oder Quittung) gewartet wird,
/// gerechnet ab dem Senden des Kommandos. Wert wie im JLab-Original
/// übernommen (siehe JLab1.doc, Abschnitt "Synchron/Asynchron").
const QUERY_TIMEOUT: Duration = Duration::from_millis(500);

/// Default-Intervall des Poll-Zyklus (Spec 0005).
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// Frühester erneuter Versuch für einen Kanal nach einem Timeout (Spec 0005).
pub const POLL_BACKOFF: Duration = Duration::from_secs(5);

/// Ergebnis eines quittierten Setz-Vorgangs ([`Lab::set`]).
///
/// Bewusst drei unterscheidbare Fälle statt `Option`/`bool` (Spec 0003,
/// AK1-AK3): Am realen Gerät verifiziert bedeutet eine Fehlerquittung
/// (`PARERR`) NICHT, dass der Wert unverändert blieb - die Firmware klemmt
/// z.B. übergroße Werte und quittiert trotzdem mit Fehler. Der Aufrufer
/// muss deshalb Ablehnung und Nicht-Antwort getrennt behandeln können und
/// den tatsächlichen Zustand per Rücklesen ermitteln.
#[derive(Debug, Clone, PartialEq)]
pub enum SetOutcome {
    /// Quittung mit Code 0 eingetroffen (`#<addr>:255=0 [OK]`).
    Confirmed { status_text: Option<String> },
    /// Quittung mit Code != 0 eingetroffen (z.B. `#<addr>:255=5 [PARERR]`).
    Rejected {
        code: f64,
        status_text: Option<String>,
    },
    /// Keine Quittung innerhalb des Timeouts - dritter Fall neben Erfolg
    /// und Ablehnung, analog zur `Option<f64>`-Entscheidung bei `query()`.
    NoReply,
}

/// Konfiguration des Poll-Zyklus (Spec 0005): feste Kanalliste plus
/// Intervall. Intervall 0 (oder eine leere Liste) = Polling aus.
#[derive(Debug, Clone, PartialEq)]
pub struct PollConfig {
    pub channels: Vec<ChannelKey>,
    pub interval: Duration,
}

impl PollConfig {
    /// Kanalliste mit [`DEFAULT_POLL_INTERVAL`].
    pub fn new(channels: Vec<ChannelKey>) -> Self {
        Self {
            channels,
            interval: DEFAULT_POLL_INTERVAL,
        }
    }
}

/// Zustand eines Kanals der Kanalliste, wie ihn der Änderungsstrom und der
/// Snapshot ([`Lab::subscribe_channels`]) melden.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelUpdate {
    pub key: ChannelKey,
    /// Letzter bekannter Wert; `None` = Kanal hat noch nie geantwortet.
    /// Ein Timeout löscht ihn NICHT (Spec 0005, AK5).
    pub value: Option<f64>,
    /// Letzte Abfrage lief ins Timeout.
    pub stale: bool,
}

/// Quittung eines Setz-Kommandos (Statuskanal 255).
#[derive(Debug, Clone, PartialEq)]
pub struct Ack {
    /// 0 = angenommen, sonst Fehlercode (z.B. 5 = PARERR).
    pub code: f64,
    pub status_text: Option<String>,
}

/// Ergebnis von [`Lab::set_and_read_back`]: Quittung und Rücklesewert
/// getrennt, beide optional - auch ohne Quittung wird zurückgelesen
/// (Spec 0005 AK6, abweichend von Spec 0003: verbindlich ist nur das
/// Rücklesen).
#[derive(Debug, Clone, PartialEq)]
pub struct SetReadBack {
    pub ack: Option<Ack>,
    pub readback: Option<f64>,
}

/// Kennzahlen des Poll-Zyklus (Spec 0005, AK10).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PollStats {
    /// Konfiguriertes Intervall (0 = Polling aus).
    pub interval: Duration,
    /// Gemessene Dauer des zuletzt abgeschlossenen Zyklus.
    pub last_cycle: Option<Duration>,
    pub completed_cycles: u64,
    /// Anzahl der Episoden, in denen Zyklen länger als das Intervall
    /// dauerten (pro Episode wird genau einmal gewarnt).
    pub overrun_episodes: u64,
}

/// Auftrag an den Actor. Das Ergebnis geht über einen `oneshot`-Kanal an
/// den wartenden Aufrufer zurück.
enum Job {
    Send(String),
    Query {
        key: ChannelKey,
        reply: oneshot::Sender<Option<f64>>,
    },
    Set {
        key: ChannelKey,
        value: f64,
        reply: oneshot::Sender<SetOutcome>,
    },
    SetAndReadBack {
        key: ChannelKey,
        value: f64,
        reply: oneshot::Sender<SetReadBack>,
    },
}

/// Kanalzustände der Kanalliste plus Änderungsstrom. Liegt hinter EINEM
/// Mutex, damit "Snapshot lesen + Strom abonnieren" atomar ist: Ein neuer
/// Abonnent verpasst so keine Änderung zwischen Snapshot und Abo.
struct ChannelTable {
    /// In Reihenfolge der Kanalliste; `None` = noch nie abgefragt.
    entries: Vec<(ChannelKey, Option<ChannelUpdate>)>,
    changes: broadcast::Sender<ChannelUpdate>,
}

impl ChannelTable {
    /// Übernimmt einen neuen Zustand und meldet ihn NUR bei Änderung von
    /// Wert oder Veraltet-Markierung. Kanäle außerhalb der Liste werden
    /// ignoriert.
    fn apply(&mut self, key: ChannelKey, value: Option<f64>, stale: bool) {
        let Some((_, current)) = self.entries.iter_mut().find(|(k, _)| *k == key) else {
            return;
        };
        let update = ChannelUpdate { key, value, stale };
        if current.as_ref() != Some(&update) {
            *current = Some(update.clone());
            let _ = self.changes.send(update);
        }
    }

    fn value(&self, key: ChannelKey) -> Option<f64> {
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, state)| state.as_ref())
            .and_then(|state| state.value)
    }
}

/// Zustand, den Actor und `Lab`-Handle teilen. `Arc` = geteilte Ownership
/// (Actor-Task und Handle leben unabhängig voneinander), `Mutex` = sichere
/// Zugriffe aus beiden. Die Locks werden nie über ein `.await` gehalten.
struct Shared {
    channels: Mutex<ChannelTable>,
    stats: Mutex<PollStats>,
}

pub struct Lab {
    jobs: mpsc::UnboundedSender<Job>,
    updates: broadcast::Sender<Message>,
    shared: Arc<Shared>,
}

impl Lab {
    /// Verbindet und startet die Actor-Task für die übergebene Verbindung,
    /// ohne Polling.
    ///
    /// `connect()` passiert bewusst HIER, vor dem `tokio::spawn`, und nicht
    /// als erster Schritt innerhalb der Actor-Task: ein Aufrufer, der einen
    /// Verbindungsfehler fail-fast behandeln will (z.B. `octlab-server` beim
    /// Start), braucht das Ergebnis synchron zurück. Würde stattdessen die
    /// Task selbst verbinden, gäbe es kein Signal nach außen außer einem
    /// Log-Eintrag – der Aufrufer hätte ein scheinbar funktionierendes
    /// `Lab`-Handle, dessen `query()` aber für immer timeoutet. Ein
    /// zusätzlicher separater "Preflight"-Connect vor diesem hier wäre KEINE
    /// Alternative: am echten XPort (nur eine aktive TCP-Session gleichzeitig)
    /// wurde live beobachtet, dass ein zweiter Connect-Versuch kurz nach dem
    /// ersten mit "Connection refused" abgewiesen wird – der Connect, der
    /// geprüft wird, muss also derselbe sein, der auch benutzt wird.
    pub async fn spawn(connection: Box<dyn BoardConnection>) -> Result<Self, TransportError> {
        Self::spawn_inner(connection, None).await
    }

    /// Wie [`Self::spawn`], zusätzlich mit Poll-Zyklus über eine feste
    /// Kanalliste (Spec 0005). Der erste Zyklus startet sofort.
    pub async fn spawn_with_polling(
        connection: Box<dyn BoardConnection>,
        config: PollConfig,
    ) -> Result<Self, TransportError> {
        Self::spawn_inner(connection, Some(config)).await
    }

    async fn spawn_inner(
        mut connection: Box<dyn BoardConnection>,
        config: Option<PollConfig>,
    ) -> Result<Self, TransportError> {
        connection.connect().await?;

        // Intervall 0 oder leere Liste = Polling aus (Spec 0005, AK2).
        let config = config.filter(|c| !c.interval.is_zero() && !c.channels.is_empty());

        let (jobs_tx, jobs_rx) = mpsc::unbounded_channel();
        let (updates_tx, _) = broadcast::channel(256);
        let (changes_tx, _) = broadcast::channel(256);
        let shared = Arc::new(Shared {
            channels: Mutex::new(ChannelTable {
                entries: config
                    .iter()
                    .flat_map(|c| c.channels.iter().map(|key| (*key, None)))
                    .collect(),
                changes: changes_tx,
            }),
            stats: Mutex::new(PollStats {
                interval: config.as_ref().map_or(Duration::ZERO, |c| c.interval),
                ..PollStats::default()
            }),
        });

        let actor = Actor {
            connection,
            jobs: jobs_rx,
            queued: VecDeque::new(),
            updates: updates_tx.clone(),
            shared: shared.clone(),
            poll: config.map(Poller::new),
        };
        tokio::spawn(actor.run());

        Ok(Self {
            jobs: jobs_tx,
            updates: updates_tx,
            shared,
        })
    }

    /// Sendet einen Befehl, ohne auf eine Antwort zu warten.
    /// Entspricht JLabs `sendCommand()`.
    pub fn send_set(&self, key: ChannelKey, value: f64) {
        let _ = self
            .jobs
            .send(Job::Send(Command::SetFloat(key, value).to_wire()));
    }

    /// Fragt einen Wert ab und wartet bis zu 500ms (ab dem Senden) auf die
    /// Antwort. Entspricht JLabs `queryValue()`. Gibt `None` zurück bei
    /// Timeout oder wenn die Verbindung bereits geschlossen ist (JLab gibt
    /// hier 0.0 zurück; wir bevorzugen `Option`, damit "kein Wert" nicht mit
    /// dem validen Messwert 0.0 verwechselt werden kann).
    pub async fn query(&self, key: ChannelKey) -> Option<f64> {
        let (reply, rx) = oneshot::channel();
        if self.jobs.send(Job::Query { key, reply }).is_err() {
            return None;
        }
        rx.await.unwrap_or(None)
    }

    /// Setzt einen Kanalwert und wartet auf die Quittung des Moduls (ohne
    /// Rücklesen). Siehe [`SetOutcome`] und Spec 0003.
    ///
    /// Die Quittung kommt NICHT auf dem gesetzten Kanal zurück, sondern
    /// unaufgefordert auf dem Statuskanal 255 des Moduls (am realen Gerät
    /// verifiziert: `4:0=1234.5!` → `#4:255=0 [OK]`).
    pub async fn set(&self, key: ChannelKey, value: f64) -> SetOutcome {
        let (reply, rx) = oneshot::channel();
        if self.jobs.send(Job::Set { key, value, reply }).is_err() {
            return SetOutcome::NoReply;
        }
        rx.await.unwrap_or(SetOutcome::NoReply)
    }

    /// Setzt einen Kanalwert, wartet auf die Quittung und liest den Kanal
    /// zurück - als EINE unteilbare Sequenz im Actor, ohne Poll-Abfrage
    /// dazwischen (Spec 0005, AK6). Zurückgelesen wird auch ohne Quittung;
    /// im schlechtesten Fall dauert die Sequenz 500 ms (Quittung) + 500 ms
    /// (Rücklesen). Ein Rücklesewert für einen Kanal der Kanalliste
    /// aktualisiert auch dessen Zustand.
    pub async fn set_and_read_back(&self, key: ChannelKey, value: f64) -> SetReadBack {
        let nothing = SetReadBack {
            ack: None,
            readback: None,
        };
        let (reply, rx) = oneshot::channel();
        if self
            .jobs
            .send(Job::SetAndReadBack { key, value, reply })
            .is_err()
        {
            return nothing;
        }
        rx.await.unwrap_or(nothing)
    }

    /// Aktueller Zustand aller Kanäle der Kanalliste, die schon einmal
    /// abgefragt wurden (Snapshot), plus Änderungsstrom: meldet einen Kanal
    /// nur, wenn sich Wert oder Veraltet-Markierung ändern. Beides atomar
    /// zueinander - zwischen Snapshot und erster Meldung geht nichts
    /// verloren. Unaufgeforderte Zeilen erscheinen hier nie (Spec 0005, AK9).
    pub fn subscribe_channels(&self) -> (Vec<ChannelUpdate>, broadcast::Receiver<ChannelUpdate>) {
        let table = self.shared.channels.lock().unwrap();
        let snapshot = table
            .entries
            .iter()
            .filter_map(|(_, state)| state.clone())
            .collect();
        (snapshot, table.changes.subscribe())
    }

    /// Kennzahlen des Poll-Zyklus (Spec 0005, AK10).
    pub fn poll_stats(&self) -> PollStats {
        self.shared.stats.lock().unwrap().clone()
    }

    /// Abonniert alle eingehenden Nachrichten, egal ob Antwort auf eine
    /// eigene Query oder unaufgefordert vom c't-Lab gesendet.
    pub fn subscribe(&self) -> broadcast::Receiver<Message> {
        self.updates.subscribe()
    }
}

/// Zustand des Poll-Zyklus im Actor.
struct Poller {
    config: PollConfig,
    /// Kanäle im Backoff: frühester nächster Versuch.
    retry_after: HashMap<ChannelKey, Instant>,
    cycle: Option<Cycle>,
    next_cycle_at: Instant,
    in_overrun: bool,
}

struct Cycle {
    started: Instant,
    remaining: VecDeque<ChannelKey>,
}

impl Poller {
    fn new(config: PollConfig) -> Self {
        Self {
            config,
            retry_after: HashMap::new(),
            cycle: None,
            next_cycle_at: Instant::now(),
            in_overrun: false,
        }
    }

    /// Beginnt einen Zyklus mit allen Kanälen, die nicht im Backoff sind.
    fn start_cycle(&mut self) {
        let now = Instant::now();
        let remaining = self
            .config
            .channels
            .iter()
            .filter(|key| self.retry_after.get(key).is_none_or(|at| *at <= now))
            .copied()
            .collect();
        self.cycle = Some(Cycle {
            started: now,
            remaining,
        });
    }

    /// Schließt einen Zyklus ab: Dauer messen, Überlauf behandeln, nächsten
    /// Start festlegen (Spec 0005, AK10: kein Überlappen, kein Nachholen -
    /// dauerte der Zyklus länger als das Intervall, startet der nächste
    /// sofort, sonst im Takt).
    fn finish_cycle(&mut self, started: Instant, stats: &Mutex<PollStats>) {
        let now = Instant::now();
        let duration = now - started;
        let overrun = duration > self.config.interval;
        let mut stats = stats.lock().unwrap();
        stats.last_cycle = Some(duration);
        stats.completed_cycles += 1;
        if overrun {
            if !self.in_overrun {
                stats.overrun_episodes += 1;
                tracing::warn!(
                    ?duration,
                    interval = ?self.config.interval,
                    "Poll-Zyklus dauert länger als das Intervall - nächster Zyklus startet sofort"
                );
            }
            self.next_cycle_at = now;
        } else {
            self.next_cycle_at = started + self.config.interval;
        }
        self.in_overrun = overrun;
    }
}

/// Die Actor-Task: besitzt die Verbindung exklusiv.
struct Actor {
    connection: Box<dyn BoardConnection>,
    jobs: mpsc::UnboundedReceiver<Job>,
    /// Bereits angenommene, noch nicht ausgeführte Aufträge (FIFO).
    queued: VecDeque<Job>,
    updates: broadcast::Sender<Message>,
    shared: Arc<Shared>,
    poll: Option<Poller>,
}

impl Actor {
    async fn run(mut self) {
        loop {
            // 1. Alle wartenden Aufträge übernehmen. Sind alle `Lab`-Handles
            //    weg, endet der Actor.
            loop {
                match self.jobs.try_recv() {
                    Ok(job) => self.queued.push_back(job),
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => return,
                }
            }

            // 2. Aufträge von außen haben Vorrang vor dem Polling.
            if let Some(job) = self.queued.pop_front() {
                self.execute(job).await;
                continue;
            }

            // 3. Nächste Abfrage des laufenden Poll-Zyklus.
            if let Some(key) = self.next_poll_key() {
                self.poll_one(key).await;
                continue;
            }

            // 4. Nichts zu tun: auf einen Auftrag, eine eingehende Zeile oder
            //    den Start des nächsten Zyklus warten.
            let next_cycle_at = self.poll.as_ref().map(|p| p.next_cycle_at);
            tokio::select! {
                biased;

                job = self.jobs.recv() => match job {
                    Some(job) => self.queued.push_back(job),
                    None => return,
                },
                line = self.connection.recv_line() => {
                    if let Some(msg) = self.receive(line) {
                        log_unsolicited(&msg);
                    }
                }
                () = sleep_until_or_forever(next_cycle_at) => {
                    if let Some(poll) = self.poll.as_mut() {
                        poll.start_cycle();
                    }
                }
            }
        }
    }

    /// Nächster Kanal des laufenden Zyklus; schließt einen leer gewordenen
    /// Zyklus ab.
    fn next_poll_key(&mut self) -> Option<ChannelKey> {
        let poll = self.poll.as_mut()?;
        let cycle = poll.cycle.as_mut()?;
        if let Some(key) = cycle.remaining.pop_front() {
            return Some(key);
        }
        let started = cycle.started;
        poll.cycle = None;
        poll.finish_cycle(started, &self.shared.stats);
        None
    }

    async fn poll_one(&mut self, key: ChannelKey) {
        self.send(&Command::Query(key).to_wire()).await;
        let answer = self.await_key(key).await;
        let mut channels = self.shared.channels.lock().unwrap();
        match answer {
            Some(msg) => {
                channels.apply(key, Some(msg.value), false);
                if let Some(poll) = self.poll.as_mut() {
                    poll.retry_after.remove(&key);
                }
            }
            None => {
                // Timeout: veraltet markieren, letzten Wert behalten (AK5),
                // frühestens nach dem Backoff erneut versuchen (AK4).
                let last = channels.value(key);
                channels.apply(key, last, true);
                if let Some(poll) = self.poll.as_mut() {
                    poll.retry_after.insert(key, Instant::now() + POLL_BACKOFF);
                }
            }
        }
    }

    async fn execute(&mut self, job: Job) {
        match job {
            Job::Send(line) => self.send(&line).await,
            Job::Query { key, reply } => {
                self.send(&Command::Query(key).to_wire()).await;
                let value = self.await_key(key).await.map(|msg| msg.value);
                let _ = reply.send(value);
            }
            Job::Set { key, value, reply } => {
                let outcome = match self.send_and_await_ack(key, value).await {
                    // Quittungs-Code 0 = angenommen, alles andere ist eine
                    // Ablehnung (real beobachtet: 5 [PARERR]). ACHTUNG:
                    // Ablehnung heißt nicht "unverändert" - die Firmware
                    // klemmt Werte und quittiert trotzdem mit Fehler; den
                    // Ist-Zustand liefert nur Rücklesen.
                    Some(ack) if ack.code == 0.0 => SetOutcome::Confirmed {
                        status_text: ack.status_text,
                    },
                    Some(ack) => SetOutcome::Rejected {
                        code: ack.code,
                        status_text: ack.status_text,
                    },
                    None => SetOutcome::NoReply,
                };
                let _ = reply.send(outcome);
            }
            Job::SetAndReadBack { key, value, reply } => {
                let ack = self.send_and_await_ack(key, value).await;
                // Rücklesen IMMER, auch ohne Quittung (Spec 0005 AK6,
                // Stellglied-Regel: verbindlich ist nur das Rücklesen).
                self.send(&Command::Query(key).to_wire()).await;
                let readback = self.await_key(key).await.map(|msg| msg.value);
                if let Some(value) = readback {
                    self.shared
                        .channels
                        .lock()
                        .unwrap()
                        .apply(key, Some(value), false);
                }
                let _ = reply.send(SetReadBack { ack, readback });
            }
        }
    }

    async fn send_and_await_ack(&mut self, key: ChannelKey, value: f64) -> Option<Ack> {
        self.send(&Command::SetFloat(key, value).to_wire()).await;
        let ack_key = ChannelKey {
            address: key.address,
            subchannel: STATUS_SUBCHANNEL,
        };
        self.await_key(ack_key).await.map(|msg| Ack {
            code: msg.value,
            status_text: msg.status_text,
        })
    }

    async fn send(&mut self, line: &str) {
        if let Err(err) = self.connection.send_line(line).await {
            tracing::warn!(?err, %line, "Senden fehlgeschlagen");
        }
    }

    /// Wartet bis zu [`QUERY_TIMEOUT`] auf eine Nachricht für `key`. Alle
    /// anderen Zeilen, die währenddessen eintreffen, werden wie gewohnt an
    /// die Abonnenten verteilt, ändern aber keinen Kanalzustand.
    async fn await_key(&mut self, key: ChannelKey) -> Option<Message> {
        let deadline = Instant::now() + QUERY_TIMEOUT;
        loop {
            tokio::select! {
                biased;

                line = self.connection.recv_line() => {
                    if let Some(msg) = self.receive(line) {
                        if msg.key == key {
                            return Some(msg);
                        }
                        log_unsolicited(&msg);
                    }
                }
                () = tokio::time::sleep_until(deadline) => return None,
            }
        }
    }

    /// Parst eine empfangene Zeile und verteilt sie an alle Abonnenten.
    /// Den Kanalzustand ändert sie hier NICHT - das tun nur Antworten auf
    /// eigene Abfragen (Spec 0005, AK9).
    fn receive(&mut self, line: Result<RawLine, TransportError>) -> Option<Message> {
        let raw = match line {
            Ok(raw) => raw,
            Err(err) => {
                tracing::trace!(?err, "kein Empfang (Timeout ist normal)");
                return None;
            }
        };
        match parse_message(&raw) {
            Ok(msg) => {
                let _ = self.updates.send(msg.clone());
                Some(msg)
            }
            Err(err) => {
                tracing::debug!(?err, %raw, "unparsbare Zeile ignoriert");
                None
            }
        }
    }
}

/// Eine gültige Zeile, auf die gerade niemand wartet (der Actor wartet
/// immer auf höchstens einen Kanal), ist unaufgefordert - z.B.
/// Panel-Bedienung. Sie geht an die Abonnenten, ändert aber keinen
/// Kanalzustand (Spec 0005, AK9) - das wird hier festgehalten.
fn log_unsolicited(msg: &Message) {
    tracing::info!(
        address = msg.key.address.0,
        subchannel = msg.key.subchannel.0,
        value = msg.value,
        status_text = ?msg.status_text,
        "unaufgeforderte Zeile: an Abonnenten verteilt, Kanalzustand unverändert"
    );
}

/// Schläft bis `at`, oder für immer bei `None` (kein Polling).
async fn sleep_until_or_forever(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octlab_protocol::{ModuleAddress, SubChannel};
    use octlab_transport::SimulatedConnection;

    #[tokio::test]
    async fn query_resolves_with_matching_response() {
        let mut sim = SimulatedConnection::new("sim");
        sim.push_response("#0:0=1.23456");
        let lab = Lab::spawn(Box::new(sim)).await.unwrap();

        let key = ChannelKey {
            address: ModuleAddress(0),
            subchannel: SubChannel(0),
        };
        let value = lab.query(key).await;
        assert_eq!(value, Some(1.23456));
    }

    #[tokio::test]
    async fn query_times_out_without_response() {
        let sim = SimulatedConnection::new("sim"); // keine Antwort in der Queue
        let lab = Lab::spawn(Box::new(sim)).await.unwrap();

        let key = ChannelKey {
            address: ModuleAddress(0),
            subchannel: SubChannel(0),
        };
        let value = lab.query(key).await;
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn subscribers_see_unsolicited_messages() {
        let mut sim = SimulatedConnection::new("sim");
        // Simuliert z.B. eine Panel-Bedienung: Wert kommt ohne vorherige Abfrage.
        sim.push_response("#2:1=42.0");
        let lab = Lab::spawn(Box::new(sim)).await.unwrap();

        let mut rx = lab.subscribe();
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.value, 42.0);
    }
}
