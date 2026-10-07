//! Verbindungsebene ("Connection Layer").
//!
//! Entspricht `BoardCommunication` in JLab bzw. der "Verbindungsebene" der
//! C#-CtLab-Library: reines Senden/Empfangen von Zeilen (ASCII-Text, durch
//! CR/LF abgeschlossen), ohne Wissen über c't-Lab-Semantik. Die Interpretation
//! der Zeilen passiert eine Ebene höher, in `octlab-protocol`.
//!
//! Vier Implementierungen sind vorgesehen (analog zu JLabs vier
//! `BoardCommunication`-Subklassen):
//! - [`SimulatedConnection`] (hier bereits implementiert, für Tests/CI ohne Hardware)
//! - `TcpConnection` (roher TCP-Socket zum XPort, Port 10001 – kommt als Nächstes)
//! - `SerialConnection` (native serielle Schnittstelle über `tokio-serial`)
//! - ggf. weitere, z.B. für Mock-Szenarien in der UI-Entwicklung

use async_trait::async_trait;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

mod sim_bus;
pub use sim_bus::{ConnectionEvent, IoAttempt, SimBus};

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("Verbindung getrennt")]
    Disconnected,
    #[error("I/O-Fehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("Timeout beim Warten auf Daten")]
    Timeout,
}

/// Eine Zeile Rohdaten wie sie das c't-Lab sendet, z.B. `#0:0=1.23456`.
pub type RawLine = String;

/// Abstraktion einer physikalischen oder simulierten Verbindung zum c't-Lab.
///
/// Bewusst schlank gehalten: nur "sende eine Zeile" und "empfange die nächste
/// Zeile". Framing (CR/LF-Erkennung), Reconnect-Logik etc. lebt in den
/// konkreten Implementierungen, nicht im Trait.
#[async_trait]
pub trait BoardConnection: Send + Sync {
    /// Baut die Verbindung auf (Socket öffnen, seriellen Port öffnen, ...).
    async fn connect(&mut self) -> Result<(), TransportError>;

    /// Trennt die Verbindung sauber.
    async fn disconnect(&mut self) -> Result<(), TransportError>;

    /// Sendet eine einzelne Befehlszeile (ohne CR/LF, wird intern angehängt).
    async fn send_line(&mut self, line: &str) -> Result<(), TransportError>;

    /// Wartet auf die nächste vollständige, empfangene Zeile.
    async fn recv_line(&mut self) -> Result<RawLine, TransportError>;

    /// Menschenlesbarer Name des Kanals (für Logging/UI), z.B. "IFP-USB" oder "XPort-Rack1".
    fn channel_name(&self) -> &str;
}

/// Simulierte Verbindung für Tests und UI-Entwicklung ohne angeschlossene Hardware.
///
/// Entspricht `SimulatedBoardInterface` in JLab. Antworten werden aus einer
/// Warteschlange bedient, die im Test vorbefüllt wird; gesendete Zeilen werden
/// mitgeloggt, damit man in Tests assertieren kann, was tatsächlich gesendet wurde.
pub struct SimulatedConnection {
    name: String,
    /// Sende-Log als `Arc<Mutex<...>>` statt `Vec` direkt: die Connection
    /// wird per `Box<dyn BoardConnection>` in den Lab-Actor MOVED (Ownership
    /// wandert komplett dorthin, der Test behält nichts zurück). Ein Test,
    /// der hinterher prüfen will, was gesendet wurde, braucht deshalb einen
    /// zweiten Besitzer desselben Logs - genau das ist `Arc` (geteilte
    /// Ownership per Referenzzählung), `Mutex` macht die Schreibzugriffe
    /// des Actors und die Lesezugriffe des Tests gegeneinander sicher.
    /// Handle vor dem `Lab::spawn()` per [`Self::sent_handle`] ziehen.
    sent: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    pub queued_responses: std::collections::VecDeque<RawLine>,
    /// Antworten, die erst beim jeweils NÄCHSTEN `send_line()` in die
    /// Empfangs-Warteschlange rücken - im Gegensatz zu `queued_responses`,
    /// die sofort abrufbar sind (unaufgeforderte Nachrichten). Bildet
    /// Request/Response-Sequenzen ab (z.B. Set → Quittung, Query →
    /// Messwert), bei denen die Antwort erst NACH dem zugehörigen Kommando
    /// eintreffen darf.
    scripted_replies: std::collections::VecDeque<RawLine>,
    /// Optionaler simulierter Bus mit "lebenden" Modulen (Spec 0005), siehe
    /// [`SimBus`]. `None` = bisheriges, rein skriptgesteuertes Verhalten.
    bus: Option<SimBus>,
}

