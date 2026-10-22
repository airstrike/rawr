//! Parent-owned receipts for actual, separately authorized release builds.
//!
//! A receipt witnesses one Cargo invocation against a committed candidate. It
//! is not a hermetic-build claim: ignored files and external tool inputs remain
//! operator-controlled. Environment values are digested, never copied into a
//! report where credentials could be disclosed.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::{Read as _, Write as _};
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use rawr_import::metrics::{cell, evidence, report};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const SCHEMA: &str = "rawr.phase9.build-provenance/v2";
const RECEIPT: &str = "build-provenance.json";
const TOOLCHAIN: &str = "nightly-2026-07-11";
// Existing report and owner-envelope bounds; this does not change product caps.
const REPORT_BYTES: usize = 8 * 1024 * 1024;
const EXCLUDES: [&str; 2] = [
    ":(exclude)SOL_PROMPT.md",
    ":(exclude)editors/vscode-rawr/rawr-language-0.0.1.vsix",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) enum Kind {
    Ordinary,
    Evidence,
    Observer,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Source {
    root: PathBuf,
    commit: Box<str>,
    tree: Box<str>,
    lockfile: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Artifact {
    cargo_record: serde_json::Value,
    file: evidence::process::file::Identity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Receipt {
    schema: Box<str>,
    kind: Kind,
    source: Source,
    program: evidence::process::file::Identity,
    cargo_executable: evidence::process::file::Identity,
    compiler_executable: evidence::process::file::Identity,
    invocation: Vec<OsString>,
    environment_sha256: [u8; 32],
    compiler: Box<str>,
    cargo: Box<str>,
    target: Box<str>,
    profile: Box<str>,
    cargo_stdout_sha256: [u8; 32],
    build_finished: serde_json::Value,
    artifacts: Vec<Artifact>,
    sdk: Option<Sdk>,
}

// The explicitly selected SDK is part of the private observer's build receipt.
// Product receipts retain their existing toolchain policy.
const OBSERVER_SDK: &str = "/Library/Developer/CommandLineTools/SDKs/MacOSX26.2.sdk";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Sdk {
    root: PathBuf,
    files: Vec<evidence::process::file::Identity>,
}

impl Sdk {
    fn capture() -> Result<Self, Error> {
        let root = fs::canonicalize(OBSERVER_SDK)?;
        let mut files = Vec::new();
        for (relative, expected) in [
            (
                "usr/include/libproc.h",
                "246d87709fc6b9157ce5cf3c475656ac48e0e1ae8bbdc46cf45acd34294448cd",
            ),
            (
                "usr/include/sys/proc_info.h",
                "e427fa96b348537b21552b9de71e01039410bcad2cedee5584c5e5fddafd70fc",
            ),
            (
                "SDKSettings.json",
                "95882a9cea02396e76f3e406a5728269b2a1a11bcd241430a84c9e63d09a6cd3",
            ),
        ] {
            let file = evidence::process::file::Identity::read(&root.join(relative))?;
            if evidence::hex(&file.digest) != expected {
                return Err(Error::Invalid(
                    "observer SDK differs from reviewed native contract",
                ));
            }
            files.push(file);
        }
        Ok(Self { root, files })
    }
}

/// Current files and committed source checked against the retained invocation.
pub(super) struct Verified(Receipt);

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("release build provenance I/O failed")]
    Io(#[from] std::io::Error),
    #[error("release build provenance JSON failed")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Identity(#[from] evidence::Error),
    #[error("release build provenance refused: {0}")]
    Invalid(&'static str),
    #[error("release build command exited with {status}: {output}")]
    Command {
        status: ExitStatus,
        output: Box<str>,
    },
    #[error("release build process observation failed: {0}")]
    Observation(Box<super::Error>),
    #[error("{source}; release build capture also failed: {capture}")]
    Capture {
        source: Box<Error>,
        capture: Box<Error>,
    },
    #[error("{source}; release build cleanup also failed: {cleanup}")]
    Cleanup {
        source: Box<Error>,
        cleanup: Box<super::Error>,
    },
}

impl Kind {
    fn binary(self) -> &'static str {
        if self == Self::Observer {
            "rawr-qualification-observer"
        } else {
            "rawr"
        }
    }

    fn directory(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::Evidence => "evidence",
            Self::Observer => "observer",
        }
    }

    fn features(self) -> &'static [&'static str] {
        match self {
            Self::Ordinary => &["default", "native-runner"],
            Self::Evidence => &["default", "native-runner", "import-baseline"],
            Self::Observer => &["qualification-observer"],
        }
    }
}

impl Source {
    fn label(&self) -> Box<str> {
        format!(
            "{}@{} tracked-diff-sha256={}",
            self.root.display(),
            self.commit,
            evidence::hex(&Sha256::digest([])),
        )
        .into_boxed_str()
    }
}

