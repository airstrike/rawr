//! Bounded execution of pinned Cargo toolchain programs.

#[cfg(feature = "rustc-driver")]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{self, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(feature = "rustc-driver")]
use std::cell::Cell;

use std::os::unix::process::CommandExt as _;


use crate::{Error, Result};

/// Physical child stdout capture bound. `read_bounded` retains at most
/// this many bytes of one child's stdout and refuses a longer stream as
/// truncated. It is the 256 MiB envelope held as a safety number: it is
/// never configurable and never sized from an accounting target.
pub(crate) const CAPTURE_BYTES: usize = 256 * 1024 * 1024;
/// Physical child stderr tail (1 MiB stream tails).
pub(crate) const TAIL_BYTES: usize = 1024 * 1024;
#[cfg(feature = "rustc-driver")]
const RUST_MIN_STACK_KEY: &str = "RUST_MIN_STACK";
#[cfg(feature = "rustc-driver")]
const RUST_MIN_STACK_VALUE: &str = "67108864";
#[cfg(feature = "rustc-driver")]
const PROBE_ARGUMENT: &str = "--probe";
#[cfg(feature = "rustc-driver")]
const DIAGNOSTIC_PROBE_ARGUMENT: &str = "--diagnostic-probe";

mod groups;
pub mod hot;
pub mod invoke;
mod ownership;
pub(crate) mod parallelism;
pub mod resident;
pub mod runner;
#[cfg(test)]
pub(crate) mod test_launch;
pub mod value;

pub use groups::{remove_on_exit, retire_owned, Left};

pub use runner::{Admitted, Candidate, Error as RunnerError, Lifecycle, Owner};

/// A logical program from the exact importer toolchain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Program {
    /// The pinned Cargo executable.
    Cargo,
    /// The matching rustc executable.
    Rustc,
    /// The matching rustdoc executable.
    Rustdoc,
    /// Rustup used only to prove installed components and targets.
    Rustup,
    /// The matching Rawr-owned metadata probe executable.
    #[cfg(feature = "rustc-driver")]
    Driver,
}

/// The closed Cargo operations admitted by the importer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CargoAction {
    Metadata,
    UnitBuild(parallelism::Proof),
    AuthoritativeBuild(parallelism::Proof),
    RunnerBuild(parallelism::Proof),
    PackagePath,
    PackageGit,
    #[cfg(feature = "rustc-driver")]
    PublicationMetadata,
    #[cfg(feature = "rustc-driver")]
    PublicationUnitBuild(parallelism::Proof),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandKind {
    Cargo(CargoAction),
    Preflight(Preflight),
    #[cfg(feature = "rustc-driver")]
    Probe,
}

/// Opaque capability for one closed local discovery or attestation command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Preflight(PreflightKind);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreflightKind {
    Which(Program),
    Version(Program),
    #[cfg(feature = "rustc-driver")]
    DriverVersion,
    ComponentList,
    TargetList,
}

/// One sealed, shell-free child invocation.
#[derive(Clone)]
pub(crate) struct Command {
    program: Program,
    kind: CommandKind,
    arguments: Box<[OsString]>,
    current_dir: PathBuf,
    environment: Box<[(OsString, OsString)]>,
}

/// Per-stream allocation limits for one child invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    stdout: usize,
    stderr: usize,
}

/// Bounded bytes and status returned by a launcher.
pub(crate) struct Output {
    success: bool,
    status: Box<str>,
    stdout: Box<[u8]>,
    stderr: Box<[u8]>,
}

/// Executes sealed importer commands.
pub(crate) trait Launcher {
    /// Run one command without a shell and within the supplied stream limits.
    fn run(&self, command: &Command, limits: Limits) -> Result<Output>;
}

/// The sealed production-only launch capability derived from a discovered toolchain.
pub(crate) struct ProductionNative<'native> {
    native: &'native Native,
}

/// Private scalar-only observation of the nonpublishing canonical evidence lane.
#[cfg(feature = "rustc-driver")]
pub(crate) struct EvidenceNative<'native, 'evidence> {
    native: &'native Native,
    evidence: &'evidence EvidenceSession,
}

/// One allocation-free command or in-process stage observation.
#[cfg(feature = "rustc-driver")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct EvidenceStage {
    invocations: usize,
    elapsed: Duration,
}

/// Fixed private evidence cells; no collection or public protocol carries them.
#[cfg(feature = "rustc-driver")]
pub(crate) struct EvidenceSession {
    build: Cell<EvidenceStage>,
    probe: Cell<EvidenceStage>,
    decode: Cell<EvidenceStage>,
    canonicalization: Cell<EvidenceStage>,
    graph_sealing: Cell<EvidenceStage>,
    classification: Cell<EvidenceStage>,
}

/// The closed in-process stages observed only by canonicalization evidence.
#[cfg(feature = "rustc-driver")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EvidenceInlineStage {
    Decode,
    Canonicalization,
    GraphSealing,
    #[allow(
        dead_code,
        reason = "used only by the private nonpublishing classification evidence lane"
    )]
    Classification,
}

/// One fixed snapshot copied into the private canonical report.
#[cfg(feature = "rustc-driver")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct EvidenceStages {
    pub(crate) build: EvidenceStage,
    pub(crate) probe: EvidenceStage,
    pub(crate) decode: EvidenceStage,
    pub(crate) canonicalization: EvidenceStage,
    pub(crate) graph_sealing: EvidenceStage,
}

/// Exact executable paths shared by the two statically separated launch modes.
pub(crate) trait ToolPaths: Launcher {
    fn executable(&self, program: Program) -> &Path;

    fn cargo_tools(&self) -> (&Path, &Path) {
        (
            self.executable(Program::Rustc),
            self.executable(Program::Rustdoc),
        )
    }

    #[cfg(feature = "rustc-driver")]
    fn driver(&self) -> &Path {
        self.executable(Program::Driver)
    }

    #[cfg(feature = "rustc-driver")]
    fn rustc(&self) -> &Path {
        self.executable(Program::Rustc)
    }
}

mod production_replay {
    pub(crate) trait Sealed {}
}

/// Production-only authority to mint replayable pinned-Cargo handles.
pub(crate) trait ProductionReplay: ToolPaths + production_replay::Sealed {
    fn cargo(&self) -> Cargo {
        Cargo {
            executable: self.executable(Program::Cargo).to_path_buf(),
        }
    }

    #[cfg(feature = "rustc-driver")]
    fn evidence_session(&self) -> Option<&EvidenceSession> {
        None
    }
}

/// The only capability authorized to execute the crate-owned pinned toolchain.
#[derive(Debug, Eq, PartialEq)]
pub struct Native {
    cargo: PathBuf,
    rustc: PathBuf,
    rustdoc: PathBuf,
    rustup: PathBuf,
    #[cfg(feature = "rustc-driver")]
    driver: PathBuf,
}

/// The sealed capability for replaying exact pinned-Cargo queries.
#[derive(Clone)]
pub(crate) struct Cargo {
    executable: PathBuf,
}

impl Program {
    /// A stable diagnostic name for the logical program.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Rustc => "rustc",
            Self::Rustdoc => "rustdoc",
            Self::Rustup => "rustup",
            #[cfg(feature = "rustc-driver")]
            Self::Driver => "rawr-import-driver",
        }
    }
}

