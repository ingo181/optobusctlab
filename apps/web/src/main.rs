//! WASM-Einstiegspunkt, von Trunk in `index.html` eingehängt.

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(octlab_web::app::App);
}
