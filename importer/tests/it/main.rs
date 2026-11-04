//! The importer's integration tests, as one binary rather than one per file,
//! each linking its own copy of Rawr and the importer.
//!
//! Tests run in parallel. The native Cargo lane admits one owner per process,
//! so a test holds [`shared::lane`] only around the calls that take it.
//! Everything a test builds goes to one shared Cargo build directory, and
//! each fixture a test only reads is extracted once per process:
//!
//! - `p2::contract`: the symbolic facade under two aliases (classify,
//!   demand, fingerprints, symbolic_contract);
//! - `p3::generated`: the runner generated from it (generate, manifest,
//!   monomorphic_execution);
//! - `p4::shared`: the generic fixture (callable_*, surface_identity);
//! - the facade fixture's graph and `widget::Button`'s demand (support,
//!   on_demand).
//!
//! A test that changes what it built, or counts cache misses, keeps a project
//! of its own: the runner tests (artifact, cache, process, runner_invoke,
//! runner_snapshot, hot_reload) a fresh project cache, and runner_dag a fresh
//! Cargo home, Rawr home and build directory as well. A project's cache lives
//! in its `target/rawr`, which an inherited `CARGO_TARGET_DIR` would move and
//! share, so the suite runs with `--target-dir` instead:
//!
//! ```text
//! env -u CARGO_TARGET_DIR cargo test -p rawr-import \
//!     --features rustc-driver,test-fixtures --target-dir <dir> --test it
//! ```

mod instances;
mod limits;
mod no_profile;
mod protocol;
mod resolution;
mod shared;

// Shared fixtures, one instance each, so their counters and project
// directories stay unique across the whole binary.
#[cfg(feature = "rustc-driver")]
mod p2;
#[cfg(feature = "rustc-driver")]
mod p3;
#[cfg(feature = "rustc-driver")]
mod p4;
#[cfg(feature = "rustc-driver")]
mod p5;

#[cfg(feature = "rustc-driver")]
mod applicability;
#[cfg(feature = "rustc-driver")]
mod artifact;
#[cfg(feature = "rustc-driver")]
mod build_script;
#[cfg(feature = "rustc-driver")]
mod builders;
#[cfg(feature = "rustc-driver")]
mod cache;
#[cfg(feature = "rustc-driver")]
mod callable_borrowed;
#[cfg(feature = "rustc-driver")]
mod callable_compile_fail;
#[cfg(feature = "rustc-driver")]
mod callable_once;
#[cfg(feature = "rustc-driver")]
mod callable_owned;
#[cfg(feature = "rustc-driver")]
mod classify;
#[cfg(feature = "rustc-driver")]
mod crate_reexport;
#[cfg(feature = "rustc-driver")]
mod demand;
#[cfg(feature = "rustc-driver")]
mod extraction;
#[cfg(feature = "rustc-driver")]
mod fingerprints;
#[cfg(feature = "rustc-driver")]
mod function_items;
#[cfg(feature = "rustc-driver")]
mod generate;
#[cfg(feature = "rustc-driver")]
mod hot_reload;
#[cfg(feature = "rustc-driver")]
mod manifest;
#[cfg(feature = "rustc-driver")]
mod monomorphic_execution;
#[cfg(feature = "rustc-driver")]
mod on_demand;
#[cfg(feature = "rustc-driver")]
mod process;
#[cfg(feature = "rustc-driver")]
mod profile_contract;
#[cfg(feature = "rustc-driver")]
mod profile_generation;
#[cfg(feature = "rustc-driver")]
mod reachability;
#[cfg(all(feature = "rustc-driver", feature = "test-fixtures"))]
mod runner_dag;
#[cfg(feature = "rustc-driver")]
mod runner_invoke;
#[cfg(feature = "rustc-driver")]
mod runner_snapshot;
#[cfg(feature = "rustc-driver")]
mod support;
#[cfg(feature = "rustc-driver")]
mod surface_identity;
#[cfg(feature = "rustc-driver")]
mod symbolic_contract;
#[cfg(feature = "rustc-driver")]
mod tolerance;