impl fmt::Debug for Command {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct EnvironmentKeys<'a>(&'a [(OsString, OsString)]);

        impl fmt::Debug for EnvironmentKeys<'_> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_list()
                    .entries(self.0.iter().map(|(key, _)| key))
                    .finish()
            }
        }

        formatter
            .debug_struct("Command")
            .field("program", &self.program)
            .field("kind", &self.kind)
            .field("arguments", &self.arguments)
            .field("current_dir", &self.current_dir)
            .field("environment_keys", &EnvironmentKeys(&self.environment))
            .finish()
    }
}

impl fmt::Debug for Output {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Output")
            .field("success", &self.success)
            .field("status", &self.status)
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .finish()
    }
}

impl Command {
    /// Transfer the sealed child environment into retained build evidence.
    pub(crate) fn into_environment(self) -> Box<[(OsString, OsString)]> {
        self.environment
    }

    pub(crate) fn cargo_arguments(
        mut prefix: Vec<OsString>,
        action: CargoAction,
        tail: Vec<OsString>,
    ) -> Result<Vec<OsString>> {
        if prefix.iter().chain(&tail).any(|argument| {
            argument == "--jobs"
                || argument == "-j"
                || argument.to_string_lossy().starts_with("--jobs=")
        }) {
            return Err(Error::Request {
                field: "Cargo parallelism",
                reason: "caller-supplied Cargo jobs arguments are forbidden".into(),
            });
        }
        let extra = 1 + usize::from(action.parallelism().is_some()) * 2 + tail.len();
        prefix.try_reserve(extra).map_err(|error| Error::Request {
            field: "Cargo arguments",
            reason: format!("failed to reserve sealed Cargo arguments: {error}").into_boxed_str(),
        })?;
        prefix.push(action.subcommand().into());
        if let Some(proof) = action.parallelism() {
            prefix.push("--jobs".into());
            prefix.push(proof.jobs().get().to_string().into());
        }
        prefix.extend(tail);
        Ok(prefix)
    }

    pub(crate) fn preflight(
        operation: Preflight,
        current_dir: PathBuf,
        environment: Box<[(OsString, OsString)]>,
    ) -> Self {
        let (program, arguments) = operation.command();
        Self {
            program,
            kind: CommandKind::Preflight(operation),
            arguments,
            current_dir,
            environment,
        }
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn probe(
        arguments: Vec<OsString>,
        current_dir: PathBuf,
        environment: Box<[(OsString, OsString)]>,
    ) -> Result<Self> {
        if arguments.first().map(OsString::as_os_str) != Some(OsStr::new(PROBE_ARGUMENT))
            || arguments.iter().skip(1).any(|argument| {
                matches!(
                    argument.to_str(),
                    Some(PROBE_ARGUMENT | DIAGNOSTIC_PROBE_ARGUMENT)
                )
            })
        {
            return Err(Error::Request {
                field: "metadata probe mode",
                reason: "must match the sealed probe authority exactly".into(),
            });
        }
        if environment
            .iter()
            .any(|(key, _)| key.as_os_str() == OsStr::new(RUST_MIN_STACK_KEY))
        {
            return Err(Error::Request {
                field: "rustc probe stack authority",
                reason: "must be supplied only by the sealed probe command".into(),
            });
        }
        let mut environment = environment.into_vec();
        environment.try_reserve(1).map_err(|error| Error::Request {
            field: "rustc probe environment",
            reason: format!("failed to reserve the sealed rustc stack authority: {error}")
                .into_boxed_str(),
        })?;
        environment.push((RUST_MIN_STACK_KEY.into(), RUST_MIN_STACK_VALUE.into()));
        Ok(Self {
            program: Program::Driver,
            kind: CommandKind::Probe,
            arguments: arguments.into_boxed_slice(),
            current_dir,
            environment: environment.into_boxed_slice(),
        })
    }

    pub(crate) fn cargo(
        prefix: Vec<OsString>,
        action: CargoAction,
        tail: Vec<OsString>,
        current_dir: PathBuf,
        environment: Box<[(OsString, OsString)]>,
    ) -> Result<Self> {
        let arguments = Self::cargo_arguments(prefix, action, tail)?;
        let mut environment = environment.into_vec();
        environment.retain(|(key, _)| {
            !matches!(
                key.to_str(),
                Some("CARGO_BUILD_JOBS" | "CARGO_MAKEFLAGS" | "MAKEFLAGS" | "MFLAGS" | "NUM_JOBS")
            )
        });
        Ok(Self {
            program: Program::Cargo,
            kind: CommandKind::Cargo(action),
            arguments: arguments.into_boxed_slice(),
            current_dir,
            environment: environment.into_boxed_slice(),
        })
    }
}

impl Preflight {
    pub(crate) fn which(program: Program) -> Result<Self> {
        match program {
            Program::Cargo | Program::Rustc | Program::Rustdoc => {
                Ok(Self(PreflightKind::Which(program)))
            }
            Program::Rustup => Err(Error::Request {
                field: "toolchain preflight",
                reason: "rustup cannot discover itself".into(),
            }),
            #[cfg(feature = "rustc-driver")]
            Program::Driver => Err(Error::Request {
                field: "toolchain preflight",
                reason: "the Rawr driver is discovered from the current executable".into(),
            }),
        }
    }

    pub(crate) fn version(program: Program) -> Result<Self> {
        match program {
            Program::Cargo | Program::Rustc | Program::Rustdoc => {
                Ok(Self(PreflightKind::Version(program)))
            }
            Program::Rustup => Err(Error::Request {
                field: "toolchain preflight",
                reason: "rustup version is not part of the pinned support proof".into(),
            }),
            #[cfg(feature = "rustc-driver")]
            Program::Driver => Ok(Self(PreflightKind::DriverVersion)),
        }
    }

    pub(crate) const fn component_list() -> Self {
        Self(PreflightKind::ComponentList)
    }

    pub(crate) const fn target_list() -> Self {
        Self(PreflightKind::TargetList)
    }

    pub(crate) const fn program(self) -> Program {
        match self.0 {
            PreflightKind::Which(_) | PreflightKind::ComponentList | PreflightKind::TargetList => {
                Program::Rustup
            }
            PreflightKind::Version(program) => program,
            #[cfg(feature = "rustc-driver")]
            PreflightKind::DriverVersion => Program::Driver,
        }
    }

