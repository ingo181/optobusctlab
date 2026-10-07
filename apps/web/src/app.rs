//! Wurzel-Komponente: hält den Messwert-Zustand und den Zustand des
//! DDS-Bedienfelds und zeigt drei Ansichten als Tabs (Spec 0006): Gauge
//! (DIV), Kanalübersicht (Spec 0005) und DDS-Steuerung (Spec 0003).
//!
//! Alle drei Ansichten bleiben IMMER gemountet, ein Tab-Wechsel blendet nur
//! über das `hidden`-Attribut aus und ein. Zusammen mit dem Zustand auf
//! App-Ebene (eine WebSocket-Verbindung, DDS-Zustand in `FrequencyState`)
//! geht beim Wechsel nichts verloren (Spec 0006, AK6).

use crate::frequency::FrequencyPanel;
use crate::gauge::needle_angle;
use crate::measurements::{ChannelId, Measurement};
use crate::tabs::Tab;
use leptos::prelude::*;
use std::collections::HashMap;
use std::f64::consts::PI;

/// Der eine, fest verdrahtete Kanal dieser Ausbaustufe: DIV, Adresse 1,
/// Subkanal 0 (siehe "Verifizierte Hardware-Fakten" in CLAUDE.md). Wird
/// konfigurierbar, sobald mehr als ein Instrument existiert (Spec 0002,
/// "außerhalb des Scopes").
const DIV_CHANNEL: ChannelId = (1, 0);

/// Erstes Stellglied (Spec 0003): DDS-Frequenz, Adresse 4, Subkanal 0 -
/// am realen Gerät gegen FW 3.71 verifiziert (siehe Spec-Kontext).
const DDS_ADDRESS: u8 = 4;
const DDS_FREQUENCY_SUBCHANNEL: u8 = 0;

/// Zustand des DDS-Bedienfelds, auf App-Ebene gehalten statt in
/// `FrequencyControl` (Spec 0006, AK6). `RwSignal` ist in Leptos 0.8 `Copy`:
/// Es ist nur ein Handle auf ein Signal, das dem reaktiven System gehört.
/// Deshalb ist auch dieses Bündel `Copy` und kann ohne `clone()` als Prop
/// weitergereicht werden.
#[derive(Clone, Copy)]
struct FrequencyState {
    input: RwSignal<String>,
    panel: RwSignal<FrequencyPanel>,
    invalid_input: RwSignal<bool>,
    /// Verhindert überlappende Requests: das Modul quittiert auf einem
    /// geteilten Statuskanal, gleichzeitige Setz-Vorgänge würden sich die
    /// Quittung streitig machen (siehe Doc-Kommentar an `Lab::set`).
    busy: RwSignal<bool>,
}

impl FrequencyState {
    fn new() -> Self {
        Self {
            input: RwSignal::new(String::new()),
            panel: RwSignal::new(FrequencyPanel::default()),
            invalid_input: RwSignal::new(false),
            busy: RwSignal::new(false),
        }
    }
}

/// Tab beim Laden: aus dem URL-Fragment (`#gauge`, `#uebersicht`, `#dds`),
/// sonst Gauge. Nur hier gelesen - Klicks schreiben das Fragment nicht.
fn initial_tab() -> Tab {
    let hash = web_sys::window()
        .and_then(|w| w.location().hash().ok())
        .unwrap_or_default();
    Tab::from_fragment(&hash)
}

#[component]
pub fn App() -> impl IntoView {
    let measurements = RwSignal::new(HashMap::<ChannelId, Measurement>::new());
    crate::ws::connect(measurements);
    let frequency = FrequencyState::new();
    let active = RwSignal::new(initial_tab());
    let connection_notice = watch_connection();

    let div_value =
        Signal::derive(move || measurements.with(|m| m.get(&DIV_CHANNEL).and_then(|x| x.value)));

    view! {
        <div class="app">
            <TabBar active=active connection_notice=connection_notice />
            <section
                id="view-gauge"
                class="view"
                role="tabpanel"
                aria-labelledby="tab-gauge"
                hidden=move || active.get() != Tab::Gauge
            >
                <Gauge
                    label="DIV – Adresse 1, Subkanal 0"
                    unit="V"
                    min=0.0
                    max=0.02
                    value=div_value
                />
            </section>
            <section
                id="view-uebersicht"
                class="view"
                role="tabpanel"
                aria-labelledby="tab-uebersicht"
                hidden=move || active.get() != Tab::Uebersicht
            >
                <ChannelOverview measurements=measurements />
            </section>
            <section
                id="view-dds"
                class="view"
                role="tabpanel"
                aria-labelledby="tab-dds"
                hidden=move || active.get() != Tab::Dds
            >
                <FrequencyControl state=frequency />
            </section>
        </div>
    }
}