impl SimulatedConnection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sent: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            queued_responses: std::collections::VecDeque::new(),
            scripted_replies: std::collections::VecDeque::new(),
            bus: None,
        }
    }

    /// Hängt einen simulierten Bus an. Der Aufrufer behält ein geklontes
    /// [`SimBus`]-Handle, um die Module zur Laufzeit zu steuern.
    pub fn with_bus(mut self, bus: SimBus) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Legt eine Antwort in die Warteschlange, die beim nächsten `recv_line()` geliefert wird.
    pub fn push_response(&mut self, line: impl Into<String>) {
        self.queued_responses.push_back(line.into());
    }

    /// Legt eine Antwort bereit, die erst durch das nächste `send_line()`
    /// "ausgelöst" wird (ein gesendetes Kommando gibt genau eine
    /// Skript-Antwort frei, in FIFO-Reihenfolge).
    pub fn push_reply(&mut self, line: impl Into<String>) {
        self.scripted_replies.push_back(line.into());
    }

    /// Zweites Handle auf das Sende-Log - VOR dem `Lab::spawn()` ziehen,
    /// danach ist die Connection weg (siehe Feld-Kommentar an `sent`).
    pub fn sent_handle(&self) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        self.sent.clone()
    }
}

#[async_trait]
impl BoardConnection for SimulatedConnection {
    /// Mit Bus: Verbindungsaufbau beim Bus (kann abgelehnt werden, startet
    /// sonst eine frische Session - Spec 0008). Ohne Bus: immer erfolgreich.
    async fn connect(&mut self) -> Result<(), TransportError> {
        match &self.bus {
            Some(bus) if !bus.try_connect() => Err(TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "SimBus: Verbindung abgelehnt",
            ))),
            _ => Ok(()),
        }
    }

    async fn disconnect(&mut self) -> Result<(), TransportError> {
        if let Some(bus) = &self.bus {
            bus.on_disconnect();
        }
        Ok(())
    }

    async fn send_line(&mut self, line: &str) -> Result<(), TransportError> {
        // Getrennte Session: nichts kommt beim Bus an, auch nicht im
        // Sende-Log - ein späteres "nachträgliches Senden" wäre sichtbar.
        if let Some(bus) = &self.bus {
            if bus.is_disconnected() {
                bus.note_io_while_disconnected(crate::IoAttempt::Send);
                return Err(TransportError::Disconnected);
            }
        }
        self.sent.lock().unwrap().push(line.to_string());
        if let Some(bus) = &self.bus {
            bus.on_line_sent(line);
        }
        // Ein gesendetes Kommando gibt die nächste Skript-Antwort frei
        // (falls vorhanden) - siehe Doc-Kommentar an `push_reply`.
        if let Some(reply) = self.scripted_replies.pop_front() {
            self.queued_responses.push_back(reply);
        }
        Ok(())
    }

    async fn recv_line(&mut self) -> Result<RawLine, TransportError> {
        // Getrennte Session: sofort und bei JEDEM Aufruf `Disconnected` -
        // so verhält sich `TcpConnection` nach dem Schließen der Gegenseite
        // (Charakterisierungstest unten, Befund zu Spec 0008).
        if let Some(bus) = &self.bus {
            if bus.is_disconnected() {
                bus.note_io_while_disconnected(crate::IoAttempt::Recv);
                return Err(TransportError::Disconnected);
            }
        }
        if let Some(line) = self.queued_responses.pop_front() {
            return Ok(line);
        }
        // Mit angeschlossenem Bus: auf dessen nächste fällige Zeile warten
        // (bleibt ebenfalls pending, solange nichts eingeplant ist) - oder
        // auf den Abbruch der Session.
        if let Some(bus) = &self.bus {
            return match bus.next_incoming_in_session().await {
                Some(line) => Ok(line),
                None => {
                    bus.note_io_while_disconnected(crate::IoAttempt::Recv);
                    Err(TransportError::Disconnected)
                }
            };
        }
        // WICHTIG: Bei leerer Warteschlange NICHT sofort einen Fehler
        // zurückgeben. Ein "instant Err" würde im Lab-Actor (siehe
        // octlab-lab) zu einer Busy-Loop führen, weil `tokio::select!` diesen
        // Zweig bei jeder Poll-Runde sofort wieder als "ready" sieht und nie
        // an den Timer-Task abgibt. Ein echter serieller Port ohne Daten
        // blockiert ebenfalls, bis etwas ankommt – wir bilden das nach.
        std::future::pending::<()>().await;
        unreachable!("pending() löst nie auf")
    }

    fn channel_name(&self) -> &str {
        &self.name
    }
}

