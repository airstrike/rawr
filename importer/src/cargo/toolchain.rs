//! Crate-owned dated-nightly and schema support authority.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::process::{self, Program, ToolPaths};
use crate::{Error, Result};

mod cached;

pub use cached::{cached, Attestation};

pub(crate) const CHANNEL: &str = "nightly-2026-07-11";
const RUST_RELEASE: &str = "1.99.0-nightly";
const RUST_COMMIT: &str = "375b1431b7d89d1c2e2bc168c011848ae12b7d14";
const CARGO_RELEASE: &str = "1.99.0-nightly";
const CARGO_COMMIT: &str = "59800466c5c41c444d264b1010b4d57e85a7117f";
pub(crate) const HOST: &str = "aarch64-apple-darwin";
const REQUIRED_COMPONENTS: [&str; 2] = ["rustc-dev", "rust-src"];
#[cfg(feature = "rustc-driver")]
const SUPPORTED_PLAN_SCHEMA: u32 = 8;
const SUPPORTED_PROBE_PROTOCOL: u32 = 20;
const SUPPORTED_UNIT_SCHEMA: u32 = 1;
const SUPPORTED_GRAPH_SCHEMA: u32 = 1;

/// The one Cargo commit whose saved build-output layout is supported.
pub const fn supported_cargo_commit() -> &'static str {
    CARGO_COMMIT
}

pub(crate) const fn supported_rustc_commit() -> &'static str {
    RUST_COMMIT
}

/// Versioned metadata-probe, unit, and graph decoding protocols.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Schemas {
    probe: u32,
    unit: u32,
    graph: u32,
}

/// The crate-owned complete support requirement.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Requirement;

/// Verified installed toolchain evidence retained by a Cargo resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proof {
    channel: Box<str>,
    rustc_release: Box<str>,
    rustc_commit: Box<str>,
    rustdoc_release: Box<str>,
    rustdoc_commit: Box<str>,
    cargo_release: Box<str>,
    cargo_commit: Box<str>,
    host: Box<str>,
    target: Box<str>,
    components: Box<[Box<str>]>,
    schemas: Schemas,
    rustup: Executable,
    cargo: Executable,
    rustc: Executable,
    rustdoc: Executable,
    #[cfg(feature = "rustc-driver")]
    driver: Executable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Executable {
    path: PathBuf,
    length: u64,
    digest: [u8; 32],
}

impl Schemas {
    pub(crate) const fn current() -> Self {
        Self {
            probe: SUPPORTED_PROBE_PROTOCOL,
            unit: SUPPORTED_UNIT_SCHEMA,
            graph: SUPPORTED_GRAPH_SCHEMA,
        }
    }

    /// The pinned Cargo unit-graph schema version.
    pub const fn unit(self) -> u32 {
        self.unit
    }

    /// The canonical resolution graph schema version.
    pub const fn graph(self) -> u32 {
        self.graph
    }

    /// The selected-artifact metadata-probe protocol version.
    pub const fn probe(self) -> u32 {
        self.probe
    }
}

impl Proof {
    pub(crate) fn node_digest(&self) -> [u8; 32] {
        self.attestation_digest()
    }

    #[cfg(feature = "import-baseline")]
    pub(crate) fn observational_digest(&self) -> [u8; 32] {
        self.attestation_digest()
    }

    fn attestation_digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"rawr.phase9.toolchain-observation/v1");
        for value in [
            self.channel.as_ref(),
            self.rustc_release.as_ref(),
            self.rustc_commit.as_ref(),
            self.rustdoc_release.as_ref(),
            self.rustdoc_commit.as_ref(),
            self.cargo_release.as_ref(),
            self.cargo_commit.as_ref(),
            self.host.as_ref(),
            self.target.as_ref(),
        ] {
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
        for component in &self.components {
            digest.update((component.len() as u64).to_le_bytes());
            digest.update(component.as_bytes());
        }
        for executable in [
            &self.rustup,
            &self.cargo,
            &self.rustc,
            &self.rustdoc,
            #[cfg(feature = "rustc-driver")]
            &self.driver,
        ] {
            digest.update((executable.path.as_os_str().len() as u64).to_le_bytes());
            digest.update(executable.path.as_os_str().as_encoded_bytes());
            digest.update(executable.length.to_le_bytes());
            digest.update(executable.digest);
        }
        digest.update(self.schemas.probe.to_le_bytes());
        digest.update(self.schemas.unit.to_le_bytes());
        digest.update(self.schemas.graph.to_le_bytes());
        digest.finalize().into()
    }

    pub(crate) fn native_paths(&self) -> (&Path, &Path, &Path, &Path) {
        (
            &self.cargo.path,
            &self.rustc.path,
            &self.rustdoc.path,
            &self.rustup.path,
        )
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn native_driver_path(&self) -> &Path {
        &self.driver.path
    }
}