    fn command(self) -> (Program, Box<[OsString]>) {
        match self.0 {
            PreflightKind::Which(program) => (
                Program::Rustup,
                [
                    "which",
                    "--toolchain",
                    crate::cargo::toolchain::CHANNEL,
                    program.as_str(),
                ]
                .map(OsString::from)
                .into(),
            ),
            PreflightKind::Version(program) => (program, Box::from([OsString::from("-Vv")])),
            #[cfg(feature = "rustc-driver")]
            PreflightKind::DriverVersion => {
                (Program::Driver, Box::from([OsString::from("--version")]))
            }
            PreflightKind::ComponentList => (
                Program::Rustup,
                [
                    "component",
                    "list",
                    "--toolchain",
                    crate::cargo::toolchain::CHANNEL,
                    "--installed",
                ]
                .map(OsString::from)
                .into(),
            ),
            PreflightKind::TargetList => (
                Program::Rustup,
                [
                    "target",
                    "list",
                    "--toolchain",
                    crate::cargo::toolchain::CHANNEL,
                    "--installed",
                ]
                .map(OsString::from)
                .into(),
            ),
        }
    }
}

impl CommandKind {
    const fn process_kind(self) -> crate::metrics::ProcessKind {
        match self {
            Self::Cargo(action) => match action {
                CargoAction::Metadata
                | CargoAction::UnitBuild(_)
                | CargoAction::AuthoritativeBuild(_) => {
                    crate::metrics::ProcessKind::CargoResolution
                }
                CargoAction::RunnerBuild(_) => crate::metrics::ProcessKind::CargoRunner,
                CargoAction::PackagePath | CargoAction::PackageGit => {
                    crate::metrics::ProcessKind::PackageList
                }
                #[cfg(feature = "rustc-driver")]
                CargoAction::PublicationMetadata | CargoAction::PublicationUnitBuild(_) => {
                    crate::metrics::ProcessKind::CargoResolution
                }
            },
            Self::Preflight(preflight) => match preflight.0 {
                PreflightKind::Which(_) => crate::metrics::ProcessKind::RustupWhich,
                PreflightKind::Version(Program::Rustc) => crate::metrics::ProcessKind::RustcVersion,
                PreflightKind::Version(Program::Rustdoc) => {
                    crate::metrics::ProcessKind::RustdocVersion
                }
                PreflightKind::Version(Program::Cargo) => crate::metrics::ProcessKind::CargoVersion,
                PreflightKind::Version(Program::Rustup) => crate::metrics::ProcessKind::RustupWhich,
                #[cfg(feature = "rustc-driver")]
                PreflightKind::Version(Program::Driver) | PreflightKind::DriverVersion => {
                    crate::metrics::ProcessKind::DriverVersion
                }
                PreflightKind::ComponentList => crate::metrics::ProcessKind::ComponentList,
                PreflightKind::TargetList => crate::metrics::ProcessKind::TargetList,
            },
            #[cfg(feature = "rustc-driver")]
            Self::Probe => crate::metrics::ProcessKind::MetadataProbe,
        }
    }
}

impl CommandKind {
    /// The launch progress stage this command belongs to, if any.
    const fn progress_stage(self) -> Option<usize> {
        match self {
            Self::Cargo(CargoAction::AuthoritativeBuild(_)) => Some(2),
            Self::Cargo(CargoAction::RunnerBuild(_)) => Some(3),
            Self::Cargo(_) => Some(1),
            _ => None,
        }
    }
}

impl CargoAction {
    const fn subcommand(self) -> &'static str {
        match self {
            #[cfg(feature = "rustc-driver")]
            Self::Metadata | Self::PublicationMetadata => "metadata",
            #[cfg(not(feature = "rustc-driver"))]
            Self::Metadata => "metadata",
            Self::UnitBuild(_) | Self::AuthoritativeBuild(_) | Self::RunnerBuild(_) => "build",
            #[cfg(feature = "rustc-driver")]
            Self::PublicationUnitBuild(_) => "build",
            Self::PackagePath | Self::PackageGit => "package",
        }
    }

    const fn parallelism(self) -> Option<parallelism::Proof> {
        match self {
            Self::UnitBuild(proof) | Self::AuthoritativeBuild(proof) | Self::RunnerBuild(proof) => {
                Some(proof)
            }
            #[cfg(feature = "rustc-driver")]
            Self::PublicationUnitBuild(proof) => Some(proof),
            Self::Metadata | Self::PackagePath | Self::PackageGit => None,
            #[cfg(feature = "rustc-driver")]
            Self::PublicationMetadata => None,
        }
    }
}

impl Limits {
    pub(crate) const fn stdout(self) -> usize {
        self.stdout
    }

    pub(crate) const fn stderr(self) -> usize {
        self.stderr
    }

    /// The physical capture bounds for a Cargo child: `CAPTURE_BYTES` of
    /// stdout and a `TAIL_BYTES` stderr tail. Accounting targets never
    /// size this slot.
    pub(crate) const fn capture() -> Self {
        Self {
            stdout: CAPTURE_BYTES,
            stderr: TAIL_BYTES,
        }
    }

    /// The physical bounds for a toolchain-discovery child (`rustup which`),
    /// whose whole output is a short path: one stream tail on each side.
    pub(crate) const fn discovery() -> Self {
        Self {
            stdout: TAIL_BYTES,
            stderr: TAIL_BYTES,
        }
    }

    /// Arbitrary stream bounds for fixtures only. Production physical slots are
    /// filled by `capture()` and `discovery()`, never by an accounting number.
    #[cfg(test)]
    pub(crate) const fn fixed(stdout: usize, stderr: usize) -> Self {
        Self { stdout, stderr }
    }
}

/// The Rust function whose future a generated runner could not make a
/// task of because it is not `Send`, with where rustc found its body:
/// rustc's E0277 at a `task::lazy` call in the runner's adapters.
fn non_send_future(stderr: &str) -> Option<(Box<str>, Box<str>)> {
    let start = stderr.find("cannot be sent between threads safely")?;
    let block = &stderr[start..];
    let block = &block[..block.find("\nerror").unwrap_or(block.len())];
    if !block.contains("task::lazy") {
        return None;
    }
    let call = block.split("lazy(move || ").nth(1)?;
    let function = call[..call.find('(')?].trim();
    let location = block
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("::: "))
        .find(|path| !path.contains("/.cargo/") && !path.contains("rawr_adapters"))
        .map(|path| match path.find("/target/rawr/crate/") {
            Some(at) => &path[at + "/target/rawr/crate/".len()..],
            None => path,
        })
        .unwrap_or("its Rust source");
    Some((function.into(), location.trim().into()))
}

impl Output {
    pub(crate) fn checked(self, program: Program) -> Result<Box<[u8]>> {
        if self.success {
            return Ok(self.stdout);
        }
        let stderr = String::from_utf8_lossy(&self.stderr);
        if let Some((function, location)) = non_send_future(&stderr) {
            return Err(Error::NonSendFuture { function, location });
        }
        Err(Error::Process {
            program: program.as_str(),
            status: self.status,
            stderr: String::from_utf8_lossy(&self.stderr)
                .into_owned()
                .into_boxed_str(),
        })
    }
}

