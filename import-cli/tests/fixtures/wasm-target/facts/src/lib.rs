//! A crate whose API follows the target it is built for.

/// What the crate was built for: only wasm32 has it.
#[cfg(target_arch = "wasm32")]
pub fn arch() -> &'static str {
    "wasm32"
}

/// Every target but wasm32 has it.
#[cfg(not(target_arch = "wasm32"))]
pub fn native() -> &'static str {
    "native"
}