/// Abstand der Abfragen von `GET /api/poll` für die Verbindungsanzeige
/// (Spec 0008, Entscheidung 2026-10-07).
const CONNECTION_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Fragt den Verbindungszustand zur Anlage sofort und dann alle 2 s ab.
/// Ein fehlgeschlagener Abruf (Server nicht erreichbar) wird NICHT als
/// "keine Verbindung zur Anlage" gewertet - siehe `connection.rs`.
fn watch_connection() -> RwSignal<Option<&'static str>> {
    use crate::connection::connection_notice;

    let notice = RwSignal::new(None);
    let refresh = move || {
        leptos::task::spawn_local(async move {
            let result = crate::api::get_poll().await;
            let response = match &result {
                Ok((http_ok, body)) => Ok((*http_ok, body.as_str())),
                Err(message) => Err(message.as_str()),
            };
            notice.set(connection_notice(response));
        });
    };
    refresh();
    // Der Handle wird bewusst nicht gespeichert: Die Abfrage läuft so lange
    // wie die Seite (die App wird nie abgebaut).
    let _ = set_interval_with_handle(refresh, CONNECTION_POLL_INTERVAL);
    notice
}

/// Tab-Leiste (Spec 0006, AK1): ein Button je Ansicht, der aktive ist
/// hervorgehoben und per `aria-selected` für Screenreader markiert. Der
/// kleine Name links ist kein Bedienelement, ebenso die Verbindungsanzeige
/// rechts (Spec 0008, AK7) - sie verdrängt kein Touch-Ziel.
#[component]
fn TabBar(
    active: RwSignal<Tab>,
    connection_notice: RwSignal<Option<&'static str>>,
) -> impl IntoView {
    let tabs = Tab::ALL
        .into_iter()
        .map(|tab| {
            let selected = move || active.get() == tab;
            view! {
                <button
                    id=format!("tab-{}", tab.fragment())
                    class="tab"
                    class:active=selected
                    role="tab"
                    aria-selected=move || if selected() { "true" } else { "false" }
                    aria-controls=format!("view-{}", tab.fragment())
                    on:click=move |_| active.set(tab)
                >
                    {tab.label()}
                </button>
            }
        })
        .collect_view();

    view! {
        <nav class="tabbar">
            <span class="app-name">"optobusctlab"</span>
            <div class="tabs" role="tablist" aria-label="Ansichten">
                {tabs}
            </div>
            <span class="connection-notice" role="status">
                {move || connection_notice.get().unwrap_or_default()}
            </span>
        </nav>
    }
}

