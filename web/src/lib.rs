//! Rawr's snippets in a browser: the interpreter on wasm.
//!
//! [`run_text`] runs a plain program and returns its output as the `?play`
//! server's text result. A snippet that needs iced or an async runtime gets
//! the server's `{"kind":"unavailable","text":..}` instead (see
//! `rawr_snippet::route`): neither runs in the browser until generated
//! bindings are built for wasm.
#![cfg(target_arch = "wasm32")]

mod text;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
fn main() {
    console_error_panic_hook::set_once();
}

/// Run a snippet: a plain program's output as the server's JSON text
/// result, `{"kind":"unavailable","text":..}` for an iced or async one, or
/// `{"kind":"error","text":..}` when it does not compile.
#[wasm_bindgen]
pub fn run_text(code: &str) -> String {
    text::run(code)
}

fn now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or(0.0, |performance| performance.now())
}