impl Native {
    /// Fixture launch paths for scripted test launches (`test_launch`).
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self {
            cargo: PathBuf::from("/fixture/cargo"),
            rustc: PathBuf::from("/fixture/rustc"),
            rustdoc: PathBuf::from("/fixture/rustdoc"),
            rustup: PathBuf::from("/fixture/rustup"),
            #[cfg(feature = "rustc-driver")]
            driver: PathBuf::from("/fixture/driver"),
        }
    }

    /// The pinned toolchain's `cargo`, for a plain `cargo check` outside an
    /// import.
    #[must_use]
    pub fn cargo_executable(&self) -> &Path {
        &self.cargo
    }

    /// The pinned toolchain's `rustc`. A plain `cargo` run sets it as `RUSTC`,
    /// or the rustup proxy picks a toolchain per crate directory.
    #[must_use]
    pub fn rustc_executable(&self) -> &Path {
        &self.rustc
    }

    /// Discover and seal the one crate-owned dated nightly installation.
    pub fn pinned() -> Result<Self> {
        let rustup = find_program("rustup")?;
        let cargo = rustup_which(&rustup, Program::Cargo)?;
        let rustc = rustup_which(&rustup, Program::Rustc)?;
        let rustdoc = rustup_which(&rustup, Program::Rustdoc)?;
        #[cfg(feature = "rustc-driver")]
        let driver = find_driver()?;
        Ok(Self {
            cargo: canonical_program("cargo executable", &cargo)?,
            rustc: canonical_program("rustc executable", &rustc)?,
            rustdoc: canonical_program("rustdoc executable", &rustdoc)?,
            rustup,
            #[cfg(feature = "rustc-driver")]
            driver,
        })
    }

    /// Discover the pinned installation from one already sealed import context.
    pub(crate) fn pinned_in(
        current_dir: &Path,
        environment: &crate::cargo::config::Environment,
        limits: Limits,
        budget: &crate::node::GraphBudget,
    ) -> Result<Self> {
        let path = environment
            .entries()
            .find_map(|(key, value)| (key == "PATH").then_some(value))
            .ok_or_else(|| Error::Request {
                field: "native tool discovery",
                reason: "sealed PATH is absent".into(),
            })?;
        let rustup = find_program_in(
            "rustup",
            std::env::split_paths(path).map(|directory| {
                if directory.is_absolute() {
                    directory
                } else {
                    current_dir.join(directory)
                }
            }),
            budget,
        )?;
        let command_environment = environment
            .entries()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let cargo = rustup_which_in(
            &rustup,
            Program::Cargo,
            current_dir,
            command_environment.clone(),
            limits,
        )?;
        let rustc = rustup_which_in(
            &rustup,
            Program::Rustc,
            current_dir,
            command_environment.clone(),
            limits,
        )?;
        let rustdoc = rustup_which_in(
            &rustup,
            Program::Rustdoc,
            current_dir,
            command_environment,
            limits,
        )?;
        #[cfg(feature = "rustc-driver")]
        let driver = find_driver_in(Some(budget))?;
        Ok(Self {
            cargo: canonical_program_in("cargo executable", &cargo, Some(budget))?,
            rustc: canonical_program_in("rustc executable", &rustc, Some(budget))?,
            rustdoc: canonical_program_in("rustdoc executable", &rustdoc, Some(budget))?,
            rustup,
            #[cfg(feature = "rustc-driver")]
            driver,
        })
    }

    /// Reconstruct launch paths only from a current validated toolchain node.
    pub fn from_attestation(
        attestation: &crate::node::Validated<crate::cargo::toolchain::Attestation>,
    ) -> Self {
        Self::from_toolchain(attestation.value().proof())
    }

    pub(crate) fn from_toolchain(proof: &crate::cargo::toolchain::Proof) -> Self {
        let (cargo, rustc, rustdoc, rustup) = proof.native_paths();
        Self {
            cargo: cargo.to_path_buf(),
            rustc: rustc.to_path_buf(),
            rustdoc: rustdoc.to_path_buf(),
            rustup: rustup.to_path_buf(),
            #[cfg(feature = "rustc-driver")]
            driver: proof.native_driver_path().to_path_buf(),
        }
    }

    /// The installation the sealed environment selects, without launching
    /// anything. These paths key the toolchain record, so different
    /// installs keep different records instead of rejecting each other.
    pub(crate) fn selected_in(
        current_dir: &Path,
        environment: &crate::cargo::config::Environment,
        budget: &crate::node::GraphBudget,
    ) -> Result<Self> {
        let value = |name: &str| {
            environment
                .entries()
                .find_map(|(key, value)| (key == name).then_some(value))
        };
        let path = value("PATH").ok_or_else(|| Error::Request {
            field: "native tool discovery",
            reason: "sealed PATH is absent".into(),
        })?;
        let rustup = find_program_in(
            "rustup",
            std::env::split_paths(path).map(|directory| {
                if directory.is_absolute() {
                    directory
                } else {
                    current_dir.join(directory)
                }
            }),
            budget,
        )?;
        let rustup_home = value("RUSTUP_HOME")
            .map(PathBuf::from)
            .or_else(|| value("HOME").map(|home| PathBuf::from(home).join(".rustup")))
            .ok_or_else(|| Error::Request {
                field: "native tool discovery",
                reason: "sealed RUSTUP_HOME and HOME are both absent".into(),
            })?;
        let rustup_home = if rustup_home.is_absolute() {
            rustup_home
        } else {
            current_dir.join(rustup_home)
        };
        let requested_toolchain = rustup_home.join("toolchains").join(format!(
            "{}-{}",
            crate::cargo::toolchain::CHANNEL,
            crate::cargo::toolchain::HOST,
        ));
        budget.paths(1).map_err(selection_budget)?;
        crate::store::path::canonical(&requested_toolchain, budget).map_err(selection_budget)?;
        let toolchain = fs::canonicalize(&requested_toolchain).map_err(|source| Error::Io {
            operation: "canonicalize pinned toolchain root",
            path: requested_toolchain,
            source,
        })?;
        let selected_cargo = canonical_program_in(
            "Cargo executable",
            &toolchain.join("bin/cargo"),
            Some(budget),
        )?;
        let selected_rustc = canonical_program_in(
            "rustc executable",
            &toolchain.join("bin/rustc"),
            Some(budget),
        )?;
        let selected_rustdoc = canonical_program_in(
            "rustdoc executable",
            &toolchain.join("bin/rustdoc"),
            Some(budget),
        )?;
        Ok(Self {
            cargo: selected_cargo,
            rustc: selected_rustc,
            rustdoc: selected_rustdoc,
            rustup,
            #[cfg(feature = "rustc-driver")]
            driver: find_driver_in(Some(budget))?,
        })
    }

    /// Length-prefixed selected paths for the toolchain recipe.
    pub(crate) fn selection_key(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for path in [&self.rustup, &self.cargo, &self.rustc, &self.rustdoc]
            .into_iter()
            .chain(
                #[cfg(feature = "rustc-driver")]
                Some(&self.driver),
                #[cfg(not(feature = "rustc-driver"))]
                None,
            )
        {
            let path = path.as_os_str().as_encoded_bytes();
            bytes.extend_from_slice(&(path.len() as u64).to_le_bytes());
            bytes.extend_from_slice(path);
        }
        bytes
    }

    fn executable(&self, program: Program) -> &Path {
        match program {
            Program::Cargo => &self.cargo,
            Program::Rustc => &self.rustc,
            Program::Rustdoc => &self.rustdoc,
            Program::Rustup => &self.rustup,
            #[cfg(feature = "rustc-driver")]
            Program::Driver => &self.driver,
        }
    }

    pub(crate) const fn production(&self) -> ProductionNative<'_> {
        ProductionNative { native: self }
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) const fn evidence<'native, 'evidence>(
        &'native self,
        evidence: &'evidence EvidenceSession,
    ) -> EvidenceNative<'native, 'evidence> {
        EvidenceNative {
            native: self,
            evidence,
        }
    }
}

impl Launcher for ProductionNative<'_> {
    fn run(&self, command: &Command, limits: Limits) -> Result<Output> {
        execute_production(self.executable(command.program), command, limits)
    }
}

#[cfg(feature = "rustc-driver")]
impl Launcher for EvidenceNative<'_, '_> {
    fn run(&self, command: &Command, limits: Limits) -> Result<Output> {
        execute(
            self.executable(command.program),
            command,
            limits,
            self.evidence,
        )
    }
}

impl ToolPaths for ProductionNative<'_> {
    fn executable(&self, program: Program) -> &Path {
        self.native.executable(program)
    }
}

#[cfg(feature = "rustc-driver")]
impl ToolPaths for EvidenceNative<'_, '_> {
    fn executable(&self, program: Program) -> &Path {
        self.native.executable(program)
    }
}

impl production_replay::Sealed for ProductionNative<'_> {}
impl ProductionReplay for ProductionNative<'_> {}

#[cfg(feature = "rustc-driver")]
impl production_replay::Sealed for EvidenceNative<'_, '_> {}
#[cfg(feature = "rustc-driver")]
impl ProductionReplay for EvidenceNative<'_, '_> {
    fn evidence_session(&self) -> Option<&EvidenceSession> {
        Some(self.evidence)
    }
}

