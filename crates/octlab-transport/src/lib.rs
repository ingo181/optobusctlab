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
}
