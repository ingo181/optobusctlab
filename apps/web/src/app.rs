//! Wurzel-Komponente: hält den Messwert-Zustand und zeigt das DIV-Gauge.

use crate::gauge::needle_angle;
use crate::measurements::{ChannelId, Measurement};
use leptos::prelude::*;
use std::collections::HashMap;
use std::f64::consts::PI;

/// Der eine, fest verdrahtete Kanal dieser Ausbaustufe: DIV, Adresse 1,
/// Subkanal 0 (siehe "Verifizierte Hardware-Fakten" in CLAUDE.md). Wird
/// konfigurierbar, sobald mehr als ein Instrument existiert (Spec 0002,
/// "außerhalb des Scopes").
const DIV_CHANNEL: ChannelId = (1, 0);

#[component]
pub fn App() -> impl IntoView {
    let measurements = RwSignal::new(HashMap::<ChannelId, Measurement>::new());
    crate::ws::connect(measurements);

    let div_value =
        Signal::derive(move || measurements.with(|m| m.get(&DIV_CHANNEL).map(|x| x.value)));

    view! {
        <main class="panel">
            <h1>"optobusctlab"</h1>
            <Gauge
                label="DIV – Adresse 1, Subkanal 0"
                unit="V"
                min=0.0
                max=0.02
                value=div_value
            />
        </main>
    }
}

/// Geometrie des Instruments (SVG-Nutzerkoordinaten): Zeiger-Drehpunkt und
/// Radien der Skalenelemente. Konstanten statt Props - es gibt genau eine
/// Instrumenten-Größe, skaliert wird per CSS über die SVG-viewBox.
const PIVOT_X: f64 = 120.0;
const PIVOT_Y: f64 = 125.0;
const R_ARC: f64 = 100.0;
const R_TICK_INNER: f64 = 90.0;
const R_TICK_INNER_MAJOR: f64 = 84.0;
const R_TICK_LABEL: f64 = 72.0;
const R_NEEDLE: f64 = 92.0;

/// Polarkoordinaten → SVG-Punkt, Winkel wie in `gauge.rs`: 0° = senkrecht
/// über dem Drehpunkt, positiv im Uhrzeigersinn.
fn polar(angle_deg: f64, radius: f64) -> (f64, f64) {
    let rad = angle_deg * PI / 180.0;
    (PIVOT_X + radius * rad.sin(), PIVOT_Y - radius * rad.cos())
}

/// Selbstgebautes Zeigerinstrument: Skalenbogen, 11 Teilstriche, Zeiger als
/// rotierte Linie. Einzige Reaktivität: der `rotate(...)`-Transform des
/// Zeigers und der Zahlenwert darunter hängen am `value`-Signal.
#[component]
fn Gauge(
    label: &'static str,
    unit: &'static str,
    min: f64,
    max: f64,
    value: Signal<Option<f64>>,
) -> impl IntoView {
    use crate::gauge::{ANGLE_MAX, ANGLE_MIN};

    // Solange noch kein Messwert da ist, ruht der Zeiger am unteren Anschlag.
    let needle_transform = move || {
        let angle = needle_angle(value.get().unwrap_or(min), min, max);
        format!("rotate({angle:.2} {PIVOT_X} {PIVOT_Y})")
    };
    let value_text = move || match value.get() {
        Some(v) => format!("{v:.4} {unit}"),
        None => "— warte auf Messwerte —".to_string(),
    };

    let (arc_start_x, arc_start_y) = polar(ANGLE_MIN, R_ARC);
    let (arc_end_x, arc_end_y) = polar(ANGLE_MAX, R_ARC);
    let arc_path = format!(
        "M {arc_start_x:.2} {arc_start_y:.2} A {R_ARC} {R_ARC} 0 0 1 {arc_end_x:.2} {arc_end_y:.2}"
    );

    let ticks = (0..=10)
        .map(|i| {
            let angle = ANGLE_MIN + f64::from(i) * (ANGLE_MAX - ANGLE_MIN) / 10.0;
            let major = i % 5 == 0;
            let inner = if major {
                R_TICK_INNER_MAJOR
            } else {
                R_TICK_INNER
            };
            let (x1, y1) = polar(angle, R_ARC);
            let (x2, y2) = polar(angle, inner);
            let tick_label = major.then(|| {
                let (lx, ly) = polar(angle, R_TICK_LABEL);
                let scale_value = min + f64::from(i) / 10.0 * (max - min);
                view! {
                    <text class="gauge-tick-label" x=lx y=ly text-anchor="middle">
                        {format!("{scale_value}")}
                    </text>
                }
            });
            view! {
                <line
                    class=if major { "gauge-tick major" } else { "gauge-tick" }
                    x1=x1 y1=y1 x2=x2 y2=y2
                />
                {tick_label}
            }
        })
        .collect_view();

    let (needle_tip_x, needle_tip_y) = (PIVOT_X, PIVOT_Y - R_NEEDLE);

    view! {
        <figure class="instrument">
            <svg viewBox="0 0 240 150" role="img" aria-label=label>
                <path class="gauge-arc" d=arc_path />
                {ticks}
                <line
                    class="gauge-needle"
                    x1=PIVOT_X y1=PIVOT_Y
                    x2=needle_tip_x y2=needle_tip_y
                    transform=needle_transform
                />
                <circle class="gauge-hub" cx=PIVOT_X cy=PIVOT_Y r="6" />
            </svg>
            <p class="gauge-value">{value_text}</p>
            <figcaption>{label}</figcaption>
        </figure>
    }
}
