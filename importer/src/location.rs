//! Where a project's Rawr products live.
//!
//! Cargo keeps a project's build output in the project's target directory, and
//! so does Rawr: the typed store (extraction results, indexes, runner
//! executions, leases, pins, ledgers), the generated runner sources and file
//! digests live in `<target>/rawr`, or in `<target>/rawr/members/<path>`
//! for a workspace member. `cargo clean` removes them. What is shared per
//! machine stays in rawr's home (`$CARGO_HOME/rawr` or `RAWR_HOME`): the
//! embedded host sources, the runtime, the bindings, and the pinned
//! nightly's check build directory (D700), which `rawr cache clean` reaches.

pub mod copy;

use std::fs;
use std::path::{Path, PathBuf};

/// How many times this process resolved a project's Rawr directory.
static RESOLVED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many times this process resolved a project's Rawr directory, for
/// `RAWR_VERBOSE`'s account of a save.
#[must_use]
pub fn resolutions() -> usize {
    RESOLVED.load(std::sync::atomic::Ordering::Relaxed)
}

/// `<target>/rawr` for `project`, the target directory resolved as Cargo
/// does, or `<target>/rawr/members/<path>` for a workspace member
/// ([`rawr_location::rawr_directory`]).
#[must_use]
pub fn rawr_directory(project: &Path, environment: &crate::cargo::config::Environment) -> PathBuf {
    RESOLVED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
            RESOLVED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

/// The directories builds write for `project`, under the process's own
/// environment: its Cargo target directory, which holds `<target>/rawr`,
/// and its build directory when Cargo keeps intermediates elsewhere.
/// Nothing under them is a program input: a change there is a build's own
/// write, never a save.
#[must_use]
pub fn outputs_here(project: &Path) -> Box<[PathBuf]> {
    let project = fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    let environment = crate::cargo::config::Environment::capture().ok();
    let value = |key: &str| environment.as_ref().and_then(|env| env.value_of(key));
    let cargo_home = environment
        .as_ref()
        .and_then(|env| env.cargo_home().ok().flatten());
    let target = rawr_location::target_directory(&project, value, cargo_home.as_deref());
    let build = rawr_location::build_directory(&project, value, cargo_home.as_deref());
    let mut outputs = vec![target];
    if !build.starts_with(&outputs[0]) {
        outputs.push(build);
    }
    // A project kept inside a configured target directory is still its
    // own input.
    outputs.retain(|output| !project.starts_with(output));
    outputs.into_boxed_slice()
}

/// The directories Cargo unpacks registry and git dependencies into under
/// the process's own Cargo home: `registry/src` and `git/checkouts`. Their
/// sources never change while a program runs; a changed dependency is a
/// new directory there, which a changed `Cargo.lock` names.
#[must_use]
pub fn checkouts_here() -> Box<[PathBuf]> {
    let home = crate::cargo::config::Environment::capture()
        .ok()
        .and_then(|environment| environment.cargo_home().ok().flatten());
    let Some(home) = home else {
        return Box::default();
    };
    let home = fs::canonicalize(&home).unwrap_or(home);
    Box::from([home.join("registry/src"), home.join("git/checkouts")])
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

/// A path as the filesystem reaches it, without `.` or `..`
/// ([`rawr_location::resolved`]).
pub use rawr_location::resolved;
