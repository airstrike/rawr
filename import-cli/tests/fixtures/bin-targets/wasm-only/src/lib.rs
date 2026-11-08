//! A dependency the package declares only for wasm32.

#[cfg(not(target_arch = "wasm32"))]
compile_error!("`wasm-only` is built only for wasm32");
