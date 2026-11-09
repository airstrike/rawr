//! A crate with a build script, whose dependencies build for the host.

/// What the crate was built for.
pub fn arch() -> &'static str {
    "wasm32"
}