impl Requirement {
    pub(crate) const fn pinned() -> Self {
        Self
    }

    pub(crate) const fn default_target(self) -> &'static str {
        HOST
    }

    pub(crate) fn supports_target(self, target: &str) -> bool {
        target == HOST
    }

    pub(crate) fn verify(
        self,
        native: &impl ToolPaths,
        current_dir: &Path,
        environment: Box<[(OsString, OsString)]>,
        limits: process::Limits,
        target: &str,
    ) -> Result<Proof> {
        self.verify_in(native, current_dir, environment, limits, target, None)
    }

    fn verify_in(
        self,
        native: &impl ToolPaths,
        current_dir: &Path,
        environment: Box<[(OsString, OsString)]>,
        limits: process::Limits,
        target: &str,
        budget: Option<&crate::node::GraphBudget>,
    ) -> Result<Proof> {
        if !self.supports_target(target) {
            return Err(Error::Toolchain {
                field: "supported target",
                expected: HOST.into(),
                actual: target.into(),
            });
        }

        let rustc = run(
            native,
            process::Preflight::version(Program::Rustc)?,
            current_dir,
            environment.clone(),
            limits,
        )?;
        let rustdoc = run(
            native,
            process::Preflight::version(Program::Rustdoc)?,
            current_dir,
            environment.clone(),
            limits,
        )?;
        let cargo = run(
            native,
            process::Preflight::version(Program::Cargo)?,
            current_dir,
            environment.clone(),
            limits,
        )?;
        let rustc = Version::parse("rustc -Vv", &rustc)?;
        let rustdoc = Version::parse("rustdoc -Vv", &rustdoc)?;
        let cargo = Version::parse("cargo -Vv", &cargo)?;
        exact("rustc release", RUST_RELEASE, rustc.release)?;
        exact("rustc commit", RUST_COMMIT, rustc.commit)?;
        exact("rustc host", HOST, rustc.host)?;
        exact("rustdoc release", RUST_RELEASE, rustdoc.release)?;
        exact("rustdoc commit", RUST_COMMIT, rustdoc.commit)?;
        exact("rustdoc host", HOST, rustdoc.host)?;
        exact("Cargo release", CARGO_RELEASE, cargo.release)?;
        exact("Cargo commit", CARGO_COMMIT, cargo.commit)?;
        exact("Cargo host", HOST, cargo.host)?;

        #[cfg(feature = "rustc-driver")]
        let driver = {
            let version = run(
                native,
                process::Preflight::version(Program::Driver)?,
                current_dir,
                environment.clone(),
                limits,
            )?;
            let expected = format!(
                "rawr-import-driver {SUPPORTED_PLAN_SCHEMA} {SUPPORTED_PROBE_PROTOCOL} {RUST_COMMIT}\n"
            );
            let actual = std::str::from_utf8(&version).map_err(|error| Error::Schema {
                document: "rawr-import-driver --version",
                reason: error.to_string().into_boxed_str(),
            })?;
            exact("metadata probe driver version", &expected, actual)?;
            executable_evidence_in("metadata probe driver", native.driver(), budget)?
        };

        let rustup = executable_evidence_in(
            "rustup executable",
            native.executable(Program::Rustup),
            budget,
        )?;
        let cargo = executable_evidence_in(
            "Cargo executable",
            native.executable(Program::Cargo),
            budget,
        )?;
        let rustc = executable_evidence_in(
            "rustc executable",
            native.executable(Program::Rustc),
            budget,
        )?;
        let rustdoc = executable_evidence_in(
            "rustdoc executable",
            native.executable(Program::Rustdoc),
            budget,
        )?;

        let components = run(
            native,
            process::Preflight::component_list(),
            current_dir,
            environment.clone(),
            limits,
        )?;
        let installed_components = installed_lines("rustup component list", &components)?;
        for required in REQUIRED_COMPONENTS {
            let present = installed_components.iter().any(|installed| {
                installed.as_ref() == required
                    || required == "rustc-dev"
                        && installed
                            .strip_prefix(required)
                            .is_some_and(|suffix| suffix.starts_with('-'))
            });
            if !present {
                return Err(Error::Toolchain {
                    field: "installed component",
                    expected: required.into(),
                    actual: "missing".into(),
                });
            }
        }

        let targets = run(
            native,
            process::Preflight::target_list(),
            current_dir,
            environment,
            limits,
        )?;
        let installed_targets = installed_lines("rustup target list", &targets)?;
        if !installed_targets.contains(HOST) {
            return Err(Error::Toolchain {
                field: "installed target",
                expected: HOST.into(),
                actual: "missing".into(),
            });
        }

        Ok(Proof {
            channel: CHANNEL.into(),
            rustc_release: RUST_RELEASE.into(),
            rustc_commit: RUST_COMMIT.into(),
            rustdoc_release: RUST_RELEASE.into(),
            rustdoc_commit: RUST_COMMIT.into(),
            cargo_release: CARGO_RELEASE.into(),
            cargo_commit: CARGO_COMMIT.into(),
            host: HOST.into(),
            target: HOST.into(),
            components: REQUIRED_COMPONENTS.map(Box::from).into(),
            schemas: Schemas::current(),
            rustup,
            cargo,
            rustc,
            rustdoc,
            #[cfg(feature = "rustc-driver")]
            driver,
        })
    }

    pub(crate) const fn schemas(self) -> Schemas {
        Schemas::current()
    }
}