impl Cargo {
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self {
            executable: PathBuf::from("/fixture/cargo"),
        }
    }

    pub(crate) fn run(&self, command: &Command, limits: Limits) -> Result<Output> {
        if command.program != Program::Cargo {
            return Err(Error::Request {
                field: "pinned Cargo replay",
                reason: "the sealed command is not a Cargo invocation".into(),
            });
        }
        execute_production(&self.executable, command, limits)
    }
}

fn execute_production(executable: &Path, command: &Command, limits: Limits) -> Result<Output> {
    #[cfg(test)]
    if let Some(output) = test_launch::execute(executable, command, limits) {
        return output;
    }
    execute(executable, command, limits, &ProductionObservation)
}

trait Observation {
    fn record(&self, command: &Command, elapsed: Duration) -> Result<()>;
}

struct ProductionObservation;

impl Observation for ProductionObservation {
    fn record(&self, _command: &Command, _elapsed: Duration) -> Result<()> {
        #[cfg(feature = "import-baseline")]
        crate::metrics::record_scalar_wide(
            crate::metrics::accounting::Scalar::ProcessElapsedNanoseconds,
            _elapsed.as_nanos(),
        );
        Ok(())
    }
}

#[cfg(feature = "rustc-driver")]
impl EvidenceStage {
    fn observe(mut self, elapsed: Duration) -> Self {
        self.invocations = self.invocations.saturating_add(1);
        self.elapsed = self.elapsed.saturating_add(elapsed);
        self
    }

    pub(crate) const fn invocations(self) -> usize {
        self.invocations
    }

    pub(crate) const fn elapsed(self) -> Duration {
        self.elapsed
    }
}

#[cfg(feature = "rustc-driver")]
impl EvidenceSession {
    pub(crate) const fn new() -> Self {
        const EMPTY: EvidenceStage = EvidenceStage {
            invocations: 0,
            elapsed: Duration::ZERO,
        };
        Self {
            build: Cell::new(EMPTY),
            probe: Cell::new(EMPTY),
            decode: Cell::new(EMPTY),
            canonicalization: Cell::new(EMPTY),
            graph_sealing: Cell::new(EMPTY),
            classification: Cell::new(EMPTY),
        }
    }

    pub(crate) fn observe_inline<T>(
        &self,
        stage: EvidenceInlineStage,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let started = Instant::now();
        let result = operation();
        let cell = match stage {
            EvidenceInlineStage::Decode => &self.decode,
            EvidenceInlineStage::Canonicalization => &self.canonicalization,
            EvidenceInlineStage::GraphSealing => &self.graph_sealing,
            EvidenceInlineStage::Classification => &self.classification,
        };
        cell.set(cell.get().observe(started.elapsed()));
        result
    }

    pub(crate) fn stages(&self) -> EvidenceStages {
        EvidenceStages {
            build: self.build.get(),
            probe: self.probe.get(),
            decode: self.decode.get(),
            canonicalization: self.canonicalization.get(),
            graph_sealing: self.graph_sealing.get(),
        }
    }

    #[allow(
        dead_code,
        reason = "read only by the private nonpublishing classification evidence lane"
    )]
    pub(crate) const fn classification(&self) -> EvidenceStage {
        self.classification.get()
    }
}

#[cfg(feature = "rustc-driver")]
impl Observation for EvidenceSession {
    fn record(&self, command: &Command, elapsed: Duration) -> Result<()> {
        let cell = match command.kind {
            CommandKind::Cargo(
                CargoAction::AuthoritativeBuild(_) | CargoAction::RunnerBuild(_),
            ) => &self.build,
            CommandKind::Probe => &self.probe,
            _ => return Ok(()),
        };
        cell.set(cell.get().observe(elapsed));
        Ok(())
    }
}

fn execute<O: Observation>(
    executable: &Path,
    command: &Command,
    limits: Limits,
    observation: &O,
) -> Result<Output> {
    let mut child = process::Command::new(executable);
    child
        .args(command.arguments.iter())
        .current_dir(&command.current_dir)
        .env_clear()
        .envs(command.environment.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // The child leads its own process group so that any descendants it
        // leaves behind are cleaned up with it.
        .process_group(0);
    let started = Instant::now();
    let child = child.spawn().map_err(|source| Error::Process {
        program: command.program.as_str(),
        status: "failed to spawn".into(),
        stderr: source.to_string().into_boxed_str(),
    })?;
    crate::metrics::record_process(command.kind.process_kind());
    groups::own(child.id());
    let mut child = ChildLease(child);
    let (Some(stdout), Some(stderr)) = (child.0.stdout.take(), child.0.stderr.take()) else {
        return Err(Error::Process {
            program: command.program.as_str(),
            status: "missing output pipe".into(),
            stderr: "launcher did not create the requested pipe".into(),
        });
    };
    let stdout_limit = limits.stdout();
    let stderr_limit = limits.stderr();
    let building = command.kind.progress_stage();
    if let Some(stage) = building {
        crate::progress::stage(stage);
    }
    let watch = building.is_some_and(|stage| stage > 1);
    let stdout_reader = thread::spawn(move || {
        read_bounded(stdout, stdout_limit, |chunk| {
            if watch {
                crate::progress::observe(true, chunk);
            }
        })
    });
    let stderr_reader = thread::spawn(move || {
        read_bounded(stderr, stderr_limit, |chunk| {
            if watch {
                crate::progress::observe(false, chunk);
            }
        })
    });
    let status = child.finish();
    let recorded = observation.record(command, started.elapsed());
    let status = recorded_result(status, recorded)?;
    let stdout = join_reader(stdout_reader, "child stdout");
    let stderr = join_reader(stderr_reader, "child stderr");
    #[cfg(feature = "import-baseline")]
    record_transport(&stdout, &stderr);
    let stdout = bounded_bytes("child stdout", stdout_limit, stdout?)?;
    let stderr = bounded_bytes("child stderr", stderr_limit, stderr?)?;
    match command.kind {
        CommandKind::Cargo(CargoAction::UnitBuild(_)) => crate::progress::plan(&stdout),
        #[cfg(feature = "rustc-driver")]
        CommandKind::Cargo(CargoAction::PublicationUnitBuild(_)) => crate::progress::plan(&stdout),
        CommandKind::Cargo(CargoAction::RunnerBuild(_)) => crate::progress::stage(4),
        _ => {}
    }
    Ok(Output {
        success: status.success(),
        status: status.to_string().into_boxed_str(),
        stdout,
        stderr,
    })
}

#[cfg(feature = "import-baseline")]
fn record_transport(stdout: &Result<(Vec<u8>, usize)>, stderr: &Result<(Vec<u8>, usize)>) {
    use crate::metrics::accounting::Scalar;
    // Reuse the completed reader's observed length, including a refused excess.
    // A failed reader has unknown length; never wait longer just for evidence.
    for (result, scalar) in [
        (stdout, Scalar::ProcessStdoutBytes),
        (stderr, Scalar::ProcessStderrBytes),
    ] {
        if let Ok((_, bytes)) = result {
            crate::metrics::record_scalar(scalar, *bytes as u64);
        }
    }
}

fn recorded_result<T>(result: Result<T>, recorded: Result<()>) -> Result<T> {
    match (result, recorded) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) | (Err(error), Ok(())) => Err(error),
        (Err(primary), Err(record)) => Err(combine_terminal_failure(primary, [Err(record)])),
    }
}

/// Bounded wait for a runner that already acknowledged its shutdown.
const DRAIN_DEADLINE: Duration = Duration::from_secs(2);

