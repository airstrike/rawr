//! Where a project's Rawr products live.
//!
//! Cargo keeps a project's build output in the project's target directory, and
//! so does Rawr: the typed store (extraction results, indexes, runner
//! executions, leases, pins, ledgers), the shared dependency build directory,
//! the generated runner sources and file digests live in `<target>/rawr`,
//! or in `<target>/rawr/members/<path>` for a workspace member.
//! `cargo clean` removes them. Only what is keyed by the Rawr install alone
//! (its embedded host sources) stays under `$CARGO_HOME/rawr`.

use std::fs;
use std::path::{Path, PathBuf};

/// `<target>/rawr` for `project`, the target directory resolved as Cargo
/// does, or `<target>/rawr/members/<path>` for a workspace member
/// ([`rawr_location::rawr_directory`]).
#[must_use]
pub fn rawr_directory(project: &Path, environment: &crate::cargo::config::Environment) -> PathBuf {
    let project = fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    rawr_location::rawr_directory(
        &project,
        |key| environment.value_of(key),
        environment.cargo_home().ok().flatten().as_deref(),
    )
}

/// [`rawr_directory`] under the process's own environment, for the files a
/// command keeps beside the store: the runner cache, the crate demand and
/// the progress estimate.
#[must_use]
pub fn rawr_directory_here(project: &Path) -> PathBuf {
    match crate::cargo::config::Environment::capture() {
        Ok(environment) => rawr_directory(project, &environment),
        Err(_) => {
            let project = fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
            rawr_location::rawr_directory(&project, |_| None, None)
        }
    }
}

/// The project's Cargo build directory, where `cargo build` keeps its
/// dependency units. Rawr's runner and resolution builds compile there, so
/// they share every unit with `cargo build`.
#[must_use]
pub fn build_directory(project: &Path, environment: &crate::cargo::config::Environment) -> PathBuf {
    let project = fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    rawr_location::build_directory(
        &project,
        |key| environment.value_of(key),
        environment.cargo_home().ok().flatten().as_deref(),
    )
}

/// The target triple `cargo build` builds `project` for when none is named,
/// `None` for the host without one.
#[must_use]
pub fn build_target(
    project: &Path,
    environment: &crate::cargo::config::Environment,
) -> Option<String> {
    rawr_location::build_target(
        project,
        |key| environment.value_of(key),
        environment.cargo_home().ok().flatten().as_deref(),
    )
}

/// The Cargo config files for `project` that set a profile for the `rawr`
/// crate by name, which Cargo cannot combine with the runner's own.
#[must_use]
pub fn rawr_profile_configs(
    project: &Path,
    environment: &crate::cargo::config::Environment,
) -> Vec<PathBuf> {
    rawr_location::rawr_profile_configs(project, environment.cargo_home().ok().flatten().as_deref())
}

/// `project`'s directory under its workspace root, when it is a member of
/// a workspace rooted elsewhere ([`rawr_location::member_path`]).
#[must_use]
pub fn member_path(project: &Path) -> Option<PathBuf> {
    rawr_location::member_path(project)
}