/// Kanalübersicht (Spec 0005): eine Zeile pro Kanal der Server-Kanalliste
/// mit Wert, Einheit und Markierung "veraltet". Die Liste kommt einmalig
/// per `GET /api/channels`, die Werte aus demselben `/ws`-Zustand wie das
/// Gauge.
#[component]
fn ChannelOverview(measurements: RwSignal<HashMap<ChannelId, Measurement>>) -> impl IntoView {
    use crate::overview::{is_stale, value_text};

    // LocalResource statt Resource::new: WASM-Futures sind nicht `Send`
    // (Erfahrungswert aus opnCAQ, siehe CLAUDE.md).
    let channels = LocalResource::new(crate::api::get_channels);

    view! {
        <section class="overview">
            {move || match channels.get() {
                None => view! { <p class="overview-notice">"Lade Kanalliste …"</p> }.into_any(),
                Some(Err(message)) => {
                    view! { <p class="overview-notice">{message}</p> }.into_any()
                }
                Some(Ok(list)) => {
                    let rows = list
                        .into_iter()
                        .map(|channel| {
                            let id: ChannelId = (channel.address, channel.subchannel);
                            let value = move || measurements.with(|m| value_text(m.get(&id)));
                            let stale = move || measurements.with(|m| is_stale(m.get(&id)));
                            view! {
                                <tr class:stale=stale>
                                    <td>{channel.name}</td>
                                    <td class="overview-key">
                                        {format!("{}:{}", channel.address, channel.subchannel)}
                                    </td>
                                    <td class="overview-value">{value}</td>
                                    <td>{channel.unit}</td>
                                    <td class="overview-state">
                                        {move || if stale() { "veraltet" } else { "" }}
                                    </td>
                                </tr>
                            }
                        })
                        .collect_view();
                    view! {
                        <table class="overview-table">
                            <thead>
                                <tr>
                                    <th>"Kanal"</th>
                                    <th>"Adr:Sub"</th>
                                    <th class="overview-value">"Wert"</th>
                                    <th>"Einheit"</th>
                                    <th>"Status"</th>
                                </tr>
                            </thead>
                            <tbody>{rows}</tbody>
                        </table>
                    }
                        .into_any()
                }
            }}
            <p class="overview-caption">
                "Kanalbelegung und Einheiten unverifiziert (außer DDS-Frequenz), siehe Spec 0005."
            </p>
        </section>
    }
}

/// Bedienfeld für die DDS-Frequenz (Spec 0003, AK6-AK8): Eingabefeld +
/// Setzen-Button, zeigt die zuletzt per Rücklesen BESTÄTIGTE Frequenz -
/// nie den Wunschwert (Klemm-Verhalten der Firmware, siehe Spec).
///
/// Der Zustand kommt von außen (`FrequencyState` in `App`), damit er einen
/// Tab-Wechsel garantiert übersteht (Spec 0006, AK6).
#[component]
fn FrequencyControl(state: FrequencyState) -> impl IntoView {
    use crate::frequency::{apply_set_response, parse_frequency_input};

    let FrequencyState {
        input,
        panel,
        invalid_input,
        busy,
    } = state;

    let on_set = move |_| {
        match parse_frequency_input(&input.get()) {
            // AK8: keine Zahl -> kein Request, nur Kennzeichnung
            None => invalid_input.set(true),
            Some(hz) => {
                invalid_input.set(false);
                busy.set(true);
                // spawn_local statt .await im Handler: Event-Callbacks sind
                // synchron, die HTTP-Antwort kommt später - die Future wird
                // deshalb an den Browser-Scheduler übergeben und schreibt
                // ihr Ergebnis reaktiv ins Signal.
                leptos::task::spawn_local(async move {
                    match crate::api::post_set_channel(DDS_ADDRESS, DDS_FREQUENCY_SUBCHANNEL, hz)
                        .await
                    {
                        Ok((http_ok, body)) => {
                            panel.update(|p| apply_set_response(p, hz, http_ok, &body));
                        }
                        Err(message) => panel.update(|p| p.note_failure(&message)),
                    }
                    busy.set(false);
                });
            }
        }
    };

    let confirmed_text = move || match panel.get().confirmed_hz {
        Some(hz) => format!("Bestätigt: {hz} Hz"),
        None => "— noch nichts gesetzt —".to_string(),
    };
    let input_class = move || {
        if invalid_input.get() {
            "frequency-input invalid"
        } else {
            "frequency-input"
        }
    };

    // Zwei Spalten (Spec 0006, AK4): links das Bedienfeld, rechts ein
    // reservierter, noch leerer Bereich für das Ziffernfeld aus Spec 0007.
    view! {
        <div class="dds-layout">
            <section class="frequency-control">
                <div class="frequency-row">
                    <input
                        class=input_class
                        type="text"
                        inputmode="decimal"
                        placeholder="Frequenz in Hz"
                        prop:value=input
                        on:input=move |ev| input.set(event_target_value(&ev))
                    />
                    <button on:click=on_set disabled=busy>"Setzen"</button>
                </div>
                <p class="frequency-confirmed">{confirmed_text}</p>
                {move || {
                    panel.get().notice.map(|notice| view! { <p class="frequency-notice">{notice}</p> })
                }}
                <p class="frequency-caption">"DDS-Frequenz – Adresse 4, Subkanal 0"</p>
            </section>
            <div class="keypad-reserved" aria-hidden="true"></div>
        </div>
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