impl Proof {
    /// The exact dated nightly selected for this resolution.
    pub fn channel(&self) -> &str {
        &self.channel
    }
    /// The exact target triple proved installed and selected.
    pub fn target(&self) -> &str {
        &self.target
    }
    /// The exact rustc release string.
    pub fn rustc_release(&self) -> &str {
        &self.rustc_release
    }
    /// The exact full rustc commit hash.
    pub fn rustc_commit(&self) -> &str {
        &self.rustc_commit
    }
    /// The exact rustdoc release string.
    pub fn rustdoc_release(&self) -> &str {
        &self.rustdoc_release
    }
    /// The exact full rustdoc commit hash.
    pub fn rustdoc_commit(&self) -> &str {
        &self.rustdoc_commit
    }
    /// The exact Cargo release string.
    pub fn cargo_release(&self) -> &str {
        &self.cargo_release
    }
    /// The exact full Cargo commit hash.
    pub fn cargo_commit(&self) -> &str {
        &self.cargo_commit
    }
    /// The host triple reported by rustc, rustdoc, and Cargo.
    pub fn host(&self) -> &str {
        &self.host
    }
    /// The exact required components proved installed.
    pub fn components(&self) -> impl ExactSizeIterator<Item = &str> {
        self.components.iter().map(|component| component.as_ref())
    }
    /// The decoder protocols bound to the installed compiler build.
    pub const fn schemas(&self) -> Schemas {
        self.schemas
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) const fn driver_length(&self) -> u64 {
        self.driver.length
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) const fn driver_digest(&self) -> &[u8; 32] {
        &self.driver.digest
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn driver_path(&self) -> &Path {
        &self.driver.path
    }

    pub(crate) fn executables(
        &self,
    ) -> impl ExactSizeIterator<Item = (&str, &Path, u64, &[u8; 32])> {
        let base = [
            ("rustup", &self.rustup),
            ("cargo", &self.cargo),
            ("rustc", &self.rustc),
            ("rustdoc", &self.rustdoc),
        ];
        base.into_iter().map(|(name, executable)| {
            (
                name,
                executable.path.as_path(),
                executable.length,
                &executable.digest,
            )
        })
    }
}

#[cfg(test)]
fn executable_evidence(field: &'static str, path: &Path) -> Result<Executable> {
    executable_evidence_in(field, path, None)
}

fn executable_evidence_in(
    field: &'static str,
    path: &Path,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<Executable> {
    const EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
    if let Some(budget) = budget {
        budget.paths(1).map_err(evidence_budget)?;
        crate::store::path::input(path, budget).map_err(evidence_budget)?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect toolchain executable",
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > EXECUTABLE_BYTES
    {
        return Err(Error::Path {
            field,
            path: path.to_path_buf(),
            reason: "must be a bounded regular non-symlink executable".into(),
        });
    }
    // The digest is a pure function of the bytes: an unchanged identity reuses
    // it across verifications and launches.
    let identity = crate::file_digest::Identity::of(&metadata);
    if let Some(digest) = crate::file_digest::get(identity, crate::file_digest::Domain::Sha256) {
        return Ok(Executable {
            path: path.to_path_buf(),
            length: metadata.len(),
            digest,
        });
    }
    if let Some(budget) = budget {
        budget
            .hash(
                usize::try_from(metadata.len())
                    .map_err(|_| evidence_budget(crate::node::Error::Allocation))?,
            )
            .map_err(evidence_budget)?;
        // Reserve the handle's close before opening; every read is admitted
        // separately below, including the final growth probe.
        budget.paths(2).map_err(evidence_budget)?;
        crate::store::path::input(path, budget).map_err(evidence_budget)?;
    }
    let mut file = File::open(path).map_err(|source| Error::Io {
        operation: "open toolchain executable",
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(test)]
    EXECUTABLE_DIGEST_PASSES.with(|passes| passes.set(passes.get().saturating_add(1)));
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut length = 0_u64;
    loop {
        let extent = if let Some(budget) = budget {
            budget.paths(1).map_err(evidence_budget)?;
            // A growing file must not make the read exceed its admitted hash
            // extent. Once exhausted, inspect only one un-hashed growth byte.
            usize::try_from(metadata.len().saturating_sub(length))
                .map_err(|_| evidence_budget(crate::node::Error::Allocation))?
                .min(buffer.len())
                .max(1)
        } else {
            buffer.len()
        };
        let count = file
            .read(&mut buffer[..extent])
            .map_err(|source| Error::Io {
                operation: "hash toolchain executable",
                path: path.to_path_buf(),
                source,
            })?;
        if count == 0 {
            break;
        }
        if budget.is_some() && length == metadata.len() {
            return Err(Error::ResolutionDrift {
                command: "toolchain verification",
                field: "toolchain executable",
            });
        }
        length = length.checked_add(count as u64).ok_or(Error::Limit {
            resource: "toolchain executable bytes",
            limit: EXECUTABLE_BYTES as usize,
            actual: usize::MAX,
        })?;
        if length > EXECUTABLE_BYTES {
            return Err(Error::Limit {
                resource: "toolchain executable bytes",
                limit: EXECUTABLE_BYTES as usize,
                actual: usize::try_from(length).unwrap_or(usize::MAX),
            });
        }
        digest.update(&buffer[..count]);
    }
    if length != metadata.len() {
        return Err(Error::ResolutionDrift {
            command: "toolchain verification",
            field: "toolchain executable",
        });
    }
    let digest: [u8; 32] = digest.finalize().into();
    if file
        .metadata()
        .ok()
        .and_then(|after| crate::file_digest::Identity::of(&after))
        == identity
    {
        crate::file_digest::insert(identity, crate::file_digest::Domain::Sha256, digest);
    }
    Ok(Executable {
        path: path.to_path_buf(),
        length,
        digest,
    })
}

fn evidence_budget(source: crate::node::Error) -> Error {
    Error::Node {
        operation: "toolchain evidence",
        source: Box::new(source),
    }
}

fn evidence_error(error: Error) -> crate::node::Error {
    match error {
        Error::Node { source, .. } => *source,
        error => error.into(),
    }
}

struct Version<'a> {
    release: &'a str,
    commit: &'a str,
    host: &'a str,
}

impl<'a> Version<'a> {
    fn parse(document: &'static str, bytes: &'a [u8]) -> Result<Self> {
        let text = std::str::from_utf8(bytes).map_err(|error| Error::Schema {
            document,
            reason: error.to_string().into_boxed_str(),
        })?;
        let Some(release) = line_value(text, "release: ") else {
            return Err(Error::Schema {
                document,
                reason: "missing release, commit-hash, or host fact".into(),
            });
        };
        let Some(commit) = line_value(text, "commit-hash: ") else {
            return Err(Error::Schema {
                document,
                reason: "missing release, commit-hash, or host fact".into(),
            });
        };
        let Some(host) = line_value(text, "host: ") else {
            return Err(Error::Schema {
                document,
                reason: "missing release, commit-hash, or host fact".into(),
            });
        };
        Ok(Self {
            release,
            commit,
            host,
        })
    }
}

fn run(
    native: &impl ToolPaths,
    operation: process::Preflight,
    current_dir: &Path,
    environment: Box<[(OsString, OsString)]>,
    limits: process::Limits,
) -> Result<Box<[u8]>> {
    let program = operation.program();
    let command = process::Command::preflight(operation, PathBuf::from(current_dir), environment);
    native.run(&command, limits)?.checked(program)
}

fn line_value<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.lines().find_map(|line| line.strip_prefix(prefix))
}

fn exact(field: &'static str, expected: &str, actual: &str) -> Result<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(Error::Toolchain {
            field,
            expected: expected.into(),
            actual: actual.into(),
        })
    }
}

