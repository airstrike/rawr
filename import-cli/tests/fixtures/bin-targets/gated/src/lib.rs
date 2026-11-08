//! A dependency the package declares for every target but wasm32.

/// A greeting for `name`.
pub fn greeting(name: &str) -> String {
    format!("hello from {name}")
}
