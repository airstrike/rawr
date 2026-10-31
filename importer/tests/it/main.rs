//! The importer's integration tests, as one binary rather than one per file,
//! each linking its own copy of Rawr and the importer.
//!
//! Tests that reach the native Cargo lane hold
//! `rawr_import::store::test_turn()` for their whole body: the lane admits one
//! owner per process, so tests from different files now take turns on it.

mod cleanliness;
mod instances;
mod limits;
mod no_profile;
mod protocol;
mod resolution;

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