fn installed_lines(document: &'static str, bytes: &[u8]) -> Result<BTreeSet<Box<str>>> {
    let text = std::str::from_utf8(bytes).map_err(|error| Error::Schema {
        document,
        reason: error.to_string().into_boxed_str(),
    })?;
    let installed = text
        .lines()
        .filter_map(|line| line.split_ascii_whitespace().next())
        .filter(|line| !line.is_empty())
        .map(Box::from)
        .collect::<BTreeSet<_>>();
    if installed.is_empty() {
        Err(Error::Schema {
            document,
            reason: "installed set is empty".into(),
        })
    } else {
        Ok(installed)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        executable_evidence, executable_evidence_in, Executable, EXECUTABLE_DIGEST_PASSES,
    };
    use crate::{node, Error};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn executable_identity_rejects_an_alternate_path_with_identical_bytes() {
        let digest = [7_u8; 32];
        let expected = Executable {
            path: PathBuf::from("/toolchain/bin/rustc"),
            length: 42,
            digest,
        };
        let alternate = Executable {
            path: PathBuf::from("/alternate/bin/rustc"),
            length: 42,
            digest,
        };
        assert_ne!(expected, alternate);
    }

    #[test]
    fn executable_evidence_hashes_once_per_verification_epoch() {
        let path = std::env::temp_dir().join(format!(
            "rawr-tool-digest-pass-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::write(&path, b"fixture executable").expect("fixture executable can be written");
        let before = EXECUTABLE_DIGEST_PASSES.with(std::cell::Cell::get);
        executable_evidence("fixture executable", &path).expect("executable evidence seals");
        let after = EXECUTABLE_DIGEST_PASSES.with(std::cell::Cell::get);
        assert_eq!(after.saturating_sub(before), 1);
        fs::remove_file(path).expect("fixture executable can be retired");
    }

    #[test]
    fn unchanged_executable_is_not_hashed_again_and_a_changed_one_is() {
        let path = std::env::temp_dir().join(format!(
            "rawr-tool-digest-reuse-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::write(&path, b"first executable").expect("fixture executable can be written");
        let passes = || EXECUTABLE_DIGEST_PASSES.with(std::cell::Cell::get);
        let before = passes();
        let first = executable_evidence("fixture executable", &path).expect("evidence seals");
        assert_eq!(
            executable_evidence("fixture executable", &path).unwrap(),
            first
        );
        assert_eq!(
            passes() - before,
            1,
            "an unchanged identity reuses its digest"
        );
        fs::write(&path, b"other executable").expect("fixture executable is replaced");
        assert_ne!(
            executable_evidence("fixture executable", &path).unwrap(),
            first
        );
        assert_eq!(passes() - before, 2, "a changed identity is hashed again");
        fs::remove_file(path).expect("fixture executable can be retired");
    }

    #[test]
    fn budgeted_executable_evidence_charges_the_original_account_once() {
        let path = std::env::temp_dir().join(format!(
            "rawr-tool-budgeted-read-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let bytes = vec![42_u8; 64 * 1024 + 3];
        fs::write(&path, &bytes).expect("the executable fixture is written");
        let budget = node::GraphBudget::new();
        budget.hash(11).expect("prior graph work is retained");
        let before = EXECUTABLE_DIGEST_PASSES.with(std::cell::Cell::get);
        let actual = executable_evidence_in("fixture", &path, Some(&budget))
            .expect("the budgeted executable seals");
        assert_eq!(actual.length, bytes.len() as u64);
        assert_eq!(
            budget.census_usage().expect("the account is readable"),
            (0, 6, 11 + bytes.len() as u64),
            "metadata, open, reserved close, two bounded reads and EOF are admitted"
        );
        assert_eq!(
            EXECUTABLE_DIGEST_PASSES.with(std::cell::Cell::get) - before,
            1
        );
        assert_eq!(
            actual,
            executable_evidence("legacy fixture", &path).unwrap()
        );
        fs::remove_file(path).expect("the executable fixture retires");
    }

    #[test]
    fn budgeted_executable_evidence_records_hash_exceedance_and_still_hashes() {
        let path = std::env::temp_dir().join(format!(
            "rawr-tool-budgeted-exceedance-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::write(&path, b"two").expect("the executable fixture is written");
        let budget = node::GraphBudget::new();
        budget
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize - 2)
            .expect("the original account has two hash bytes left");
        assert!(!budget.exceedances().is_exceeded());
        let before = EXECUTABLE_DIGEST_PASSES.with(std::cell::Cell::get);
        // The three-byte hash crosses the target; the original account
        // keeps the charge, records the axis, and the executable is still
        // opened and hashed to the same evidence as the legacy path.
        let actual = executable_evidence_in("fixture", &path, Some(&budget))
            .expect("the hash past the target still seals the evidence");
        assert_eq!(
            EXECUTABLE_DIGEST_PASSES.with(std::cell::Cell::get) - before,
            1
        );
        assert_eq!(
            actual,
            executable_evidence("legacy fixture", &path).unwrap()
        );
        let (_, paths, hashed) = budget.census_usage().expect("the account is readable");
        assert_eq!(
            paths, 5,
            "metadata probe, reserved open and close, and two bounded reads are admitted"
        );
        assert_eq!(hashed, node::MAX_HASHED_BYTES_PER_GRAPH + 1);
        let axis = budget
            .exceedances()
            .axis(node::HASHED_RESOURCE)
            .copied()
            .expect("the hash records the hashed byte axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                node::MAX_HASHED_BYTES_PER_GRAPH,
                node::MAX_HASHED_BYTES_PER_GRAPH + 1,
                hashed
            ),
            "the three-byte hash is the single exceeding charge"
        );
        fs::remove_file(path).expect("the executable fixture retires");
    }

    #[test]
    fn budgeted_executable_evidence_records_path_exceedance_before_missing_path_metadata() {
        let budget = node::GraphBudget::new();
        budget
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("the original path account is exhausted");
        assert!(!budget.exceedances().is_exceeded());
        // The exhausted path account still admits the probe before any
        // metadata call and records the axis; only the missing path refuses.
        assert!(matches!(
            executable_evidence_in(
                "fixture",
                &PathBuf::from("/absent/toolchain"),
                Some(&budget)
            ),
            Err(Error::Io {
                operation: "inspect toolchain executable",
                ..
            })
        ));
        let paths = budget.census_usage().expect("the account is readable").1;
        let axis = budget
            .exceedances()
            .axis(node::PATHS_RESOURCE)
            .copied()
            .expect("the probe records the path-check axis");
        assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
        assert_eq!(
            axis.first_requested,
            node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
        );
        assert_eq!(axis.high_water, paths as u64);
        assert!(axis.site.is_some());
    }
}

#[cfg(test)]
thread_local! {
    static EXECUTABLE_DIGEST_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
