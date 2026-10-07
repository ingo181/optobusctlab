//! Anzeige des Verbindungszustands zur Anlage (Spec 0008, AK7) - reine
//! Logik ohne Browser-Bezug, damit sie als Host-Test läuft. Die Komponente
//! fragt `GET /api/poll` alle 2 s ab (`app.rs`).
//!
//! Wichtig ist die Unterscheidung: Nur wenn der SERVER antwortet und
//! `"connection": "disconnected"` meldet, ist die Verbindung zur ANLAGE
//! weg. Ist der Server selbst nicht erreichbar oder antwortet er mit einem
//! Fehler, sagt das nichts über die Anlage - dann bleibt die Anzeige aus
//! (Erkennen eines Server-Neustarts ist Spec 0009).

use serde::Deserialize;

/// Ergebnis einer Abfrage von `GET /api/poll`: `Ok((http_ok, body))` wenn
/// der Server antwortete, `Err(meldung)` wenn er gar nicht erreichbar war.
pub type PollResponse<'a> = Result<(bool, &'a str), &'a str>;

/// Die Felder von `GET /api/poll`, die hier zählen (Spec 0008, AK6).
#[derive(Deserialize)]
struct PollConnection {
    connection: String,
}

/// Text für die Tab-Leiste, oder `None` (keine Anzeige).
pub fn connection_notice(response: PollResponse<'_>) -> Option<&'static str> {
    let Ok((true, body)) = response else {
        return None;
    };
    let poll: PollConnection = serde_json::from_str(body).ok()?;
    (poll.connection == "disconnected").then_some("keine Verbindung zur Anlage")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(connection: &str, attempts: u32) -> String {
        format!(
            r#"{{"interval_ms":1000,"last_cycle_ms":4,"completed_cycles":7,"overrun_episodes":0,"connection":"{connection}","reconnect_attempts":{attempts}}}"#
        )
    }

    // AK7: getrennt -> Anzeige
    #[test]
    fn getrennt_zeigt_keine_verbindung_zur_anlage() {
        let raw = body("disconnected", 3);
        assert_eq!(
            connection_notice(Ok((true, &raw))),
            Some("keine Verbindung zur Anlage")
        );
    }

    // AK7: verbunden -> keine Anzeige
    #[test]
    fn verbunden_zeigt_nichts() {
        let raw = body("connected", 0);
        assert_eq!(connection_notice(Ok((true, &raw))), None);
    }

    // Entscheidung 2026-10-07: Ist der SERVER nicht erreichbar, ist das
    // nicht "keine Verbindung zur Anlage" - die Anzeige bleibt aus
    // (Server-Neustart-Erkennung ist Spec 0009).
    #[test]
    fn server_nicht_erreichbar_zeigt_nicht_keine_verbindung_zur_anlage() {
        assert_eq!(
            connection_notice(Err("Anfrage fehlgeschlagen: TypeError")),
            None
        );
    }

    // Ebenso: Server antwortet mit Fehlerstatus (z.B. Proxy 502 bei trunk
    // serve), oder unlesbarer Körper - keine Aussage über die Anlage.
    #[test]
    fn fehlerstatus_oder_unlesbare_antwort_zeigt_nichts() {
        let raw = body("disconnected", 3);
        assert_eq!(connection_notice(Ok((false, &raw))), None);
        assert_eq!(connection_notice(Ok((true, "kein json"))), None);
    }

    // Ein Server ohne die Felder aus Spec 0008 sagt nichts über die
    // Verbindung - keine Anzeige.
    #[test]
    fn antwort_ohne_verbindungsfeld_zeigt_nichts() {
        let raw =
            r#"{"interval_ms":1000,"last_cycle_ms":4,"completed_cycles":7,"overrun_episodes":0}"#;
        assert_eq!(connection_notice(Ok((true, raw))), None);
    }
}
