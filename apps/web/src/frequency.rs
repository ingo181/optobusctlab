//! Logik des Frequenz-Bedienfelds (Spec 0003, AK6-AK8): Eingabe-Parsen und
//! Auswertung der Server-Antwort auf einen Setz-Request.
//!
//! Wie `measurements.rs` bewusst reine Funktionen ohne Leptos-/Browser-Bezug,
//! damit sie als normale `cargo test`-Tests auf dem Host laufen. Die
//! reaktive Anbindung (Signal-Updates, fetch) passiert in `app.rs`/`api.rs`.

use serde::Deserialize;

/// Toleranz für den Vergleich Wunschwert ↔ zurückgelesener Wert, absolut
/// in Hz. Herleitung in Spec 0003: Die DDS-Firmware liefert das Rücklesen
/// mit einer Nachkommastelle (real gemessen, z.B. 1234.5678 → 1234.5), und
/// die Phasenakkumulator-Quantisierung ist eine konstante ABSOLUTE
/// Schrittweite - die Abweichung wächst also nicht mit der Frequenz, eine
/// relative Toleranz wäre die falsche Form.
pub const FREQ_TOLERANCE_HZ: f64 = 0.1;

/// Spiegelbild des JSON aus `POST /api/channel/{addr}/{sub}` - wie
/// `Measurement` bewusst hier dupliziert statt aus `octlab-server`
/// importiert (das Server-Crate zieht axum/tokio mit, die im WASM-Build
/// nichts verloren haben).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SetChannelResponse {
    /// Quittungs-Statustext der Anlage (z.B. "OK", "PARERR"), falls eine
    /// Quittung kam.
    pub ack: Option<String>,
    /// Zurückgelesener Ist-Wert - wegen des Klemm-Verhaltens der Firmware
    /// auch im Ablehnungsfall gefüllt, wenn das Rücklesen klappte.
    pub value: Option<f64>,
    /// Fehlerbeschreibung des Servers (z.B. Timeout), falls vorhanden.
    pub error: Option<String>,
}

/// Anzeige-Zustand des Bedienfelds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FrequencyPanel {
    /// Zuletzt per Rücklesen bestätigte Frequenz - NIE der Wunschwert.
    pub confirmed_hz: Option<f64>,
    /// Hinweis für den Bediener (Ablehnung, Timeout, Abweichung).
    /// `None` = letzter Setz-Vorgang war unauffällig.
    pub notice: Option<String>,
}

impl FrequencyPanel {
    /// Für Fälle, in denen gar kein HTTP-Austausch zustande kam
    /// (Netzwerkfehler im Browser) oder die Eingabe ungültig war (AK8).
    pub fn note_failure(&mut self, message: &str) {
        self.notice = Some(message.to_string());
    }
}

/// Parst die Bediener-Eingabe. `None` heißt: keine Zahl, es darf KEIN
/// Request gesendet werden (AK8).
pub fn parse_frequency_input(raw: &str) -> Option<f64> {
    // `parse::<f64>` akzeptiert auch "NaN"/"inf" - als Frequenz ist beides
    // Unsinn, deshalb der zusätzliche `is_finite`-Filter.
    raw.trim().parse::<f64>().ok().filter(|hz| hz.is_finite())
}