impl Verified {
    pub(super) fn executable(&self) -> Result<&evidence::process::file::Identity, Error> {
        Ok(&selected(&self.0, self.0.kind.binary())?.file)
    }

    pub(super) fn sdk(&self) -> Option<Sdk> {
        self.0.sdk.clone()
    }

    pub(super) fn report(&self) -> Result<report::Build, Error> {
        let receipt = &self.0;
        let artifact = selected(receipt, receipt.kind.binary())?;
        let file = &artifact.file;
        Ok(report::Build {
            executable_path: std::str::from_utf8(&file.path)
                .map_err(|_| Error::Invalid("executable path is not UTF-8"))?
                .into(),
            executable_digest_sha256: evidence::hex(&file.digest).into_boxed_str(),
            device: file.device,
            inode: file.inode,
            size: file.size,
            source_tree: receipt.source.label(),
            lockfile_digest_sha256: evidence::hex(&receipt.source.lockfile).into_boxed_str(),
            compiler: receipt.compiler.clone(),
            cargo: receipt.cargo.clone(),
            target: receipt.target.clone(),
            profile: receipt.profile.clone(),
            features: features(&artifact.cargo_record)?,
        })
    }

    pub(super) fn cell(&self, executable: &Path) -> Result<cell::Build, Error> {
        if self.0.kind != Kind::Evidence {
            return Err(Error::Invalid(
                "ordinary artifacts cannot enter a cell",
            ));
        }
        // The child repeats this check after authenticating the parent. These
        // current-checkout facts corroborate the receipt; they do not create it.
        let current = crate::evidence::cell_build(executable)
            .map_err(|_| Error::Invalid("current child build corroboration failed"))?;
        if current.executable != selected(&self.0, "rawr")?.file
            || current.source_tree != self.0.source.label()
            || current.lockfile_digest != self.0.source.lockfile
        {
            return Err(Error::Invalid(
                "child build facts differ from the actual build",
            ));
        }
        Ok(current)
    }
}

pub(super) fn artifact(kind: &str) -> Result<PathBuf, Error> {
    let kind = match kind {
        "ordinary" => Kind::Ordinary,
        "evidence" => Kind::Evidence,
        "observer" => Kind::Observer,
        _ => return Err(Error::Invalid("unknown release artifact kind")),
    };
    let root = root()?;
    controlled_target(&root, kind, false)?;
    let receipt = read_receipt(&directory(&root, kind).join(RECEIPT))?;
    if receipt.kind != kind {
        return Err(Error::Invalid(
            "artifact directory and receipt kind disagree",
        ));
    }
    let executable = file_path(&selected(&receipt, kind.binary())?.file);
    verify(receipt, &executable, kind)?;
    Ok(executable)
}

pub(super) fn load(executable: &Path, kind: Kind) -> Result<Verified, Error> {
    let root = root()?;
    controlled_target(&root, kind, false)?;
    let receipt = read_receipt(&directory(&root, kind).join(RECEIPT))?;
    verify(receipt, executable, kind)
}

fn verify(receipt: Receipt, executable: &Path, kind: Kind) -> Result<Verified, Error> {
    let current = source(&root()?)?;
    validate(&receipt, &current, kind)?;
    if file_path(&selected(&receipt, kind.binary())?.file) != executable {
        return Err(Error::Invalid(
            "selected executable is not the emitted Cargo artifact",
        ));
    }
    verify_files(&receipt)?;
    if kind == Kind::Observer && receipt.sdk.as_ref() != Some(&Sdk::capture()?) {
        return Err(Error::Invalid("selected observer SDK custody changed"));
    }
    Ok(Verified(receipt))
}

fn verify_files(receipt: &Receipt) -> Result<(), Error> {
    for artifact in &receipt.artifacts {
        if executable_identity(&file_path(&artifact.file))? != artifact.file {
            return Err(Error::Invalid(
                "an emitted executable changed after compilation",
            ));
        }
    }
    Ok(())
}

