//! Cargo discovery and bridge generation for ordinary Rust libraries.

#![forbid(unsafe_code)]

// The unchanged, test-only pinned Punycode reference uses alloc paths.
#[cfg(test)]
extern crate alloc;

// Phase-1 proof/model internals have their production consumers behind the optional
// compiler bridge; P2 will also consume these modules in the default configuration.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub mod api;
#[cfg(feature = "rustc-driver")]
pub mod bindings;
pub mod bridge;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub mod cache;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub mod cargo;
pub mod checked;
pub mod classify;
pub mod composition;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub mod context;
pub mod contract;
pub mod crate_demand;
pub mod deps;
mod error;
pub mod exceedance;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub mod extract;
mod file_digest;
pub mod host;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub mod input;
pub mod location;
pub mod metrics;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub mod node;
pub mod process;
pub mod profile;
pub mod progress;
pub mod protocol;
pub mod runner;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub mod store;
pub mod supervisor;
pub mod surface;

#[cfg(any(test, feature = "test-fixtures"))]
#[doc(hidden)]
pub mod test_fixture;

#[cfg_attr(not(test), allow(dead_code))]
mod xid;

pub use contract::{Classified, Contract};
pub use error::Error;
pub use surface::{Demand, Sealed};

/// The result type returned by importer transformations.
pub type Result<T> = std::result::Result<T, Error>;