/// TCP-Verbindung zum XPort (roher Socket, "raw"-Modus, Port 10001).
/// Siehe `specs/0001-tcp-connection.md` für den vollständigen Vertrag.
pub struct TcpConnection {
    addr: String,
    stream: Option<TcpStream>,
    /// Bytes, die schon vom Socket gelesen, aber noch nicht zu einer
    /// vollständigen Zeile (bis `\n`) zusammengesetzt wurden. Muss ein Feld
    /// sein, kein lokales `let` in `recv_line()` - ein `read()` liefert
    /// beliebige Bruchstücke (zu wenig für eine ganze Zeile, oder mehr als
    /// eine Zeile auf einmal), und der Rest muss den Aufruf überleben, bis
    /// der nächste `recv_line()`-Aufruf ihn weiterverarbeitet.
    buffer: Vec<u8>,
}

impl TcpConnection {
    pub fn new(addr: impl Into<String>) -> Self {
        Self {
            addr: addr.into(),
            stream: None,
            buffer: Vec::new(),
        }
    }
}

#[async_trait]
impl BoardConnection for TcpConnection {
    async fn connect(&mut self) -> Result<(), TransportError> {
        self.stream = Some(TcpStream::connect(&self.addr).await?);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), TransportError> {
        self.stream = None;
        Ok(())
    }

    async fn send_line(&mut self, line: &str) -> Result<(), TransportError> {
        let stream = self.stream.as_mut().ok_or(TransportError::Disconnected)?;
        write_frame(stream, line).await?;
        Ok(())
    }

    async fn recv_line(&mut self) -> Result<RawLine, TransportError> {
        loop {
            if let Some(newline_pos) = self.buffer.iter().position(|&b| b == b'\n') {
                let mut line_bytes: Vec<u8> = self.buffer.drain(..=newline_pos).collect();
                line_bytes.pop(); // '\n'
                if line_bytes.last() == Some(&b'\r') {
                    line_bytes.pop();
                }
                return String::from_utf8(line_bytes).map_err(|err| {
                    TransportError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
                });
            }

            let stream = self.stream.as_mut().ok_or(TransportError::Disconnected)?;
            let mut chunk = [0u8; 1024];
            let bytes_read = stream.read(&mut chunk).await?;
            if bytes_read == 0 {
                return Err(TransportError::Disconnected);
            }
            self.buffer.extend_from_slice(&chunk[..bytes_read]);
        }
    }

    fn channel_name(&self) -> &str {
        &self.addr
    }
}

