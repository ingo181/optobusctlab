//! octlab-web: Leptos-CSR-Frontend (WASM, gebaut mit Trunk).
//!
//! Läuft identisch im Browser (Pi-Kiosk, `octlab-server` serviert
//! `apps/web/dist`) und in der Tauri-WebView (`apps/desktop`). Die reine
//! Logik (Messwert-Zustand, Gauge-Winkel) ist von Browser-APIs getrennt und
//! läuft als normale Host-Tests - siehe `specs/0002-web-frontend-gauge.md`.

pub mod app;
pub mod gauge;
pub mod measurements;
pub mod ws;
