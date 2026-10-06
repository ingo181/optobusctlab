//! Statische Kanalliste der Mehrkanal-Ansicht (Spec 0005) - Server-Konfiguration,
//! bewusst KEIN ESDM-Read-Model und keine dynamische Erkennung.
//!
//! ACHTUNG Verifikationsstand: Belegung, Namen und Einheiten stammen aus der
//! c't-Lab-Syntax-Doku von 2010 (siehe Spec 0005, "Kanalmodell") und sind -
//! mit Ausnahme der DDS-Frequenz (4:0, Spec 0003) - UNVERIFIZIERT
//! (Hardware-Messung offen, Messpunkte am Ende der Spec). Die Einheit von
//! DIV 1:0 hängt vom Messbereich (RNG, 1:19) ab und ist deshalb nicht
//! statisch angebbar.

use octlab_lab::PollConfig;
use octlab_protocol::{ChannelKey, ModuleAddress, SubChannel};
use std::time::Duration;

/// Ein Eintrag der Kanalliste.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelSpec {
    pub address: u8,
    pub subchannel: u8,
    pub name: &'static str,
    pub unit: &'static str,
}

const fn ch(address: u8, subchannel: u8, name: &'static str, unit: &'static str) -> ChannelSpec {
    ChannelSpec {
        address,
        subchannel,
        name,
        unit,
    }
}

/// Die 12 Kanäle aus Spec 0005, Abschnitt "Kanalliste für diese Spec".
pub const CHANNELS: [ChannelSpec; 12] = [
    ch(0, 10, "ADA AD16 Kanal 0", "V"),
    ch(0, 11, "ADA AD16 Kanal 1", "V"),
    ch(0, 12, "ADA AD16 Kanal 2", "V"),
    ch(0, 13, "ADA AD16 Kanal 3", "V"),
    ch(1, 0, "DIV Messwert", "abhängig von RNG"),
    ch(1, 19, "DIV Messbereich", "Code"),
    ch(2, 10, "DCG U Ist", "V"),
    ch(2, 11, "DCG I Ist", "A"),
    ch(2, 0, "DCG U Soll", "V"),
    ch(2, 1, "DCG I Soll", "A"),
    ch(4, 0, "DDS Frequenz", "Hz"),
    ch(4, 1, "DDS Pegel", "mVeff"),
];

impl ChannelSpec {
    pub fn key(&self) -> ChannelKey {
        ChannelKey {
            address: ModuleAddress(self.address),
            subchannel: SubChannel(self.subchannel),
        }
    }
}

/// Poll-Konfiguration über die Kanalliste mit dem angegebenen Intervall
/// (0 = Polling aus).
pub fn poll_config(interval: Duration) -> PollConfig {
    PollConfig {
        channels: CHANNELS.iter().map(ChannelSpec::key).collect(),
        interval,
    }
}
