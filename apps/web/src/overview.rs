//! Anzeige-Logik der Kanalübersicht (Spec 0005) - reine Funktionen ohne
//! Leptos-/Browser-Bezug, damit sie als Host-Tests laufen. Die Komponente
//! selbst steht in `app.rs`.

use crate::measurements::Measurement;
use serde::Deserialize;

/// Eintrag der Kanalliste von `GET /api/channels`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChannelInfo {
    pub address: u8,
    pub subchannel: u8,
    pub name: String,
    pub unit: String,
}

/// Anzeigetext für den Wert eines Kanals:
/// - noch keine Nachricht empfangen → `–` (keine Aussage über die Anlage)
/// - Kanal hat nie geantwortet → `kein Wert` (AK5: NICHT `0`)
/// - sonst die Zahl, so kurz wie verlustfrei möglich (`Display` von `f64`)
pub fn value_text(measurement: Option<&Measurement>) -> String {
    match measurement {
        None => "–".to_string(),
        Some(Measurement { value: None, .. }) => "kein Wert".to_string(),
        Some(Measurement {
            value: Some(value), ..
        }) => format!("{value}"),
    }
}

/// Kanal als veraltet markieren? Nur wenn der Server das sagt.
pub fn is_stale(measurement: Option<&Measurement>) -> bool {
    measurement.is_some_and(|m| m.stale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measurements::apply_measurement;
    use std::collections::HashMap;

    /// Nachrichten exakt im `/ws`-Format von Spec 0005 (siehe
    /// `crates/octlab-server/tests/ws_channels.rs`).
    fn msg(address: u8, subchannel: u8, value: &str, stale: bool) -> String {
        format!(
            r#"{{"address":{address},"subchannel":{subchannel},"value":{value},"status_text":null,"stale":{stale}}}"#
        )
    }

    // AK5: Kanal hat noch nie geantwortet -> "kein Wert", nicht 0
    #[test]
    fn nie_geantwortet_zeigt_kein_wert_und_veraltet() {
        let mut state = HashMap::new();
        assert!(apply_measurement(&mut state, &msg(2, 10, "null", true)));
        assert_eq!(value_text(state.get(&(2, 10))), "kein Wert");
        assert!(is_stale(state.get(&(2, 10))));
    }

    // AK5: veralteter Wert bleibt sichtbar und ist markiert
    #[test]
    fn veralteter_wert_bleibt_sichtbar_und_ist_markiert() {
        let mut state = HashMap::new();
        assert!(apply_measurement(&mut state, &msg(2, 10, "12.0", false)));
        assert!(apply_measurement(&mut state, &msg(2, 10, "12.0", true)));
        assert_eq!(value_text(state.get(&(2, 10))), "12");
        assert!(is_stale(state.get(&(2, 10))));
    }

    // Aktueller Wert: Zahl, keine Markierung
    #[test]
    fn aktueller_wert_wird_angezeigt() {
        let mut state = HashMap::new();
        assert!(apply_measurement(&mut state, &msg(1, 0, "0.0022", false)));
        assert_eq!(value_text(state.get(&(1, 0))), "0.0022");
        assert!(!is_stale(state.get(&(1, 0))));
    }

    // Noch keine Nachricht für den Kanal: Platzhalter, nicht "kein Wert"
    // (das wäre eine Aussage über die Anlage, die noch nicht vorliegt).
    #[test]
    fn ohne_nachricht_platzhalter() {
        assert_eq!(value_text(None), "–");
        assert!(!is_stale(None));
    }

    // Nachricht im alten 0002-Format (ohne `stale`) gilt als aktuell.
    #[test]
    fn nachricht_ohne_stale_feld_gilt_als_aktuell() {
        let mut state = HashMap::new();
        let raw = r#"{"address":1,"subchannel":0,"value":1.5,"status_text":null}"#;
        assert!(apply_measurement(&mut state, raw));
        assert!(!is_stale(state.get(&(1, 0))));
        assert_eq!(value_text(state.get(&(1, 0))), "1.5");
    }

    #[test]
    fn kanalliste_aus_api_wird_gelesen() {
        let raw = r#"[{"address":4,"subchannel":0,"name":"DDS Frequenz","unit":"Hz"}]"#;
        let channels: Vec<ChannelInfo> = serde_json::from_str(raw).unwrap();
        assert_eq!(
            channels,
            vec![ChannelInfo {
                address: 4,
                subchannel: 0,
                name: "DDS Frequenz".to_string(),
                unit: "Hz".to_string(),
            }]
        );
    }
}