fn validate(receipt: &Receipt, current: &Source, kind: Kind) -> Result<(), Error> {
    if receipt.schema.as_ref() != SCHEMA
        || receipt.kind != kind
        || receipt.program.digest == [0; 32]
        || receipt.cargo_executable.digest == [0; 32]
        || receipt.compiler_executable.digest == [0; 32]
        || &receipt.source != current
        || receipt.profile.as_ref() != "release"
        || receipt.target.is_empty()
        || receipt.compiler.is_empty()
        || receipt.cargo.is_empty()
        || receipt.environment_sha256 == [0; 32]
        || receipt.cargo_stdout_sha256 == [0; 32]
        || receipt.build_finished["reason"] != "build-finished"
        || receipt.build_finished["success"] != true
        || receipt.invocation
            != invocation(
                kind,
                &receipt.target,
                &current.root,
                &file_path(&receipt.cargo_executable),
                &file_path(&receipt.compiler_executable),
            )?
    {
        return Err(Error::Invalid(
            "receipt, successful invocation, or committed source mismatch",
        ));
    }
    let rawr = selected(receipt, kind.binary())?;
    if (kind == Kind::Observer) != receipt.sdk.is_some() {
        return Err(Error::Invalid(
            "observer SDK association missing or unexpected",
        ));
    }
    if features(&rawr.cargo_record)?
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<&str>>()
        != kind.features()
    {
        return Err(Error::Invalid(
            "Cargo's actual feature set differs from the selected lane",
        ));
    }
    if kind != Kind::Observer
        && !features(&selected(receipt, "rawr-import-driver")?.cargo_record)?
            .iter()
            .any(|feature| feature.as_ref() == "rustc-driver")
    {
        return Err(Error::Invalid("Cargo omitted the required driver feature"));
    }
    if kind == Kind::Observer
        && (receipt.target.as_ref() != "aarch64-apple-darwin"
            || receipt
                .sdk
                .as_ref()
                .is_none_or(|sdk| sdk.root != Path::new(OBSERVER_SDK)))
    {
        return Err(Error::Invalid(
            "observer SDK/target is outside the reviewed candidate tuple",
        ));
    }
    if kind == Kind::Observer && receipt.artifacts.len() != 1 {
        return Err(Error::Invalid(
            "observer build must contain only its selected executable",
        ));
    }
    for artifact in &receipt.artifacts {
        let record = &artifact.cargo_record;
        let name = record["target"]["name"]
            .as_str()
            .ok_or(Error::Invalid("Cargo target name is missing"))?;
        let manifest = match name {
            "rawr" | "rawr-qualification-observer" => "cli/Cargo.toml",
            "rawr-import-driver" => "importer/Cargo.toml",
            _ => return Err(Error::Invalid("unexpected selected binary")),
        };
        let expected = directory(&current.root, kind)
            .join(receipt.target.as_ref())
            .join("release")
            .join(name);
        if record["reason"] != "compiler-artifact"
            || record["target"]["kind"] != serde_json::json!(["bin"])
            || record["manifest_path"] != current.root.join(manifest).to_string_lossy().as_ref()
            || record["executable"] != expected.to_string_lossy().as_ref()
            || file_path(&artifact.file) != expected
            || !record["fresh"].is_boolean()
            || record["profile"]["test"] != false
            || record["profile"]["opt_level"] != "3"
            || record["profile"]["debug_assertions"] != false
        {
            return Err(Error::Invalid(
                "Cargo artifact path, target, reuse fact, or release profile differs",
            ));
        }
    }
    Ok(())
}

fn selected<'a>(receipt: &'a Receipt, name: &str) -> Result<&'a Artifact, Error> {
    let mut artifacts = receipt
        .artifacts
        .iter()
        .filter(|artifact| artifact.cargo_record["target"]["name"] == name);
    let artifact = artifacts
        .next()
        .ok_or(Error::Invalid("Cargo omitted a required executable"))?;
    if artifacts.next().is_some() {
        return Err(Error::Invalid(
            "Cargo emitted duplicate selected executables",
        ));
    }
    Ok(artifact)
}

fn features(record: &serde_json::Value) -> Result<Box<[Box<str>]>, Error> {
    let mut values = record["features"]
        .as_array()
        .ok_or(Error::Invalid("Cargo feature list is missing"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(Box::<str>::from)
                .ok_or(Error::Invalid("Cargo feature is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::Invalid("Cargo feature list contains duplicates"));
    }
    Ok(values.into_boxed_slice())
}

fn root() -> Result<PathBuf, Error> {
    Ok(fs::canonicalize(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".."),
    )?)
}

fn directory(root: &Path, kind: Kind) -> PathBuf {
    root.join("target/phase9-01-build").join(kind.directory())
}

fn controlled_target(root: &Path, kind: Kind, create: bool) -> Result<(), Error> {
    let mut path = root.to_path_buf();
    for component in ["target", "phase9-01-build", kind.directory()] {
        path.push(component);
        if create {
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(Error::Io(error)),
            }
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != nix::unistd::geteuid().as_raw()
            || metadata.permissions().mode() & 0o022 != 0
        {
            return Err(Error::Invalid(
                "dedicated build target is not a controlled ordinary directory",
            ));
        }
    }
    Ok(())
}

fn rustup() -> Result<evidence::process::file::Identity, Error> {
    let path = std::env::var_os("PATH").ok_or(Error::Invalid("PATH does not locate rustup"))?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("rustup");
        if fs::metadata(&candidate)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        {
            return executable_identity(&fs::canonicalize(candidate)?);
        }
    }
    Err(Error::Invalid("PATH does not locate an executable rustup"))
}

fn file_path(identity: &evidence::process::file::Identity) -> PathBuf {
    Path::new(std::ffi::OsStr::from_bytes(&identity.path)).to_path_buf()
}

fn executable_identity(path: &Path) -> Result<evidence::process::file::Identity, Error> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(Error::Invalid(
            "emitted binary is not an executable regular file",
        ));
    }
    Ok(evidence::process::file::Identity::read(path)?)
}

