//! WebSocket-Client: hängt sich an `/ws` des octlab-servers und schreibt
//! jede eingehende Messwert-Nachricht in das reaktive Signal.
//!
//! Nur im WASM-Build wirksam (`web_sys::WebSocket` existiert auf dem Host
//! zwar als Typ, aber ohne Browser dahinter) - die Host-Tests decken
//! stattdessen die reine Logik in `measurements.rs` ab.

use crate::measurements::{apply_measurement, ChannelId, Measurement};
use leptos::prelude::*;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, WebSocket};

/// Baut die WebSocket-Verbindung zum selben Host auf, von dem die Seite
/// geladen wurde. Damit funktioniert dasselbe Binary an allen drei Orten:
/// `trunk serve` (Proxy leitet `/ws` auf :3000 weiter), `octlab-server`
/// direkt und die Tauri-WebView (lädt von `http://localhost:3000`).
pub fn connect(measurements: RwSignal<HashMap<ChannelId, Measurement>>) {
    let location = web_sys::window()
        .expect("kein window-Objekt - läuft das außerhalb eines Browsers?")
        .location();
    let scheme = if location.protocol().as_deref() == Ok("https:") {
        "wss"
    } else {
        "ws"
    };
    let host = location.host().expect("window.location ohne host");
    let url = format!("{scheme}://{host}/ws");

    let ws = WebSocket::new(&url).expect("WebSocket-Objekt konnte nicht erzeugt werden");

    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        if let Some(text) = event.data().as_string() {
            measurements.update(|state| {
                apply_measurement(state, &text);
            });
        }
    });
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    // Die Closure lebt so lange wie die Seite - `forget()` gibt die Ownership
    // bewusst an die JS-Seite ab (einmaliges, gewolltes "Leak" pro Verbindung;
    // ein Drop würde den Callback sonst sofort wieder abmelden).
    on_message.forget();
}
