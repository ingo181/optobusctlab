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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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
    pub sent: Vec<String>,
    pub queued_responses: std::collections::VecDeque<RawLine>,
}

impl SimulatedConnection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sent: Vec::new(),
            queued_responses: std::collections::VecDeque::new(),
        }
    }

    /// Legt eine Antwort in die Warteschlange, die beim nächsten `recv_line()` geliefert wird.
    pub fn push_response(&mut self, line: impl Into<String>) {
        self.queued_responses.push_back(line.into());
    }
}

#[async_trait]
impl BoardConnection for SimulatedConnection {
    async fn connect(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    async fn send_line(&mut self, line: &str) -> Result<(), TransportError> {
        self.sent.push(line.to_string());
        Ok(())
    }

    async fn recv_line(&mut self) -> Result<RawLine, TransportError> {
        if let Some(line) = self.queued_responses.pop_front() {
            return Ok(line);
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
        stream.write_all(line.as_bytes()).await?;
        stream.write_all(b"\r\n").await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn simulated_connection_echoes_queued_responses() {
        let mut conn = SimulatedConnection::new("test-channel");
        conn.push_response("#0:0=1.23456");

        conn.send_line("0:VAL 0?").await.unwrap();
        assert_eq!(conn.sent, vec!["0:VAL 0?"]);

        let reply = conn.recv_line().await.unwrap();
        assert_eq!(reply, "#0:0=1.23456");
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
