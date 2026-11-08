//! Cargo discovery and bridge generation for ordinary Rust libraries.

#![forbid(unsafe_code)]

// The unchanged, test-only pinned Punycode reference uses alloc paths.
#[cfg(test)]
extern crate alloc;

// The model: what a runner needs at run time, on any target, wasm included.
// It names no store, cargo, process, node, context, host or extract module.
//
// Phase-1 proof/model internals have their production consumers behind the optional
// compiler bridge; P2 will also consume these modules in the default configuration.
// On wasm the host's store, cache and generator are absent, and so are the
// parts of these modules only they reach.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub mod api;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub mod bridge;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub mod classify;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub mod contract;
mod error;
pub mod expand;
pub mod limits;
pub mod node_parts;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub mod profile;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub mod protocol;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub mod runner;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub mod supervisor;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub mod surface;
pub mod universe;
#[cfg_attr(not(test), allow(dead_code))]
mod xid;

// The host: files, locks, processes and terminals, which build a project's
// runner and keep its store. A runner built for wasm has none of them.
#[cfg(all(feature = "rustc-driver", not(target_arch = "wasm32")))]
pub mod applicability;
#[cfg(all(feature = "rustc-driver", not(target_arch = "wasm32")))]
pub mod bindings;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
#[cfg(not(target_arch = "wasm32"))]
pub mod cache;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
#[cfg(not(target_arch = "wasm32"))]
pub mod cargo;
#[cfg(not(target_arch = "wasm32"))]
pub mod checked;
#[cfg(not(target_arch = "wasm32"))]
pub mod composition;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
#[cfg(not(target_arch = "wasm32"))]
pub mod context;
#[cfg(not(target_arch = "wasm32"))]
pub mod crate_demand;
#[cfg(not(target_arch = "wasm32"))]
pub mod deps;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
#[cfg(not(target_arch = "wasm32"))]
pub mod extract;
#[cfg(not(target_arch = "wasm32"))]
mod file_digest;
#[cfg(not(target_arch = "wasm32"))]
pub mod host;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
#[cfg(not(target_arch = "wasm32"))]
pub mod input;
#[cfg(not(target_arch = "wasm32"))]
pub mod location;
#[cfg(not(target_arch = "wasm32"))]
pub mod metrics;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
#[cfg(not(target_arch = "wasm32"))]
pub mod node;
#[cfg(all(feature = "rustc-driver", not(target_arch = "wasm32")))]
pub mod prebuilt;
#[cfg(not(target_arch = "wasm32"))]
pub mod process;
#[cfg(not(target_arch = "wasm32"))]
pub mod progress;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
#[cfg(not(target_arch = "wasm32"))]
pub mod store;

#[cfg(any(test, feature = "test-fixtures"))]
#[doc(hidden)]
pub mod test_fixture;

pub use contract::{Classified, Contract};
pub use error::Error;
pub use surface::{Demand, Sealed};

/// The result type returned by importer transformations.
pub type Result<T> = std::result::Result<T, Error>;
