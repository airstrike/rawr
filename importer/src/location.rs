//! Where a project's Rawr products live.
//!
//! Cargo keeps a project's build output in the project's target directory, and
//! so does Rawr: the typed store (extraction results, indexes, runner
//! executions, leases, pins, ledgers), the shared dependency build directory,
//! the generated runner sources and file digests live in `<target>/rawr`.
//! `cargo clean` removes them. Only what is keyed by the Rawr install alone
//! (its embedded host sources) stays under `$CARGO_HOME/rawr`.

use std::fs;
use std::path::{Path, PathBuf};

/// `<target>/rawr` for `project`, the target directory resolved as Cargo does.
#[must_use]
pub fn rawr_directory(project: &Path, environment: &crate::cargo::config::Environment) -> PathBuf {
    let project = fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    rawr_location::target_directory(
        &project,
        |key| environment.value_of(key),
        environment.cargo_home().ok().flatten().as_deref(),
    )
    .join("rawr")
}