/// A spawned process-group leader that is always reaped together with its group.
struct ChildLease(process::Child);

impl ChildLease {
    /// Wait for the leader, then retire every descendant it left in its group.
    fn finish(&mut self) -> Result<ExitStatus> {
        let status = self
            .0
            .wait()
            .map_err(|source| group_error("failed while reaping group leader", source))?;
        kill_group(self.0.id());
        groups::release(self.0.id());
        Ok(status)
    }
}

impl Drop for ChildLease {
    fn drop(&mut self) {
        // Reaping caches the status, so a finished lease only re-observes it.
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = reap_group(&mut self.0);
        }
        groups::release(self.0.id());
    }
}

/// SIGKILL whatever remains of a leader's process group. After the leader is
/// reaped, its PGID stays reserved for as long as any member is alive.
fn kill_group(pid: u32) {
    if let Ok(pid) = i32::try_from(pid) {
        // ESRCH means nothing is left; a zombie-only group may report EPERM.
        if let Some(pid) = rustix::process::Pid::from_raw(pid) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
}

/// Kill a live leader's whole process group, then reap the leader.
fn reap_group(child: &mut process::Child) -> Result<ExitStatus> {
    kill_group(child.id());
    child
        .wait()
        .map_err(|source| group_error("failed while reaping group leader", source))
}

fn group_error(status: &'static str, detail: impl fmt::Display) -> Error {
    Error::Process {
        program: "native process group",
        status: status.into(),
        stderr: detail.to_string().into_boxed_str(),
    }
}

fn combine_terminal_failure<const N: usize>(error: Error, cleanup: [Result<()>; N]) -> Error {
    let mut failures = String::new();
    for result in cleanup {
        if let Err(failure) = result {
            if !failures.is_empty() {
                failures.push_str("; ");
            }
            failures.push_str(&failure.to_string());
        }
    }
    if failures.is_empty() {
        error
    } else {
        Error::Process {
            program: "native process group",
            status: "joint terminal cleanup failed".into(),
            stderr: format!("primary: {error}; cleanup: {failures}").into_boxed_str(),
        }
    }
}

fn find_program(name: &'static str) -> Result<PathBuf> {
    let path = std::env::var_os("PATH").ok_or_else(|| Error::Request {
        field: "native tool discovery",
        reason: "PATH is absent".into(),
    })?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return canonical_program("rustup executable", &candidate);
        }
    }
    Err(Error::Request {
        field: "native tool discovery",
        reason: format!("`{name}` was not found in PATH").into_boxed_str(),
    })
}

fn find_program_in(
    name: &str,
    directories: impl IntoIterator<Item = PathBuf>,
    budget: &crate::node::GraphBudget,
) -> Result<PathBuf> {
    for directory in directories {
        let candidate = directory.join(name);
        selection_input(&candidate, budget)?;
        if candidate.is_file() {
            return canonical_program_in("rustup executable", &candidate, Some(budget));
        }
    }
    Err(Error::Request {
        field: "native tool discovery",
        reason: format!("`{name}` was not found in sealed PATH").into_boxed_str(),
    })
}

#[cfg(feature = "rustc-driver")]
fn find_driver() -> Result<PathBuf> {
    find_driver_in(None)
}

#[cfg(feature = "rustc-driver")]
fn find_driver_in(budget: Option<&crate::node::GraphBudget>) -> Result<PathBuf> {
    let executable = std::env::current_exe().map_err(|source| Error::Io {
        operation: "discover current importer executable",
        path: PathBuf::from("<current-executable>"),
        source,
    })?;
    let directory = executable.parent().ok_or_else(|| Error::Path {
        field: "current importer executable",
        path: executable.clone(),
        reason: "has no containing directory".into(),
    })?;
    let target_directory = if directory.file_name().is_some_and(|name| name == "deps") {
        directory.parent().ok_or_else(|| Error::Path {
            field: "current importer executable",
            path: executable.clone(),
            reason: "dependency executable has no target directory".into(),
        })?
    } else {
        directory
    };
    let candidate = target_directory.join(format!(
        "rawr-import-driver{}",
        std::env::consts::EXE_SUFFIX
    ));
    canonical_program_in("metadata probe driver", &candidate, budget)
}

fn rustup_which(rustup: &Path, program: Program) -> Result<PathBuf> {
    const LIMIT: usize = TAIL_BYTES;
    let current_dir = std::env::current_dir().map_err(|source| Error::Io {
        operation: "discover toolchain current directory",
        path: PathBuf::from("."),
        source,
    })?;
    let mut environment = Vec::new();
    for key in ["HOME", "RUSTUP_HOME"] {
        if let Some(value) = std::env::var_os(key) {
            if value.len() > LIMIT {
                return Err(Error::Limit {
                    resource: "tool discovery environment bytes",
                    limit: LIMIT,
                    actual: value.len(),
                });
            }
            environment.push((OsString::from(key), value));
        }
    }
    let command = Command::preflight(
        Preflight::which(program)?,
        current_dir,
        environment.into_boxed_slice(),
    );
    let stdout =
        execute_production(rustup, &command, Limits::discovery())?.checked(Program::Rustup)?;
    let path = std::str::from_utf8(&stdout).map_err(|error| Error::Schema {
        document: "rustup which",
        reason: error.to_string().into_boxed_str(),
    })?;
    let path = path.trim();
    if path.is_empty() {
        return Err(Error::Schema {
            document: "rustup which",
            reason: "returned an empty executable path".into(),
        });
    }
    Ok(PathBuf::from(path))
}

fn rustup_which_in(
    rustup: &Path,
    program: Program,
    current_dir: &Path,
    environment: Box<[(OsString, OsString)]>,
    limits: Limits,
) -> Result<PathBuf> {
    let command = Command::preflight(
        Preflight::which(program)?,
        current_dir.to_path_buf(),
        environment,
    );
    let stdout = execute_production(rustup, &command, limits)?.checked(Program::Rustup)?;
    let path = std::str::from_utf8(&stdout).map_err(|error| Error::Schema {
        document: "rustup which",
        reason: error.to_string().into_boxed_str(),
    })?;
    let path = path.trim();
    if path.is_empty() {
        return Err(Error::Schema {
            document: "rustup which",
            reason: "returned an empty executable path".into(),
        });
    }
    Ok(PathBuf::from(path))
}

fn canonical_program(field: &'static str, path: &Path) -> Result<PathBuf> {
    canonical_program_in(field, path, None)
}

fn canonical_program_in(
    field: &'static str,
    path: &Path,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<PathBuf> {
    if let Some(budget) = budget {
        budget.paths(1).map_err(selection_budget)?;
        crate::store::path::canonical(path, budget).map_err(selection_budget)?;
    }
    let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
        operation: "canonicalize executable",
        path: path.to_path_buf(),
        source,
    })?;
    if let Some(budget) = budget {
        selection_input(&canonical, budget)?;
    }
    if !canonical.is_file() {
        return Err(Error::Path {
            field,
            path: canonical,
            reason: "expected a file".into(),
        });
    }
    Ok(canonical)
}

fn selection_input(path: &Path, budget: &crate::node::GraphBudget) -> Result<()> {
    budget.paths(1).map_err(selection_budget)?;
    crate::store::path::input(path, budget).map_err(selection_budget)
}

fn selection_budget(source: crate::node::Error) -> Error {
    Error::Node {
        operation: "toolchain selection",
        source: Box::new(source),
    }
}

