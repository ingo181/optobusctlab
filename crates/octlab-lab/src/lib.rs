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
//! Statt eines geparkten OS-Threads pro wartender Anfrage (JLab) verwenden
//! wir pro Anfrage einen `oneshot::channel` – kostet keinen OS-Thread, der
//! Tokio-Scheduler kümmert sich ums Aufwecken.
//!
//! Architektur-Entscheidung: Die serielle/TCP-Verbindung zum c't-Lab ist
//! physikalisch ein einziger, geteilter Bus (siehe "Elektrischer Aufbau und
//! Verdrahtung": ein OptoBus-Kabel für alle Module in der Kette). Deshalb
//! gibt es hier genau EINE Task ("Actor"), die die Verbindung exklusiv
//! besitzt; alle Sende-Wünsche laufen über einen `mpsc`-Kanal zu ihr rein,
//! empfangene Nachrichten verteilt sie über `broadcast` wieder raus. Das
//! bildet die reale Hardware-Topologie 1:1 ab, statt sie zu verstecken.

use octlab_protocol::{parse_message, ChannelKey, Command, Message, STATUS_SUBCHANNEL};
use octlab_transport::{BoardConnection, TransportError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};

/// Wie lange `query()` maximal auf eine Antwort wartet, bevor sie aufgibt.
/// Wert wie im JLab-Original übernommen (siehe JLab1.doc, Abschnitt
/// "Synchron/Asynchron").
const QUERY_TIMEOUT: Duration = Duration::from_millis(500);

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

pub struct Lab {
    outgoing: mpsc::UnboundedSender<String>,
    pending: Arc<Mutex<HashMap<ChannelKey, oneshot::Sender<Message>>>>,
    updates: broadcast::Sender<Message>,
}

impl Lab {
    /// Verbindet und startet die Actor-Task für die übergebene Verbindung,
    /// gibt dann ein `Lab`-Handle zurück, das beliebig oft `clone()`d werden
    /// kann (billig: nur Channel-Handles, keine eigene Verbindung).
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
    pub async fn spawn(mut connection: Box<dyn BoardConnection>) -> Result<Self, TransportError> {
        connection.connect().await?;

        let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<String>();
        let pending: Arc<Mutex<HashMap<ChannelKey, oneshot::Sender<Message>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (updates_tx, _) = broadcast::channel(256);

        let pending_for_task = pending.clone();
        let updates_for_task = updates_tx.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;

                    // Ausgehende Befehle haben Vorrang, damit Steuerbefehle
                    // (z.B. Netzteil abschalten) nicht hinter einer langsamen
                    // Messung anstehen.
                    maybe_line = outgoing_rx.recv() => {
                        match maybe_line {
                            Some(line) => {
                                if let Err(err) = connection.send_line(&line).await {
                                    tracing::warn!(?err, %line, "Senden fehlgeschlagen");
                                }
                            }
                            None => break, // alle Lab-Handles wurden gedroppt
                        }
                    }

                    result = connection.recv_line() => {
                        match result {
                            Ok(raw) => Self::dispatch(&raw, &pending_for_task, &updates_for_task),
                            Err(err) => {
                                tracing::trace!(?err, "kein Empfang (Timeout ist normal)");
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            outgoing: outgoing_tx,
            pending,
            updates: updates_tx,
        })
    }

    fn dispatch(
        raw: &str,
        pending: &Mutex<HashMap<ChannelKey, oneshot::Sender<Message>>>,
        updates: &broadcast::Sender<Message>,
    ) {
        let msg = match parse_message(raw) {
            Ok(msg) => msg,
            Err(err) => {
                tracing::debug!(?err, %raw, "unparsbare Zeile ignoriert");
                return;
            }
        };

        // Falls jemand genau auf diesen Adresse/Subkanal-Schlüssel wartet:
        // wecke ihn auf. Kein Fehler, wenn niemand wartet (unaufgeforderte
        // Nachricht, z.B. vom PM8-Panel oder einem Trigger).
        if let Some(tx) = pending.lock().unwrap().remove(&msg.key) {
            let _ = tx.send(msg.clone());
        }

        // Immer auch an alle Abonnenten weiterreichen (Web-UI, Logging, ...).
        let _ = updates.send(msg);
    }

    /// Sendet einen Befehl, ohne auf eine Antwort zu warten.
    /// Entspricht JLabs `sendCommand()`.
    pub fn send_set(&self, key: ChannelKey, value: f64) {
        let _ = self.outgoing.send(Command::SetFloat(key, value).to_wire());
    }

    /// Fragt einen Wert ab und wartet bis zu 500ms auf die Antwort.
    /// Entspricht JLabs `queryValue()`. Gibt `None` zurück bei Timeout oder
    /// wenn die Verbindung bereits geschlossen ist (JLab gibt hier 0.0
    /// zurück; wir bevorzugen `Option`, damit "kein Wert" nicht mit dem
    /// validen Messwert 0.0 verwechselt werden kann).
    pub async fn query(&self, key: ChannelKey) -> Option<f64> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(key, tx);

        if self.outgoing.send(Command::Query(key).to_wire()).is_err() {
            self.pending.lock().unwrap().remove(&key);
            return None;
        }

        match tokio::time::timeout(QUERY_TIMEOUT, rx).await {
            Ok(Ok(msg)) => Some(msg.value),
            _ => {
                self.pending.lock().unwrap().remove(&key);
                None
            }
        }
    }

