//! A build dependency with a dependency only the host builds.

/// What the build script runs.
pub fn run() {
    #[cfg(not(target_arch = "wasm32"))]
    hostonly::run();
}
