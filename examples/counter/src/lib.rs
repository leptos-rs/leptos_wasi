mod pages;
pub mod routes;

#[cfg(feature = "ssr")]
mod server;

#[cfg(feature = "ssr")]
mod storage;

/// This is the entrypoint called by the JS "igniter" script.
#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use crate::routes::*;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}