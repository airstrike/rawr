//! Rawr's snippets in a browser: the interpreter on wasm.
//!
//! [`run_text`] runs a snippet and returns its output as the `?play`
//! server's text result.
#![cfg(target_arch = "wasm32")]

mod text;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
fn main() {
    console_error_panic_hook::set_once();
}

/// Run a snippet: its output as the server's JSON text result, or
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