/// Schreibt eine Zeile samt CR/LF-Abschluss auf `writer` - in EINEM Write.
///
/// Zwei getrennte kleine Writes (erst die Zeile, dann `\r\n`) lösen am
/// TCP-Socket den Nagle-Algorithmus aus: Das zweite Segment wartet auf die
/// Bestätigung des ersten, die Gegenseite bestätigt verzögert. Gegen
/// `fake_xport` gemessen kostete das ~41 ms pro Abfrage statt <1 ms (Spec
/// 0005, Messpunkt 14). Bewusst KEIN `TCP_NODELAY`: Ein vollständiger Frame
/// pro Write löst das Problem an der Ursache.
///
/// Generisch über `AsyncWrite` statt fest auf `TcpStream`, damit sich im
/// Test zählen lässt, wie viele einzelne Writes dabei entstehen - an einem
/// echten Socket ist das von außen nicht verlässlich sichtbar (der Kernel
/// darf kleine Segmente zusammenfassen oder teilen).
async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, line: &str) -> std::io::Result<()> {
    let frame = format!("{line}\r\n");
    writer.write_all(frame.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-Writer: nimmt jeden Puffer vollständig an und protokolliert
    /// jeden einzelnen Write für sich - so wird sichtbar, in wie viele
    /// Writes ein Frame zerfällt.
    #[derive(Default)]
    struct CountingWriter {
        writes: Vec<Vec<u8>>,
    }

    impl AsyncWrite for CountingWriter {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            self.writes.push(buf.to_vec());
            std::task::Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    /// Zeile und CR/LF gehen in EINEM Write raus. Zwei getrennte kleine
    /// Writes lösten gegen `fake_xport` Nagle-Algorithmus + verzögerte
    /// Bestätigung aus (~41 ms pro Abfrage statt <1 ms, Spec 0005,
    /// Messpunkt 14).
    #[tokio::test]
    async fn frame_geht_in_genau_einem_write_raus() {
        let mut writer = CountingWriter::default();

        write_frame(&mut writer, "0:IDN?").await.unwrap();

        pretty_assert_eq!(writer.writes, vec![b"0:IDN?\r\n".to_vec()]);
    }

    #[tokio::test]
    async fn simulated_connection_echoes_queued_responses() {
        let mut conn = SimulatedConnection::new("test-channel");
        conn.push_response("#0:0=1.23456");

        conn.send_line("0:VAL 0?").await.unwrap();
        assert_eq!(*conn.sent_handle().lock().unwrap(), vec!["0:VAL 0?"]);

        let reply = conn.recv_line().await.unwrap();
        assert_eq!(reply, "#0:0=1.23456");
    }

    #[tokio::test]
    async fn scripted_reply_wird_erst_durch_send_freigegeben() {
        let mut conn = SimulatedConnection::new("test-channel");
        conn.push_reply("#4:255=0 [OK]");
        conn.push_reply("#4:0=2500.0");

        // Vor dem ersten Send darf recv_line() NICHT liefern (pending) -
        // prüfbar über ein kurzes Timeout.
        let pending =
            tokio::time::timeout(std::time::Duration::from_millis(20), conn.recv_line()).await;
        assert!(pending.is_err(), "Antwort kam vor dem zugehörigen Kommando");

        conn.send_line("4:0=2500!").await.unwrap();
        assert_eq!(conn.recv_line().await.unwrap(), "#4:255=0 [OK]");

        conn.send_line("4:0?").await.unwrap();
        assert_eq!(conn.recv_line().await.unwrap(), "#4:0=2500.0");
    }

    // TcpConnection: siehe specs/0001-tcp-connection.md AK1-AK6. Jeder Test
    // spannt einen echten Loopback-TCP-Server auf (127.0.0.1:0 -> OS wählt
    // einen freien Port), kein Mock - AK5/AK6 prüfen genau das Verhalten
    // von TcpConnection beim Lesen vom echten Socket, das ein Mock des
    // BoardConnection-Traits gar nicht sehen würde.
    use pretty_assertions::assert_eq as pretty_assert_eq;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn ak1_connect_succeeds_when_peer_accepts() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let mut conn = TcpConnection::new(addr);
        conn.connect().await.unwrap();
    }

    #[tokio::test]
    async fn ak2_connect_fails_without_blocking_when_nobody_listens() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        drop(listener); // Port wieder frei, aber niemand nimmt Verbindungen an

        let mut conn = TcpConnection::new(addr);
        let result = conn.connect().await;

        assert!(
            matches!(result, Err(TransportError::Io(_))),
            "erwartete TransportError::Io, bekam {result:?}"
        );
    }

    /// Charakterisierung (Befund zu Spec 0008, kein neues Verhalten): Nach
    /// dem Schließen der Gegenseite liefert `recv_line()` NICHT pending,
    /// sondern sofort und bei jedem weiteren Aufruf `Disconnected`. Genau
    /// das ließ den Lab-Actor in eine Dauerschleife laufen. Gezählt werden
    /// Rückgaben, nicht Zeiten; das `timeout` schützt nur gegen Hängen.
    #[tokio::test]
    async fn nach_schliessen_der_gegenseite_liefert_recv_line_wiederholt_disconnected() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream); // Gegenseite schließt sofort
        });

        let mut conn = TcpConnection::new(addr);
        conn.connect().await.unwrap();
        peer.await.unwrap();

        let results = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut results = Vec::new();
            for _ in 0..5 {
                results.push(conn.recv_line().await);
            }
            results
        })
        .await
        .expect("recv_line() blieb hängen statt Disconnected zu liefern");

        assert_eq!(results.len(), 5);
        for result in &results {
            assert!(
                matches!(result, Err(TransportError::Disconnected)),
                "erwartet Disconnected, kam: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn ak3_send_line_appends_crlf() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let received = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 64];
            let n = stream.read(&mut buf).await.unwrap();
            buf[..n].to_vec()
        });

        let mut conn = TcpConnection::new(addr);
        conn.connect().await.unwrap();
        conn.send_line("0:IDN?").await.unwrap();

        let received = received.await.unwrap();
        pretty_assert_eq!(received, b"0:IDN?\r\n".to_vec());
    }

    #[tokio::test]
    async fn ak4_recv_line_returns_complete_line_without_terminator() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            stream
                .write_all(
                    "#0:254=1.742 [ADA by CM/c't 04/2007; DA12 AD16 IO32 LCD ]\r\n".as_bytes(),
                )
                .await
                .unwrap();
        });

        let mut conn = TcpConnection::new(addr);
        conn.connect().await.unwrap();
        let line = conn.recv_line().await.unwrap();

        pretty_assert_eq!(
            line,
            "#0:254=1.742 [ADA by CM/c't 04/2007; DA12 AD16 IO32 LCD ]"
        );
    }

    #[tokio::test]
    async fn ak5_recv_line_reassembles_ascii_split_across_two_writes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            // Bewusst mitten in "c't" geschnitten (reales ASCII, siehe Spec-
            // Begründung: kein Multibyte-Zeichen, aber ein echter,
            // beobachteter TCP-Fragmentierungsfall).
            stream
                .write_all("#0:254=1.742 [ADA by CM/c".as_bytes())
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            stream
                .write_all("'t 04/2007; DA12 AD16 IO32 LCD ]\r\n".as_bytes())
                .await
                .unwrap();
        });

        let mut conn = TcpConnection::new(addr);
        conn.connect().await.unwrap();
        let line = conn.recv_line().await.unwrap();

        pretty_assert_eq!(
            line,
            "#0:254=1.742 [ADA by CM/c't 04/2007; DA12 AD16 IO32 LCD ]"
        );
    }

    #[tokio::test]
    async fn ak6_recv_line_reassembles_multibyte_char_split_across_two_writes() {
        let full_line = "#2:1=23.5 [Temperatur 23.5°C]\r\n";
        let bytes = full_line.as_bytes();
        // "°" (Grad-Zeichen, U+00B0) ist in UTF-8 zwei Bytes: 0xC2 0xB0.
        // Schneide exakt zwischen den beiden - das ist der eigentliche Zweck
        // dieses Tests (siehe AK6-Begründung in der Spec).
        let split_at = bytes.iter().position(|&b| b == 0xC2).unwrap() + 1;
        let (first_chunk, second_chunk) = bytes.split_at(split_at);
        assert_eq!(
            first_chunk.last(),
            Some(&0xC2),
            "Testaufbau kaputt: Split trifft nicht das erste Byte von '°'"
        );
        assert_eq!(
            second_chunk.first(),
            Some(&0xB0),
            "Testaufbau kaputt: Split trifft nicht das zweite Byte von '°'"
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let first_chunk = first_chunk.to_vec();
        let second_chunk = second_chunk.to_vec();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            stream.write_all(&first_chunk).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            stream.write_all(&second_chunk).await.unwrap();
        });

        let mut conn = TcpConnection::new(addr);
        conn.connect().await.unwrap();
        let line = conn.recv_line().await.unwrap();

        pretty_assert_eq!(line, "#2:1=23.5 [Temperatur 23.5°C]");
    }
}