/// Wertet die Antwort auf einen Setz-Request aus und aktualisiert den
/// Anzeige-Zustand (AK6/AK7). `http_ok` = HTTP-Status war 2xx,
/// `raw_body` = Antwort-Body als Text.
pub fn apply_set_response(
    panel: &mut FrequencyPanel,
    requested_hz: f64,
    http_ok: bool,
    raw_body: &str,
) {
    let response: SetChannelResponse = match serde_json::from_str(raw_body) {
        Ok(response) => response,
        Err(_) => {
            panel.notice = Some("Unlesbare Server-Antwort".to_string());
            return;
        }
    };

    // Grundregel der Spec: Nur ein zurückgelesener Ist-Wert darf die
    // bestätigte Frequenz werden - der Wunschwert nie. Das gilt auch im
    // Ablehnungsfall (Klemmung: PARERR + trotzdem veränderter Wert).
    if let Some(actual) = response.value {
        panel.confirmed_hz = Some(actual);
    }

    if let Some(error) = response.error {
        panel.notice = Some(error);
        return;
    }

    if !http_ok {
        panel.notice = Some(format!(
            "Anlage hat abgelehnt: {}",
            response.ack.as_deref().unwrap_or("unbekannter Status")
        ));
        return;
    }

    match response.value {
        Some(actual) if (actual - requested_hz).abs() <= FREQ_TOLERANCE_HZ => {
            panel.notice = None; // bestätigt, Abweichung nur Quantisierung
        }
        Some(actual) => {
            panel.notice = Some(format!(
                "Anlage hat {actual} Hz eingestellt (gewünscht: {requested_hz} Hz)"
            ));
        }
        None => {
            panel.notice = Some("Antwort ohne Rücklesewert".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // AK8: Unsinnige Eingabe verlässt den Browser nicht
    #[test]
    fn eingabe_parsen_akzeptiert_nur_zahlen() {
        assert_eq!(parse_frequency_input("2500"), Some(2500.0));
        assert_eq!(parse_frequency_input(" 1234.5 "), Some(1234.5));
        assert_eq!(parse_frequency_input("abc"), None);
        assert_eq!(parse_frequency_input(""), None);
        // f64::parse würde "NaN"/"inf" akzeptieren - als Frequenz-Eingabe
        // ist beides Unsinn und darf den Browser nicht verlassen.
        assert_eq!(parse_frequency_input("NaN"), None);
        assert_eq!(parse_frequency_input("inf"), None);
    }

    // AK6: bestätigte Frequenz aus dem Rücklesen, Abweichung innerhalb der
    // Toleranz ist kein Fehler (real gemessen: 1234.5678 -> 1234.5)
    #[test]
    fn ok_innerhalb_toleranz_bestaetigt_ohne_warnung() {
        let mut panel = FrequencyPanel::default();
        apply_set_response(
            &mut panel,
            1234.5678,
            true,
            r#"{"ack":"OK","value":1234.5,"error":null}"#,
        );
        assert_eq!(panel.confirmed_hz, Some(1234.5));
        assert_eq!(panel.notice, None);
    }

    // AK7: Ablehnung mit Ist-Wert (Klemmung) - Ist-Wert wird übernommen,
    // Statustext angezeigt
    #[test]
    fn ablehnung_uebernimmt_ist_wert_und_zeigt_statustext() {
        let mut panel = FrequencyPanel {
            confirmed_hz: Some(1000.0),
            notice: None,
        };
        apply_set_response(
            &mut panel,
            2000000.0,
            false,
            r#"{"ack":"PARERR","value":999999.8,"error":null}"#,
        );
        assert_eq!(panel.confirmed_hz, Some(999999.8));
        let notice = panel.notice.expect("Ablehnung muss einen Hinweis zeigen");
        assert!(notice.contains("PARERR"), "Hinweis war: {notice}");
    }

    // AK7: keine Antwort - bestätigte Frequenz bleibt stehen
    #[test]
    fn keine_antwort_laesst_bestaetigte_frequenz_stehen() {
        let mut panel = FrequencyPanel {
            confirmed_hz: Some(1000.0),
            notice: None,
        };
        apply_set_response(
            &mut panel,
            2500.0,
            false,
            r#"{"ack":null,"value":null,"error":"keine Antwort vom Modul (Timeout)"}"#,
        );
        assert_eq!(panel.confirmed_hz, Some(1000.0));
        let notice = panel.notice.expect("Timeout muss einen Hinweis zeigen");
        assert!(notice.contains("keine Antwort"), "Hinweis war: {notice}");
    }

    // AK7 (defensiv): 2xx, aber Rücklesen weicht über die Toleranz ab -
    // Ist-Wert übernehmen UND Abweichung anzeigen
    #[test]
    fn abweichung_ueber_toleranz_wird_angezeigt() {
        let mut panel = FrequencyPanel::default();
        apply_set_response(
            &mut panel,
            2500.0,
            true,
            r#"{"ack":"OK","value":2400.0,"error":null}"#,
        );
        assert_eq!(panel.confirmed_hz, Some(2400.0));
        assert!(panel.notice.is_some(), "Abweichung > 0.1 Hz ohne Hinweis");
    }

    // Unlesbare Server-Antwort: kein Absturz, Hinweis statt Datenverlust
    #[test]
    fn unlesbare_antwort_gibt_hinweis_und_laesst_zustand_stehen() {
        let mut panel = FrequencyPanel {
            confirmed_hz: Some(1000.0),
            notice: None,
        };
        apply_set_response(&mut panel, 2500.0, false, "<html>Gateway Error</html>");
        assert_eq!(panel.confirmed_hz, Some(1000.0));
        assert!(panel.notice.is_some());
    }

    // Netzwerkfehler-Pfad (fetch schlug fehl)
    #[test]
    fn note_failure_setzt_hinweis() {
        let mut panel = FrequencyPanel {
            confirmed_hz: Some(1000.0),
            notice: None,
        };
        panel.note_failure("Server nicht erreichbar");
        assert_eq!(panel.confirmed_hz, Some(1000.0));
        assert_eq!(panel.notice.as_deref(), Some("Server nicht erreichbar"));
    }
}
