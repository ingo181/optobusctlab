//! Geräteebene ("Device Layer").
//!
//! Übersetzt die generischen `Command`/`Message`-Typen aus `octlab-protocol`
//! in typisierte, gerätespezifische Methoden – analog zu JLabs `boards`-Paket
//! bzw. der "Geräteebene" der C#-CtLab-Library (dort z.B.
//! `signalGenerator.DdsGenerators[0].Frequency = 1000`).
//!
//! Neue Module (DCG, DIV, ADA-IO, ...) folgen demselben Muster wie [`Dds`]
//! hier: ein Struct mit der Moduladresse, private Subkanal-Konstanten (siehe
//! c't-Lab-Syntax-Doku im Community-Forum, https://ctlabforum.thoralt.de,
//! und/oder https://www.sn7400.de/ctlab/ – `www.ct-lab.de` selbst ist tot),
//! und Methoden, die `Command`- bzw. `ChannelKey`-Werte für die Query-Ebene
//! in `octlab-lab` produzieren.
//!
//! HINWEIS Verifikationsstand (Spec 0003, 2026-07-19, real gegen FW 3.71):
//! FREQUENCY=0 ist am echten DDS verifiziert (lesen, setzen mit Quittung
//! auf Subkanal 255, rücklesen - `cargo run --example dds_probe -p
//! octlab-transport`). LEVEL=1 stammt weiterhin nur aus dem c't-Artikel
//! "DDS-Funktionsgenerator-Modul" und ist VOR dem ersten Schreibzugriff
//! gegen die aktuelle Syntax-Doku (siehe oben) bzw. am Gerät zu
//! verifizieren - Firmware-Updates haben Subkanäle gelegentlich verschoben.

use octlab_protocol::{ChannelKey, Command, ModuleAddress, SubChannel};

/// Gemeinsames Verhalten aller c't-Lab-Module.
pub trait Module {
    fn address(&self) -> ModuleAddress;

    /// Baut den Identifikationsbefehl `<addr>:IDN?` (Subkanal 254 lt. Artikel).
    fn identify(&self) -> Command {
        Command::Query(ChannelKey {
            address: self.address(),
            subchannel: SubChannel(254),
        })
    }
}

/// DDS-Funktionsgenerator-Modul.
pub struct Dds {
    pub address: ModuleAddress,
}

impl Dds {
    const FREQUENCY: SubChannel = SubChannel(0);
    const LEVEL: SubChannel = SubChannel(1);

    fn key(&self, subchannel: SubChannel) -> ChannelKey {
        ChannelKey {
            address: self.address,
            subchannel,
        }
    }

    /// Baut den Befehl zum Setzen der Frequenz in Hz, z.B. `4:0=1000!`.
    pub fn set_frequency_hz(&self, hz: f64) -> Command {
        Command::SetFloat(self.key(Self::FREQUENCY), hz)
    }

    /// Baut die Abfrage der aktuell eingestellten Frequenz.
    pub fn query_frequency(&self) -> Command {
        Command::Query(self.key(Self::FREQUENCY))
    }

    /// Baut den Befehl zum Setzen des Ausgangspegels in mV RMS.
    pub fn set_level_mv(&self, mv: f64) -> Command {
        Command::SetFloat(self.key(Self::LEVEL), mv)
    }
}

impl Module for Dds {
    fn address(&self) -> ModuleAddress {
        self.address
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dds_set_frequency_matches_wire_format_from_ct_article() {
        let dds = Dds {
            address: ModuleAddress(4),
        };
        // Aus dem Artikel: "4:FRQ=1000.0!" ist äquivalent zu "4:0=1000.0!"
        assert_eq!(dds.set_frequency_hz(1000.0).to_wire(), "4:0=1000!");
    }
}
