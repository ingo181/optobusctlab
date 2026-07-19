//! HTTP-Aufrufe des Frontends an den octlab-server.
//!
//! Wie `ws.rs` nur im WASM-Build wirksam (braucht `window.fetch`) - die
//! Auswertung der Antwort ist davon getrennt in `frequency.rs` und läuft
//! als Host-Test. Bewusst rohes `web_sys`-fetch statt einer zusätzlichen
//! HTTP-Client-Dependency: `web-sys`/`wasm-bindgen-futures` sind über
//! Leptos ohnehin schon im Dependency-Baum.

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, Response};

/// `POST /api/channel/{address}/{subchannel}` mit dem Wunschwert.
///
/// Liefert `(HTTP-Status war 2xx, Body-Text)` - die Interpretation des
/// Bodys übernimmt `frequency::apply_set_response`. `Err` nur, wenn gar
/// kein HTTP-Austausch zustande kam (Server nicht erreichbar o.ä.).
/// Relative URL, damit dasselbe Binary hinter `trunk serve` (Proxy),
/// `octlab-server` direkt und der Tauri-WebView funktioniert.
pub async fn post_set_channel(
    address: u8,
    subchannel: u8,
    value: f64,
) -> Result<(bool, String), String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_body(&JsValue::from_str(&format!(r#"{{"value":{value}}}"#)));

    let url = format!("/api/channel/{address}/{subchannel}");
    let request = Request::new_with_str_and_init(&url, &init).map_err(js_error)?;
    request
        .headers()
        .set("content-type", "application/json")
        .map_err(js_error)?;

    let window = web_sys::window().ok_or_else(|| "kein window-Objekt".to_string())?;
    let response: Response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?
        .dyn_into()
        .map_err(js_error)?;

    let http_ok = response.ok();
    let body = JsFuture::from(response.text().map_err(js_error)?)
        .await
        .map_err(js_error)?
        .as_string()
        .unwrap_or_default();

    Ok((http_ok, body))
}

fn js_error(err: JsValue) -> String {
    format!("Anfrage fehlgeschlagen: {err:?}")
}