fn read_receipt(path: &Path) -> Result<Receipt, Error> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(Error::Invalid("build receipt is not a regular file"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(REPORT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > REPORT_BYTES {
        return Err(Error::Invalid("receipt exceeds the existing report bound"));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn source(root: &Path) -> Result<Source, Error> {
    let git = |arguments: &[&str]| -> Result<Box<str>, Error> {
        let mut command = Command::new("git");
        command.current_dir(root).args(arguments);
        text(output(command, super::OUTPUT_BYTES)?)
    };
    let commit = git(&["rev-parse", "HEAD"])?;
    let tree = git(&["rev-parse", "HEAD^{tree}"])?;
    let mut status = Command::new("git");
    status
        .current_dir(root)
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ])
        .args(EXCLUDES);
    if !output(status, super::OUTPUT_BYTES)?.is_empty() {
        return Err(Error::Invalid(
            "release provenance requires a committed candidate with no unexpected untracked files",
        ));
    }
    let lockfile = Sha256::digest(fs::read(root.join("Cargo.lock"))?).into();
    if git(&["rev-parse", "HEAD"])? != commit {
        return Err(Error::Invalid("source commit changed during capture"));
    }
    Ok(Source {
        root: root.to_path_buf(),
        commit,
        tree,
        lockfile,
    })
}

fn invocation(
    kind: Kind,
    target: &str,
    root: &Path,
    cargo: &Path,
    compiler: &Path,
) -> Result<Vec<OsString>, Error> {
    let mut arguments = vec![
        OsString::from("run"),
        OsString::from(TOOLCHAIN),
        cargo.as_os_str().to_owned(),
    ];
    arguments.extend(
        [
            "build",
            "--locked",
            "--release",
            "--message-format=json",
            "--target",
            target,
            "--target-dir",
        ]
        .map(OsString::from),
    );
    arguments.push(directory(root, kind).into_os_string());
    if kind == Kind::Observer {
        arguments.extend(
            [
                "-p",
                "rawr-cli",
                "--bin",
                "rawr-qualification-observer",
                "--no-default-features",
                "--features",
                "rawr-cli/qualification-observer",
            ]
            .map(OsString::from),
        );
        let sdk = serde_json::to_string(OBSERVER_SDK)?;
        let flags = serde_json::to_string(&format!("-C link-arg=-Wl,-syslibroot,{OBSERVER_SDK}"))?;
        for setting in [
            format!("env.SDKROOT={{value={sdk},force=true}}"),
            format!("env.RUSTFLAGS={{value={flags},force=true}}"),
            format!(
                "env.CARGO_ENCODED_RUSTFLAGS={{value={},force=true}}",
                serde_json::to_string(&format!("-C\u{1f}link-arg=-Wl,-syslibroot,{OBSERVER_SDK}"))?
            ),
        ] {
            arguments.push("--config".into());
            arguments.push(setting.into());
        }
    } else {
        arguments.extend(["-p", "rawr-cli", "-p", "rawr-import", "--bins"].map(OsString::from));
    }
    if kind == Kind::Evidence {
        arguments.extend(["--features", "rawr-cli/import-baseline"].map(OsString::from));
    }
    let compiler = serde_json::to_string(
        compiler
            .to_str()
            .ok_or(Error::Invalid("compiler path is not UTF-8"))?,
    )?;
    for setting in [
        format!("build.rustc={compiler}"),
        "build.rustc-wrapper=\"\"".to_owned(),
        "build.rustc-workspace-wrapper=\"\"".to_owned(),
        format!("env.RUSTC={{value={compiler},force=true}}"),
        "env.RUSTC_WRAPPER={value=\"\",force=true}".to_owned(),
        "env.RUSTC_WORKSPACE_WRAPPER={value=\"\",force=true}".to_owned(),
    ] {
        arguments.push("--config".into());
        arguments.push(setting.into());
    }
    Ok(arguments)
}

/// The only receipt producer: it owns the actual command and observes its exit.
fn produce(kind: Kind) -> Result<(), Error> {
    let root = root()?;
    let before = source(&root)?;
    let sdk = if kind == Kind::Observer {
        Some(Sdk::capture()?)
    } else {
        None
    };
    let program = rustup()?;
    let mut environment: Vec<_> = std::env::vars_os().collect();
    let which = |tool: &str| -> Result<evidence::process::file::Identity, Error> {
        let mut command = Command::new(file_path(&program));
        command
            .current_dir(&root)
            .env_clear()
            .envs(environment.iter().cloned())
            .args(["which", "--toolchain", TOOLCHAIN, tool]);
        let path = text(output(command, super::OUTPUT_BYTES)?)?;
        executable_identity(&fs::canonicalize(Path::new(path.as_ref()))?)
    };
    let cargo_executable = which("cargo")?;
    let compiler_executable = which("rustc")?;
    let compiler_path = file_path(&compiler_executable);
    // Explicit selections also reach build scripts. Command-line Cargo config
    // below supersedes user config, including its [env] force entries.
    environment.retain(|(key, _)| {
        !matches!(
            key.to_str(),
            Some("RUSTC" | "RUSTC_WRAPPER" | "RUSTC_WORKSPACE_WRAPPER")
        )
    });
    environment.extend([
        (
            OsString::from("RUSTC"),
            compiler_path.as_os_str().to_owned(),
        ),
        (OsString::from("RUSTC_WRAPPER"), OsString::new()),
        (OsString::from("RUSTC_WORKSPACE_WRAPPER"), OsString::new()),
    ]);
    if kind == Kind::Observer {
        environment.retain(|(key, _)| {
            !matches!(
                key.to_str(),
                Some("SDKROOT" | "RUSTFLAGS" | "CARGO_ENCODED_RUSTFLAGS")
            )
        });
        environment.extend([
            (OsString::from("SDKROOT"), OsString::from(OBSERVER_SDK)),
            (
                OsString::from("RUSTFLAGS"),
                OsString::from(format!("-C link-arg=-Wl,-syslibroot,{OBSERVER_SDK}")),
            ),
            (
                OsString::from("CARGO_ENCODED_RUSTFLAGS"),
                OsString::from(format!("-C\u{1f}link-arg=-Wl,-syslibroot,{OBSERVER_SDK}")),
            ),
        ]);
    }
    environment.sort();
    let mut digest = Sha256::new();
    for (key, value) in &environment {
        for bytes in [key.as_bytes(), value.as_bytes()] {
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        }
    }
    let metadata = |tool: &evidence::process::file::Identity| -> Result<Box<str>, Error> {
        let mut command = Command::new(file_path(&program));
        command
            .current_dir(&root)
            .env_clear()
            .envs(environment.iter().cloned())
            .args(["run", TOOLCHAIN])
            .arg(file_path(tool))
            .arg("-Vv");
        text(output(command, super::OUTPUT_BYTES)?)
    };
    let compiler = metadata(&compiler_executable)?;
    let cargo = metadata(&cargo_executable)?;
    let target: Box<str> = compiler
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .filter(|target| !target.is_empty())
        .ok_or(Error::Invalid("compiler omitted its host target"))?
        .into();
    let invocation = invocation(
        kind,
        &target,
        &root,
        &file_path(&cargo_executable),
        &compiler_path,
    )?;
    let directory = directory(&root, kind);
    // Keep useful Cargo caches. Reuse is justified by this actual invocation's
    // artifact records, never by the prior receipt or an existing filename.
    controlled_target(&root, kind, true)?;
    // A failed attempt cannot leave an earlier successful receipt eligible.
    match fs::remove_file(directory.join(RECEIPT)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::Io(error)),
    }
    let mut command = Command::new(file_path(&program));
    command
        .current_dir(&root)
        .env_clear()
        .envs(environment.iter().cloned())
        .args(&invocation);
    let stdout = output(command, REPORT_BYTES)?;
    let (records, build_finished) = messages(&stdout)?;
    let after = source(&root)?;
    if kind == Kind::Observer && sdk.as_ref() != Some(&Sdk::capture()?) {
        return Err(Error::Invalid("observer SDK changed during build"));
    }
    if before != after
        || metadata(&compiler_executable)? != compiler
        || metadata(&cargo_executable)? != cargo
        || [&program, &cargo_executable, &compiler_executable]
            .into_iter()
            .any(|tool| executable_identity(&file_path(tool)).as_ref().ok() != Some(tool))
    {
        return Err(Error::Invalid(
            "source or toolchain changed during the build",
        ));
    }
    let artifacts = records
        .into_iter()
        .map(|cargo_record| {
            let path = cargo_record["executable"]
                .as_str()
                .ok_or(Error::Invalid("Cargo omitted its executable path"))?;
            let file = executable_identity(Path::new(path))?;
            Ok(Artifact { cargo_record, file })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let receipt = Receipt {
        schema: SCHEMA.into(),
        kind,
        source: before,
        program,
        cargo_executable,
        compiler_executable,
        invocation,
        environment_sha256: digest.finalize().into(),
        compiler,
        cargo,
        target,
        profile: "release".into(),
        cargo_stdout_sha256: Sha256::digest(&stdout).into(),
        build_finished,
        artifacts,
        sdk,
    };
    validate(&receipt, &after, kind)?;
    let bytes = serde_json::to_vec(&receipt)?;
    if bytes.len() > REPORT_BYTES {
        return Err(Error::Invalid("receipt exceeds the existing report bound"));
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join(RECEIPT))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn messages(stdout: &[u8]) -> Result<(Vec<serde_json::Value>, serde_json::Value), Error> {
    let mut artifacts = Vec::new();
    let mut finished = None;
    for line in stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if line.len() > super::OUTPUT_BYTES {
            return Err(Error::Invalid(
                "Cargo record exceeds the existing output bound",
            ));
        }
        let record: serde_json::Value = serde_json::from_slice(line)?;
        if finished.is_some() {
            return Err(Error::Invalid("Cargo output trails build-finished"));
        }
        match record["reason"].as_str() {
            Some("compiler-artifact")
                if matches!(
                    record["target"]["name"].as_str(),
                    Some("rawr" | "rawr-import-driver" | "rawr-qualification-observer")
                ) =>
            {
                artifacts.push(record)
            }
            Some("build-finished") => finished = Some(record),
            _ => {}
        }
    }
    Ok((
        artifacts,
        finished.ok_or(Error::Invalid("Cargo omitted build-finished"))?,
    ))
}

fn text(bytes: Vec<u8>) -> Result<Box<str>, Error> {
    let text =
        String::from_utf8(bytes).map_err(|_| Error::Invalid("command output is not UTF-8"))?;
    if text.trim().is_empty() {
        return Err(Error::Invalid("command output is empty"));
    }
    Ok(text.trim().into())
}

// A local build capture, using the harness's existing process-group owner and
// deadline. Nonblocking stdout cannot strand the owner on a parser/read error.
fn output(mut command: Command, limit: usize) -> Result<Vec<u8>, Error> {
    let (mut reader, writer) = UnixStream::pair()?;
    reader.set_nonblocking(true)?;
    let writer: OwnedFd = writer.into();
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::inherit())
        .process_group(0);
    let mut child = {
        let mut owned = command;
        owned.spawn()?
    };
    let id = child.id();
    let observer = match super::ProcessObserver::transform(id) {
        Ok(observer) => observer,
        Err(original) => {
            let mut failure = Error::Observation(Box::new(original));
            if let Err(cleanup) = super::terminate_cell(&mut child, BTreeSet::from([id])) {
                failure = Error::Cleanup {
                    source: Box::new(failure),
                    cleanup: Box::new(cleanup),
                };
            }
            return Err(failure);
        }
    };
    let deadline = Instant::now() + Duration::from_secs(30 * 60);
    let mut bytes = Vec::new();
    let mut status = None;
    let mut eof = false;
    let result = (|| {
        loop {
            let mut chunk = [0; 8192];
            match reader.read(&mut chunk) {
                Ok(0) => eof = true,
                Ok(count) if bytes.len().saturating_add(count) <= limit => {
                    bytes.extend_from_slice(&chunk[..count])
                }
                Ok(_) => return Err(Error::Invalid("command exceeded its existing output bound")),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(Error::Io(error)),
            }
            if status.is_none() {
                status = child.try_wait()?;
            }
            let (alive, failure, _) = observer.state();
            if let Some(reason) = failure {
                return Err(Error::Observation(Box::new(super::Error::Process {
                    reason,
                })));
            }
            if status.is_some() && eof && !alive {
                break;
            }
            if Instant::now() >= deadline {
                return Err(Error::Invalid(
                    "build command exceeded the existing thirty-minute deadline",
                ));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        status.ok_or(Error::Invalid("completed command lost its status"))
    })();
    match captured(status, bytes, result) {
        Ok(bytes) => {
            observer
                .finish()
                .map_err(|error| Error::Observation(Box::new(error)))?;
            Ok(bytes)
        }
        Err(original) => {
            let cleanup = super::terminate_cell(&mut child, observer.state().2).map(|_| ());
            let retired = observer.wait_retired();
            let observed = observer.finish().map(|_| ());
            Err([cleanup, retired, observed]
                .into_iter()
                .fold(original, |source, result| match result {
                    Ok(()) => source,
                    Err(cleanup) => Error::Cleanup {
                        source: Box::new(source),
                        cleanup: Box::new(cleanup),
                    },
                }))
        }
    }
}

fn successful(status: ExitStatus, bytes: Vec<u8>) -> Result<Vec<u8>, Error> {
    if !status.success() {
        return Err(command_failure(status, &bytes));
    }
    Ok(bytes)
}

fn captured(
    status: Option<ExitStatus>,
    bytes: Vec<u8>,
    result: Result<ExitStatus, Error>,
) -> Result<Vec<u8>, Error> {
    match result {
        Ok(status) => successful(status, bytes),
        Err(capture) => match status {
            Some(status) if !status.success() => Err(Error::Capture {
                source: Box::new(command_failure(status, &bytes)),
                capture: Box::new(capture),
            }),
            _ => Err(capture),
        },
    }
}

fn command_failure(status: ExitStatus, bytes: &[u8]) -> Error {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(super::OUTPUT_BYTES)]);
    let mut end = text.len().min(super::OUTPUT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Error::Command {
        status,
        output: text[..end].into(),
    }
}

#[test]
#[ignore = "explicitly authorized producer for a committed ordinary release candidate"]
fn build_ordinary_release_with_provenance() {
    produce(Kind::Ordinary).expect("actual ordinary release build produces provenance");
}

#[test]
#[ignore = "explicitly authorized producer for a committed evidence release candidate"]
fn build_evidence_release_with_provenance() {
    produce(Kind::Evidence).expect("actual evidence release build produces provenance");
}

#[test]
#[ignore = "explicitly authorized private observer build for the same frozen candidate"]
fn build_observer_release_with_provenance() {
    produce(Kind::Observer).expect("actual private observer build produces provenance");
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::process::ExitStatusExt as _;
    use std::path::PathBuf;
    use std::process::ExitStatus;
    use std::sync::atomic::Ordering;

    use rawr_import::metrics::evidence;

    use super::{
        captured, directory, executable_identity, file_path, invocation, messages, successful,
        validate, verify_files, Artifact, Error, Kind, Receipt, Source, SCHEMA,
    };

    fn receipt() -> Receipt {
        let source = Source {
            root: PathBuf::from("/candidate"),
            commit: "committed-candidate".into(),
            tree: "committed-tree".into(),
            lockfile: [1; 32],
        };
        let kind = Kind::Evidence;
        let target = "aarch64-apple-darwin";
        let artifacts = [
            ("rawr", "cli/Cargo.toml", kind.features()),
            (
                "rawr-import-driver",
                "importer/Cargo.toml",
                &["rustc-driver"][..],
            ),
        ]
        .into_iter()
        .map(|(name, manifest, features)| {
            let path = directory(&source.root, kind)
                .join(target)
                .join("release")
                .join(name);
            Artifact {
                cargo_record: serde_json::json!({
                    "reason": "compiler-artifact", "manifest_path": source.root.join(manifest),
                    "target": { "name": name, "kind": ["bin"] },
                    "executable": path, "fresh": false, "features": features,
                    "profile": { "test": false, "opt_level": "3", "debug_assertions": false }
                }),
                file: evidence::process::file::Identity {
                    path: path.as_os_str().as_bytes().into(),
                    digest: [2; 32],
                    device: 1,
                    inode: 2,
                    size: 3,
                },
            }
        })
        .collect();
        let tool = |name: &str| evidence::process::file::Identity {
            path: format!("/toolchain/{name}").into_bytes().into_boxed_slice(),
            digest: [5; 32],
            device: 1,
            inode: 1,
            size: 1,
        };
        let program = tool("rustup");
        let cargo_executable = tool("cargo");
        let compiler_executable = tool("rustc");
        Receipt {
            schema: SCHEMA.into(),
            kind,
            invocation: invocation(
                kind,
                target,
                &source.root,
                &file_path(&cargo_executable),
                &file_path(&compiler_executable),
            )
            .expect("fixed invocation seals"),
            source,
            program,
            cargo_executable,
            compiler_executable,
            environment_sha256: [3; 32],
            compiler: "rustc observed".into(),
            cargo: "cargo observed".into(),
            target: target.into(),
            profile: "release".into(),
            cargo_stdout_sha256: [4; 32],
            build_finished: serde_json::json!({"reason": "build-finished", "success": true}),
            artifacts,
            sdk: None,
        }
    }

    #[test]
    fn observer_invocation_selects_only_its_private_target_and_reviewed_sdk() {
        let args = invocation(
            Kind::Observer,
            "aarch64-apple-darwin",
            std::path::Path::new("/candidate"),
            std::path::Path::new("/cargo"),
            std::path::Path::new("/rustc"),
        )
        .unwrap();
        let text: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        assert!(text
            .windows(2)
            .any(|pair| pair[0] == "--bin" && pair[1] == "rawr-qualification-observer"));
        assert!(text.iter().any(|arg| arg == "--no-default-features"));
        assert!(!text
            .iter()
            .any(|arg| arg == "--bins" || arg == "rawr-cli/import-baseline"));
        assert!(text
            .iter()
            .any(|arg| arg.starts_with("env.SDKROOT=") && arg.contains(super::OBSERVER_SDK)));
        assert!(text
            .iter()
            .any(|arg| arg.starts_with("env.CARGO_ENCODED_RUSTFLAGS=")
                && arg.contains("-syslibroot,")));
    }

    #[test]
    fn source_features_driver_and_success_are_required_facts() {
        let original = receipt();
        validate(&original, &original.source, Kind::Evidence)
            .expect("matching actual facts validate");
        let mut changed_source = original.source.clone();
        changed_source.tree = "new-committed-tree".into();
        assert!(validate(&original, &changed_source, Kind::Evidence).is_err());
        changed_source = original.source.clone();
        changed_source.lockfile = [9; 32];
        assert!(validate(&original, &changed_source, Kind::Evidence).is_err());
        for change in 0..6 {
            let mut changed = original.clone();
            match change {
                0 => {
                    changed.artifacts[0].cargo_record["features"] =
                        serde_json::json!(["default", "native-runner"])
                }
                1 => {
                    changed.artifacts.pop();
                }
                2 => changed.build_finished["success"] = serde_json::json!(false),
                3 => changed.artifacts[0].cargo_record["fresh"] = serde_json::Value::Null,
                4 => changed.invocation.push("--no-default-features".into()),
                _ => {
                    changed.artifacts[1].cargo_record["executable"] =
                        serde_json::json!("/other/driver")
                }
            }
            assert!(
                validate(&changed, &original.source, Kind::Evidence).is_err(),
                "changed fact {change} must refuse"
            );
        }
        assert!(validate(&original, &original.source, Kind::Ordinary).is_err());
        let mut reused = original.clone();
        reused.artifacts[0].cargo_record["fresh"] = serde_json::json!(true);
        validate(&reused, &original.source, Kind::Evidence)
            .expect("actual Cargo reuse remains valid provenance");
    }

    #[test]
    fn changed_rawr_driver_and_nonexecutable_files_are_rejected() {
        let directory = std::env::temp_dir().join(format!(
            "rawr-build-provenance-{}-{}",
            std::process::id(),
            super::super::NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).expect("exclusive test directory");
        let mut receipt = receipt();
        for (index, artifact) in receipt.artifacts.iter_mut().enumerate() {
            let path = directory.join(index.to_string());
            fs::write(&path, b"compiled artifact").expect("test artifact writes");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("test artifact is executable");
            artifact.file = executable_identity(&path).expect("test identity seals");
        }
        verify_files(&receipt).expect("unchanged files agree");
        for index in 0..receipt.artifacts.len() {
            let path = file_path(&receipt.artifacts[index].file);
            fs::write(&path, b"stale replacement").expect("test artifact changes");
            assert!(verify_files(&receipt).is_err());
            fs::write(&path, b"compiled artifact").expect("original bytes return");
        }
        let driver = file_path(&receipt.artifacts[1].file);
        fs::set_permissions(&driver, fs::Permissions::from_mode(0o600))
            .expect("test driver loses executable mode");
        assert!(verify_files(&receipt).is_err());
        fs::remove_file(driver).expect("test driver removed");
        assert!(verify_files(&receipt).is_err());
        fs::remove_dir_all(directory).expect("test directory retires");
    }

    #[test]
    fn failed_command_status_precedes_json_and_receipt_failures() {
        let status = ExitStatus::from_raw(23 << 8);
        let result = successful(status, b"malformed Cargo output".to_vec())
            .and_then(|bytes| messages(&bytes));
        assert!(matches!(result, Err(Error::Command { status, .. }) if status.code() == Some(23)));
        assert!(messages(b"{\"reason\":\"build-finished\",\"success\":true}\n{}\n").is_err());
        assert!(messages(b"{\"reason\":\"compiler-artifact\"}\n").is_err());
    }

    #[test]
    fn observed_command_failure_survives_later_capture_failures() {
        let status = ExitStatus::from_raw(23 << 8);
        for failure in [
            Error::Invalid("EOF deadline elapsed"),
            Error::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe)),
            Error::Observation(Box::new(super::super::Error::Process {
                reason: "observer refused a process row".into(),
            })),
        ] {
            let reason = failure.to_string();
            let result = captured(
                Some(status),
                vec![0xff; super::super::OUTPUT_BYTES],
                Err(failure),
            );
            let Error::Capture { source, capture } = result.expect_err("capture failure refuses")
            else {
                panic!("known command failure must remain primary");
            };
            assert_eq!(capture.to_string(), reason);
            assert!(matches!(*source, Error::Command { status, output }
                if status.code() == Some(23) && output.len() <= super::super::OUTPUT_BYTES));
        }
        for status in [None, Some(ExitStatus::from_raw(0))] {
            assert!(matches!(
                captured(status, Vec::new(), Err(Error::Invalid("capture failed"))),
                Err(Error::Invalid("capture failed"))
            ));
        }
    }
}
