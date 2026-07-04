//! Kommando-/Nachrichtenebene ("Command & Message Layer").
//!
//! Setzt auf `octlab-transport` auf und kapselt die c't-Lab-ASCII-Syntax aus
//! den Original-Artikeln, z.B.:
//!   - Abfrage:  `0:VAL 0?`      -> Antwort `#0:0=1.23456`
//!   - Befehl:   `2:DCV=15.0!`   -> Antwort `#2:255=0 [OK]`
//!   - Fehler:   `#0:255=6 [LOCKED]`
//!
//! Bewusste Design-Entscheidung gegenüber dem Original-Protokoll: Die
//! "sparsame" Schreibweise (Adresse nur beim ersten Befehl je Modul angeben)
//! war 2007 eine Bandbreiten-Optimierung für 38400 Bit/s auf einem seriellen
//! Bus mit genau einem Client. Bei uns senden potenziell mehrere async Tasks
//! gleichzeitig über denselben Kanal (siehe `octlab-lab`) – da wäre sticky
//! addressing eine Race-Condition-Quelle. Wir senden deshalb IMMER die volle
//! Adresse, auch wenn das ein paar Bytes mehr sind.

use std::num::{ParseFloatError, ParseIntError};
use thiserror::Error;

/// Modul-Adresse (0..7 im Original-Protokoll, per Jumper am Modul eingestellt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleAddress(pub u8);

/// Subkanal-Nummer (0..255), z.B. VAL-Kanal, D/A-Kanal oder Statuskanal 255.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubChannel(pub u8);

/// Eindeutiger Schlüssel für einen Messwert/Parameter im ganzen c't-Lab-System.
/// Entspricht dem Zweidimensional-Array-Index (Moduladresse × Subkanal), das
/// sowohl JLab als auch die LabVIEW-Demos zur Ablage der Messwerte nutzen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelKey {
    pub address: ModuleAddress,
    pub subchannel: SubChannel,
}

/// Der Statuskanal, an den Module ihre OK/Fehler-Meldungen schicken (siehe c't-Artikel).
pub const STATUS_SUBCHANNEL: SubChannel = SubChannel(255);

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Fragt einen Messwert oder Parameter ab, z.B. `0:VAL 0?`.
    Query(ChannelKey),
    /// Setzt einen Fließkomma-Parameter, z.B. `2:DCV=15.0!`.
    SetFloat(ChannelKey, f64),
}

impl Command {
    /// Serialisiert den Befehl in die Draht-Syntax (ohne CR/LF, das übernimmt
    /// die Transport-Ebene beim tatsächlichen Senden).
    pub fn to_wire(&self) -> String {
        match self {
            Command::Query(key) => format!("{}:{}?", key.address.0, key.subchannel.0),
            Command::SetFloat(key, value) => {
                format!("{}:{}={value}!", key.address.0, key.subchannel.0)
            }
        }
    }
}

/// Eine vom c't-Lab empfangene Nachricht, z.B. geparst aus `#0:0=1.23456`
/// oder `#0:255=6 [LOCKED]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub key: ChannelKey,
    pub value: f64,
    /// Klartext in eckigen Klammern, falls vorhanden (z.B. "OK", "LOCKED",
    /// oder bei IDN-Antworten der Modulname samt Firmware-Version).
    pub status_text: Option<String>,
}

#[derive(Debug, Error, PartialEq)]
pub enum ProtocolError {
    #[error("Zeile ist keine gültige c't-Lab-Nachricht: {0:?}")]
    Malformed(String),
    #[error("Adresse/Subkanal nicht parsbar: {0}")]
    InvalidNumber(#[from] ParseIntError),
    #[error("Messwert nicht parsbar: {0}")]
    InvalidFloat(#[from] ParseFloatError),
}

/// Parst eine einzelne, bereits von CR/LF befreite Zeile in eine [`Message`].
pub fn parse_message(line: &str) -> Result<Message, ProtocolError> {
    let line = line.trim();
    let rest = line
        .strip_prefix('#')
        .ok_or_else(|| ProtocolError::Malformed(line.to_string()))?;

    let (main, status_text) = match rest.find(" [") {
        Some(idx) => {
            let text = rest[idx + 2..].trim_end_matches(']').to_string();
            (&rest[..idx], Some(text))
        }
        None => (rest, None),
    };

    let (addr_sub, value_str) = main
        .split_once('=')
        .ok_or_else(|| ProtocolError::Malformed(line.to_string()))?;
    let (addr_str, sub_str) = addr_sub
        .split_once(':')
        .ok_or_else(|| ProtocolError::Malformed(line.to_string()))?;

    let address = addr_str.parse::<u8>()?;
    let subchannel = sub_str.parse::<u8>()?;
    let value = value_str.parse::<f64>()?;

    Ok(Message {
        key: ChannelKey {
            address: ModuleAddress(address),
            subchannel: SubChannel(subchannel),
        },
        value,
        status_text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_query_command() {
        let key = ChannelKey {
            address: ModuleAddress(0),
            subchannel: SubChannel(0),
        };
        assert_eq!(Command::Query(key).to_wire(), "0:0?");
    }

    #[test]
    fn builds_set_command() {
        let key = ChannelKey {
            address: ModuleAddress(2),
            subchannel: SubChannel(0),
        };
        assert_eq!(Command::SetFloat(key, 15.0).to_wire(), "2:0=15!");
    }

    #[test]
    fn parses_plain_measurement() {
        // Beispiel aus dem c't-Artikel: DIV-Modul liefert einen Messwert
        let msg = parse_message("#3:0=1.23456").unwrap();
        assert_eq!(msg.key.address, ModuleAddress(3));
        assert_eq!(msg.key.subchannel, SubChannel(0));
        assert!((msg.value - 1.23456).abs() < f64::EPSILON);
        assert_eq!(msg.status_text, None);
    }

    #[test]
    fn parses_status_with_bracket_text() {
        // Beispiel: Schreibversuch auf gesperrten EEPROM-Bereich
        let msg = parse_message("#0:255=6 [LOCKED]").unwrap();
        assert_eq!(msg.key.subchannel, STATUS_SUBCHANNEL);
        assert_eq!(msg.value, 6.0);
        assert_eq!(msg.status_text.as_deref(), Some("LOCKED"));
    }

    #[test]
    fn parses_idn_reply() {
        // Beispiel: ADA-IO-Identifikationsantwort auf "0:IDN?"
        let msg = parse_message("#0:254=1.28 [ADA by CM/c't 04/2007; Modules: IO]").unwrap();
        assert_eq!(msg.value, 1.28);
        assert_eq!(
            msg.status_text.as_deref(),
            Some("ADA by CM/c't 04/2007; Modules: IO")
        );
    }

    #[test]
    fn rejects_malformed_line() {
        assert!(parse_message("garbage").is_err());
    }
}