    /// Setzt einen Kanalwert und wartet auf die Quittung des Moduls.
    /// Siehe [`SetOutcome`] und Spec 0003.
    ///
    /// Die Quittung kommt NICHT auf dem gesetzten Kanal zurück, sondern
    /// unaufgefordert auf dem Statuskanal 255 des Moduls (am realen Gerät
    /// verifiziert: `4:0=1234.5!` → `#4:255=0 [OK]`) - deshalb registriert
    /// sich diese Methode in der Pending-Map unter `(Adresse, 255)`,
    /// nicht unter dem Ziel-Key. Konsequenz: Zwei gleichzeitige `set()`s
    /// auf DASSELBE Modul würden sich die Quittung streitig machen
    /// (der zweite `insert` verdrängt den ersten Warteplatz) - für diese
    /// Ausbaustufe akzeptiert, siehe Spec 0003 "außerhalb des Scopes".
    pub async fn set(&self, key: ChannelKey, value: f64) -> SetOutcome {
        let ack_key = ChannelKey {
            address: key.address,
            subchannel: STATUS_SUBCHANNEL,
        };
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(ack_key, tx);

        if self
            .outgoing
            .send(Command::SetFloat(key, value).to_wire())
            .is_err()
        {
            self.pending.lock().unwrap().remove(&ack_key);
            return SetOutcome::NoReply;
        }

        match tokio::time::timeout(QUERY_TIMEOUT, rx).await {
            // Quittungs-Code 0 = angenommen, alles andere ist eine
            // Ablehnung (real beobachtet: 5 [PARERR]). ACHTUNG: Ablehnung
            // heißt nicht "unverändert" - die Firmware klemmt Werte und
            // quittiert trotzdem mit Fehler; den Ist-Zustand liefert nur
            // ein anschließendes `query()`.
            Ok(Ok(ack)) if ack.value == 0.0 => SetOutcome::Confirmed {
                status_text: ack.status_text,
            },
            Ok(Ok(ack)) => SetOutcome::Rejected {
                code: ack.value,
                status_text: ack.status_text,
            },
            _ => {
                self.pending.lock().unwrap().remove(&ack_key);
                SetOutcome::NoReply
            }
        }
    }

    /// Abonniert alle eingehenden Nachrichten, egal ob Antwort auf eine
    /// eigene Query oder unaufgefordert vom c't-Lab gesendet.
    pub fn subscribe(&self) -> broadcast::Receiver<Message> {
        self.updates.subscribe()
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