fn read_bounded(
    mut reader: impl Read,
    limit: usize,
    mut observe: impl FnMut(&[u8]),
) -> io::Result<(Vec<u8>, usize)> {
    let capacity = limit.min(64 * 1024);
    let mut retained = Vec::new();
    retained.try_reserve_exact(capacity)?;
    let mut observed = 0_usize;
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        observed = observed.saturating_add(count);
        observe(&chunk[..count]);
        if retained.len() < limit {
            let remaining = limit - retained.len();
            let keep = count.min(remaining);
            retained.try_reserve(keep)?;
            retained.extend_from_slice(&chunk[..keep]);
        }
    }
    Ok((retained, observed))
}

fn join_reader(
    reader: thread::JoinHandle<io::Result<(Vec<u8>, usize)>>,
    resource: &'static str,
) -> Result<(Vec<u8>, usize)> {
    reader
        .join()
        .map_err(|_| Error::Process {
            program: "child",
            status: "output reader panicked".into(),
            stderr: resource.into(),
        })?
        .map_err(|source| Error::Process {
            program: "child",
            status: "failed to read output".into(),
            stderr: source.to_string().into_boxed_str(),
        })
}

fn bounded_bytes(
    resource: &'static str,
    limit: usize,
    (bytes, observed): (Vec<u8>, usize),
) -> Result<Box<[u8]>> {
    if observed > limit {
        return Err(Error::Limit {
            resource,
            limit,
            actual: observed,
        });
    }
    Ok(bytes.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::*;

    #[cfg(feature = "import-baseline")]
    #[test]
    fn transport_observation_keeps_refused_length_and_unknown_reader_distinct() {
        use crate::metrics::{accounting::Scalar, recorder::Recorder};
        let recorder = Recorder::transform(100).unwrap();
        let scope = recorder.scope().unwrap();
        let stdout = Ok((vec![1; 4], 17));
        let stderr = Ok((Vec::new(), 0));
        record_transport(&stdout, &stderr);
        let failure = bounded_bytes("child stdout", 16, stdout.unwrap()).unwrap_err();
        assert!(matches!(
            failure,
            Error::Limit {
                actual: 17,
                limit: 16,
                ..
            }
        ));
        let unavailable = Err(group_error("reader failed", "fixture"));
        record_transport(&unavailable, &unavailable);
        drop(scope);
        let accounting = recorder.snapshot().unwrap().accounting;
        assert_eq!(
            accounting.scalar(Scalar::ProcessStdoutBytes).unwrap().max,
            17
        );
        let stderr = accounting.scalar(Scalar::ProcessStderrBytes).unwrap();
        assert_eq!((stderr.count, stderr.max), (1, 0));
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn production_recording_fault_preserves_the_original_completion_error() {
        use crate::metrics::accounting::{Fault, Scalar};

        let recorder = crate::metrics::recorder::Recorder::transform_at(1, 1).unwrap();
        let scope = recorder.scope().unwrap();
        let command = Command::preflight(
            Preflight::component_list(),
            PathBuf::from("fixture"),
            Box::default(),
        );
        let record = || ProductionObservation.record(&command, Duration::from_nanos(7));
        assert_eq!(recorded_result(Ok(5), record()).unwrap(), 5);
        let failure: Result<()> = Err(Error::Limit {
            resource: "original native failure",
            limit: 123,
            actual: 124,
        });
        assert!(matches!(
            recorded_result(failure, record()),
            Err(Error::Limit {
                resource: "original native failure",
                limit: 123,
                actual: 124,
            })
        ));
        drop(scope);
        let evidence = recorder.snapshot().unwrap().accounting;
        assert_eq!(
            evidence.fault,
            Some(Fault::ScalarOverflow(Scalar::ProcessElapsedNanoseconds))
        );
        assert_eq!(
            evidence
                .scalar(Scalar::ProcessElapsedNanoseconds)
                .unwrap()
                .count,
            1
        );
    }

    #[test]
    fn sealed_tool_selection_charges_only_attempted_paths_and_canonical_storage() {
        let executable = std::env::current_exe().expect("test executable");
        let directory = executable.parent().expect("executable directory");
        let name = executable
            .file_name()
            .expect("executable name")
            .to_str()
            .expect("UTF-8 fixture");
        let budget = crate::node::GraphBudget::new();
        let selected = find_program_in(
            name,
            [
                directory.join("missing-selection-directory"),
                directory.to_path_buf(),
            ],
            &budget,
        )
        .expect("second PATH entry selects executable");
        assert_eq!(
            selected,
            fs::canonicalize(&executable).expect("canonical test executable")
        );
        let (decoded, paths, hashed) = budget.census_usage().expect("usage");
        assert_eq!(paths, 4, "two PATH probes, canonicalize, selected metadata");
        assert_eq!(hashed, 0, "selection is separate from executable capture");
        assert!(decoded >= crate::store::path::canonical_allocation() as u64);
    }

    #[test]
    fn sealed_tool_selection_charges_before_filesystem_work() {
        let missing = Path::new("/rawr-tool-selection-must-not-be-opened");
        // Exact selection charges against a missing path, on a fresh account.
        let probe = crate::node::GraphBudget::new();
        find_program_in("rustup", [missing.to_path_buf()], &probe).expect_err("probe misses");
        let (_, probe_paths, _) = probe.census_usage().expect("usage");
        assert_eq!(probe_paths, 1, "one PATH probe");
        let probe = crate::node::GraphBudget::new();
        canonical_program_in("tool", missing, Some(&probe)).expect_err("canonicalize fails");
        let (canonical_decoded, canonical_paths, _) = probe.census_usage().expect("usage");
        assert_eq!(canonical_paths, 1, "one canonicalize probe");
        assert!(
            canonical_decoded > 0,
            "canonical storage is charged before realpath"
        );
        // An exhausted path target never refuses selection. Each probe is
        // still charged before its filesystem work and recorded past the target,
        // and the work then reports its own missing-path outcome.
        let budget = crate::node::GraphBudget::new();
        budget
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("fill path account");
        assert!(matches!(
            find_program_in("rustup", [missing.to_path_buf()], &budget),
            Err(Error::Request {
                field: "native tool discovery",
                ..
            })
        ));
        assert!(matches!(
            canonical_program_in("tool", missing, Some(&budget)),
            Err(Error::Io {
                operation: "canonicalize executable",
                ..
            })
        ));
        let paths = budget
            .exceedances()
            .axis(crate::node::PATHS_RESOURCE)
            .copied()
            .expect("the path-check axis crossed its target");
        assert_eq!(
            (paths.first_requested, paths.high_water),
            (
                crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1,
                crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 2
            ),
            "one path check per probe lands on the shared account before any IO"
        );
        let budget = crate::node::GraphBudget::new();
        budget
            .decode(crate::node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("fill decode account");
        assert!(matches!(
            canonical_program_in("tool", missing, Some(&budget)),
            Err(Error::Io {
                operation: "canonicalize executable",
                ..
            })
        ));
        assert_eq!(
            budget
                .exceedances()
                .axis(crate::node::DECODED_RESOURCE)
                .map(|axis| axis.high_water),
            Some(crate::node::MAX_DECODED_BYTES_PER_GRAPH + canonical_decoded),
            "canonical storage is charged in full before realpath runs"
        );
    }

    #[test]
    fn closed_command_roles_map_without_argv_classification() {
        use crate::metrics::ProcessKind;

        for program in [Program::Cargo, Program::Rustc, Program::Rustdoc] {
            assert_eq!(
                CommandKind::Preflight(Preflight::which(program).expect("which role seals"))
                    .process_kind(),
                ProcessKind::RustupWhich
            );
        }
        for (program, expected) in [
            (Program::Cargo, ProcessKind::CargoVersion),
            (Program::Rustc, ProcessKind::RustcVersion),
            (Program::Rustdoc, ProcessKind::RustdocVersion),
        ] {
            assert_eq!(
                CommandKind::Preflight(Preflight::version(program).expect("version role seals"))
                    .process_kind(),
                expected
            );
        }
        assert_eq!(
            CommandKind::Preflight(Preflight::component_list()).process_kind(),
            ProcessKind::ComponentList
        );
        assert_eq!(
            CommandKind::Preflight(Preflight::target_list()).process_kind(),
            ProcessKind::TargetList
        );
        assert_eq!(
            CommandKind::Cargo(CargoAction::Metadata).process_kind(),
            ProcessKind::CargoResolution
        );
        assert_eq!(
            CommandKind::Cargo(CargoAction::PackagePath).process_kind(),
            ProcessKind::PackageList
        );
        assert_eq!(
            CommandKind::Cargo(CargoAction::PackageGit).process_kind(),
            ProcessKind::PackageList
        );
        #[cfg(feature = "rustc-driver")]
        {
            assert_eq!(
                CommandKind::Preflight(
                    Preflight::version(Program::Driver).expect("driver version seals")
                )
                .process_kind(),
                ProcessKind::DriverVersion
            );
            assert_eq!(
                CommandKind::Probe.process_kind(),
                ProcessKind::MetadataProbe
            );
        }
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn probe_command_seals_one_exact_inner_rustc_stack_authority() {
        let probe = Command::probe(
            vec![PROBE_ARGUMENT.into()],
            PathBuf::from("fixture"),
            Box::default(),
        )
        .expect("the sealed probe command supplies its own stack authority");
        assert_eq!(
            probe.environment.as_ref(),
            [(RUST_MIN_STACK_KEY.into(), RUST_MIN_STACK_VALUE.into())]
        );

        let forged = Command::probe(
            vec![PROBE_ARGUMENT.into()],
            PathBuf::from("fixture"),
            vec![(RUST_MIN_STACK_KEY.into(), "16777216".into())].into_boxed_slice(),
        )
        .expect_err("caller-supplied stack authority is rejected");
        assert!(matches!(
            forged,
            Error::Request {
                field: "rustc probe stack authority",
                ..
            }
        ));

        let diagnostics = Command::probe(
            vec![DIAGNOSTIC_PROBE_ARGUMENT.into()],
            PathBuf::from("fixture"),
            Box::default(),
        )
        .expect_err("normal probes cannot request evidence-only diagnostics");
        assert!(matches!(
            diagnostics,
            Error::Request {
                field: "metadata probe mode",
                ..
            }
        ));
    }

    #[test]
    fn cargo_jobs_follow_the_compilation_subcommand_and_queries_omit_them() {
        let proof = parallelism::Proof::fixture(
            NonZeroUsize::new(7).expect("fixture parallelism is nonzero"),
        )
        .expect("fixture parallelism seals");
        let compilation = Command::cargo(
            vec![
                "--config".into(),
                "config.toml".into(),
                "-Z".into(),
                "unstable-options".into(),
            ],
            CargoAction::UnitBuild(proof),
            vec!["--unit-graph".into()],
            PathBuf::from("fixture"),
            Box::default(),
        )
        .expect("controlled compilation arguments seal");
        assert_eq!(
            compilation.arguments.as_ref(),
            [
                "--config",
                "config.toml",
                "-Z",
                "unstable-options",
                "build",
                "--jobs",
                "7",
                "--unit-graph",
            ]
            .map(OsString::from)
        );

        let query = Command::cargo(
            Vec::new(),
            CargoAction::Metadata,
            vec!["--format-version".into(), "1".into()],
            PathBuf::from("fixture"),
            Box::default(),
        )
        .expect("metadata query seals");
        assert!(!query.arguments.iter().any(|argument| argument == "--jobs"));
    }

    #[test]
    fn cargo_action_owns_subcommand_even_when_profile_names_are_cargo_words() {
        let proof = parallelism::Proof::fixture(
            NonZeroUsize::new(5).expect("fixture parallelism is nonzero"),
        )
        .expect("fixture parallelism seals");
        for profile in ["metadata", "build", "check", "doc"] {
            let command = Command::cargo(
                vec!["--config".into(), "build".into()],
                CargoAction::AuthoritativeBuild(proof),
                vec!["--profile".into(), profile.into()],
                PathBuf::from("fixture"),
                Box::default(),
            )
            .expect("closed Cargo action seals independently of profile spelling");
            assert_eq!(command.arguments[2], "build");
            assert_eq!(command.arguments[3], "--jobs");
            assert_eq!(command.arguments[4], "5");
            assert_eq!(
                command
                    .arguments
                    .iter()
                    .filter(|argument| argument.as_os_str() == "--jobs")
                    .count(),
                1
            );
        }
    }

    #[test]
    fn leader_exit_reaps_descendants_left_in_its_group() {
        let root = std::env::temp_dir().join(format!(
            "rawr-leader-exit-descendants-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("descendant fixture root can be created");
        let escaped = root.join("escaped");
        let mut child = process::Command::new("/bin/sh");
        child
            .args([
                "-c",
                "(sleep 1; printf escaped > \"$1\") & exit 0",
                "fixture",
            ])
            .arg(&escaped)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut lease = ChildLease(child.spawn().expect("fixture process group can spawn"));
        assert!(lease
            .finish()
            .expect("leader exits and is reaped")
            .success());
        thread::sleep(Duration::from_millis(1200));
        assert!(!escaped.exists());
        fs::remove_dir_all(root).expect("descendant fixture can be retired");
    }

    #[test]
    fn cargo_command_scrubs_jobserver_forgery() {
        let proof =
            parallelism::Proof::fixture(NonZeroUsize::MIN).expect("fixture parallelism seals");
        let environment = [
            ("CARGO_BUILD_JOBS", "99"),
            ("CARGO_MAKEFLAGS", "forged"),
            ("MAKEFLAGS", "forged"),
            ("MFLAGS", "forged"),
            ("NUM_JOBS", "99"),
            ("RETAINED", "yes"),
        ]
        .map(|(key, value)| (key.into(), value.into()));
        let command = Command::cargo(
            Vec::new(),
            CargoAction::AuthoritativeBuild(proof),
            Vec::new(),
            PathBuf::from("fixture"),
            Box::from(environment),
        )
        .expect("controlled Cargo command seals");
        assert_eq!(
            command.environment.as_ref(),
            [(OsString::from("RETAINED"), OsString::from("yes"))]
        );
    }
}

#[cfg(test)]
mod non_send_tests {
    #[test]
    fn a_non_send_future_is_named_with_its_source() {
        let stderr = "error[E0277]: `Rc<i64>` cannot be sent between threads safely\n   --> src/rawr_adapters_app_f.rs:9:18\n    |\n  9 |     let output = __rawr_profile_host::task::lazy(move || app::download::local());\n    |\n   ::: /p/app/target/rawr/crate/src/download.rs:42:25\n    |\nnote: required by a bound in `rawr_iced::task::lazy`\n   --> /Users/x/.cargo/rawr/hosts/1/iced/src/task.rs:319:30\n";
        let (function, location) = super::non_send_future(stderr).unwrap();
        assert_eq!(&*function, "app::download::local");
        assert_eq!(&*location, "src/download.rs:42:25");
        assert!(super::non_send_future("error[E0308]: mismatched types").is_none());
    }
}
