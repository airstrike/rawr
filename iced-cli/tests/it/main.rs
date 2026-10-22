//! The CLI's integration tests, as one binary rather than one per file, each
//! linking its own copy of Rawr, the importer and iced.
//!
//! Tests that reach the importer's native Cargo lane hold
//! `rawr_import::store::test_turn()`: the lane admits one owner per process.

mod commands;

// The importer's shared fixtures, one instance each for the whole binary.
#[cfg(feature = "native-runner")]
mod message;
#[cfg(feature = "native-runner")]
#[path = "../../../importer/tests/it/p3.rs"]
mod p3;
#[cfg(feature = "native-runner")]
#[path = "../../../importer/tests/it/p5.rs"]
mod p5;

#[cfg(feature = "native-runner")]
mod native_race;
#[cfg(feature = "native-runner")]
mod profile_exclusive_failure;
#[cfg(feature = "native-runner")]
mod profile_lifecycle;
#[cfg(feature = "native-runner")]
mod profile_stale;
#[cfg(feature = "native-runner")]
mod runner_bootstrap;
#[cfg(feature = "native-runner")]
mod runner_failure;
#[cfg(feature = "native-runner")]
mod runner_limits;
#[cfg(feature = "native-runner")]
mod runner_restart;
#[cfg(feature = "native-runner")]
mod surface_hit;
#[cfg(feature = "native-runner")]
mod surface_miss;
