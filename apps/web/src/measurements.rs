//! Frontend-Zustand für Messwerte: letzter Wert pro Kanal gewinnt.
//!
//! Bewusst reine Funktionen ohne Leptos-/Browser-Bezug, damit sie als
//! normale `cargo test`-Tests auf dem Host laufen (kein WASM-Testrunner
//! nötig). Die reaktive Anbindung (Signal-Update im WebSocket-Callback)
//! passiert in `ws.rs`.

use serde::Deserialize;
use std::collections::HashMap;

/// Kanal-Schlüssel wie im Draht-Protokoll: (Modul-Adresse, Subkanal).
pub type ChannelId = (u8, u8);

/// Spiegelbild des `MeasurementDto`-JSON aus `octlab-server`. Bewusst hier
/// dupliziert statt aus `octlab-server` importiert: das Server-Crate zieht
/// axum/tokio mit, die im WASM-Build nichts verloren haben. Die Feldnamen
/// sind der Vertrag; driftet er, schlägt AK3 (Verwerfen unlesbarer
/// Nachrichten) zu und das Gauge bleibt sichtbar stehen.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Measurement {
    pub address: u8,
    pub subchannel: u8,
    pub value: f64,
    pub status_text: Option<String>,
}

/// Nimmt eine rohe WebSocket-Textnachricht entgegen und trägt sie in den
/// Zustand ein: letzter Wert pro Kanal gewinnt. Unlesbare Nachrichten werden
/// verworfen (`false`), der Zustand bleibt dann unangetastet.
pub fn apply_measurement(state: &mut HashMap<ChannelId, Measurement>, raw: &str) -> bool {
    match serde_json::from_str::<Measurement>(raw) {
        Ok(m) => {
            state.insert((m.address, m.subchannel), m);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(address: u8, subchannel: u8, value: f64) -> String {
        format!(
            r#"{{"address":{address},"subchannel":{subchannel},"value":{value},"status_text":null}}"#
        )
    }

    // AK1: Letzter Wert pro Kanal gewinnt
    #[test]
    fn letzter_wert_pro_kanal_gewinnt() {
        let mut state = HashMap::new();
        assert!(apply_measurement(&mut state, &msg(1, 0, 1.5)));
        assert!(apply_measurement(&mut state, &msg(1, 0, 2.5)));
        assert_eq!(state.len(), 1);
        assert_eq!(state[&(1, 0)].value, 2.5);
    }

    // AK2: Kanäle bleiben getrennt
    #[test]
    fn kanaele_bleiben_getrennt() {
        let mut state = HashMap::new();
        assert!(apply_measurement(&mut state, &msg(1, 0, 1.5)));
        assert!(apply_measurement(&mut state, &msg(2, 3, 7.0)));
        assert_eq!(state.len(), 2);
        assert_eq!(state[&(1, 0)].value, 1.5);
        assert_eq!(state[&(2, 3)].value, 7.0);
    }

    // AK3: Unlesbare Nachricht wird verworfen
    #[test]
    fn unlesbare_nachricht_wird_verworfen() {
        let mut state = HashMap::new();
        assert!(apply_measurement(&mut state, &msg(1, 0, 1.5)));
        let before = state.clone();

        assert!(!apply_measurement(&mut state, "kein json"));
        assert!(!apply_measurement(
            &mut state,
            r#"{"address":"nicht-numerisch"}"#
        ));
        assert_eq!(state, before);
    }

    // Randfall zu AK3: status_text darf gefüllt sein (kommt bei
    // Status-Nachrichten des Labs vor), das ist KEINE unlesbare Nachricht.
    #[test]
    fn status_text_ist_optional_aber_erlaubt() {
        let mut state = HashMap::new();
        let raw = r#"{"address":4,"subchannel":254,"value":3.71,"status_text":"DDS by CM"}"#;
        assert!(apply_measurement(&mut state, raw));
        assert_eq!(state[&(4, 254)].status_text.as_deref(), Some("DDS by CM"));
    }
}
