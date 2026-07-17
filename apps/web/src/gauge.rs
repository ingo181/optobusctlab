//! SVG-Zeigerinstrument (LabVIEW-Optik als Inspiration, pures Leptos+SVG).
//!
//! Die Winkelberechnung ist eine reine Funktion und wird als Host-Test
//! geprüft (Spec 0002 AK4/AK5); die eigentliche SVG-Komponente bindet nur
//! noch `transform="rotate(...)"` reaktiv an dieses Ergebnis.

/// Zeiger-Anschlag links/rechts in Grad, 0° = senkrecht nach oben.
/// 120°-Gesamtausschlag wie bei einem klassischen Drehspulinstrument.
pub const ANGLE_MIN: f64 = -60.0;
pub const ANGLE_MAX: f64 = 60.0;

/// Bildet einen Messwert linear auf den Zeigerwinkel ab. Werte außerhalb
/// von `[min, max]` bleiben am jeweiligen Anschlag stehen; NaN fällt auf
/// den unteren Anschlag (ein Instrument zeigt nie "kein Winkel").
pub fn needle_angle(value: f64, min: f64, max: f64) -> f64 {
    // NaN-Vergleiche sind immer false - `clamp` würde NaN durchreichen,
    // deshalb der explizite Vorab-Check auf den unteren Anschlag.
    if value.is_nan() {
        return ANGLE_MIN;
    }
    let fraction = ((value - min) / (max - min)).clamp(0.0, 1.0);
    ANGLE_MIN + fraction * (ANGLE_MAX - ANGLE_MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    // AK4: linear zwischen Skalenanfang und -ende
    #[test]
    fn skalenanfang_mitte_ende() {
        assert_eq!(needle_angle(0.0, 0.0, 10.0), ANGLE_MIN);
        assert_eq!(needle_angle(5.0, 0.0, 10.0), 0.0);
        assert_eq!(needle_angle(10.0, 0.0, 10.0), ANGLE_MAX);
    }

    #[test]
    fn linear_dazwischen() {
        // 25% der Skala = 25% des Ausschlags
        assert_eq!(needle_angle(2.5, 0.0, 10.0), ANGLE_MIN + 30.0);
    }

    // AK5: Anschlag statt Herausdrehen
    #[test]
    fn werte_ausserhalb_bleiben_am_anschlag() {
        assert_eq!(needle_angle(-3.0, 0.0, 10.0), ANGLE_MIN);
        assert_eq!(needle_angle(42.0, 0.0, 10.0), ANGLE_MAX);
    }

    #[test]
    fn nan_faellt_auf_unteren_anschlag() {
        assert_eq!(needle_angle(f64::NAN, 0.0, 10.0), ANGLE_MIN);
    }
}
