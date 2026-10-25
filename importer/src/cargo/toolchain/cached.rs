use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Executable, Proof, Requirement, Schemas};
use crate::node::{self, Value as _};
use crate::{context::Context, metrics, process};

const STATE_SCHEMA: u32 = 1;
const DRIVER_SCHEMA: u32 = 1;
const NO_DRIVER_SCHEMA: u32 = 2;

/// Capability-free pinned-toolchain facts accepted only through node validation.
pub struct Attestation {
    proof: Proof,
    target_directory: PathBuf,
    state: Box<[FileEvidence]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileEvidence {
    path: PathBuf,
    length: u64,
    digest: [u8; 32],
}

/// One toolchain capture or cached-toolchain revalidation account. The
/// state path-byte and file-count accounting targets crossed while the
/// installation state is read are recorded here and published under
/// `Account::Toolchain`; every state file is still retained and nothing
/// refuses. Physical bounds (the node content ceiling, checked arithmetic,
/// `try_reserve`) keep refusing.
pub(crate) struct ToolchainAccount {
    id: u64,
    exceeded: crate::exceedance::Record,
}

impl ToolchainAccount {
    pub(crate) fn new() -> Self {
        Self {
            id: crate::exceedance::next_id(),
            exceeded: crate::exceedance::Record::new(),
        }
    }

    /// The axes of this account that crossed their targets so far.
    #[cfg(test)]
    pub(crate) const fn exceedances(&self) -> crate::exceedance::Record {
        self.exceeded
    }

    /// Record one measured amount against its accounting target. A crossing
    /// is published with a truthful high water and the caller continues.
    fn exceeded(&mut self, resource: &'static str, target: usize, actual: usize) {
        if actual > target
            && self
                .exceeded
                .exceeded(resource, target as u64, actual as u64, None)
        {
            crate::exceedance::publish(
                crate::exceedance::Account::Toolchain,
                self.id,
                &self.exceeded,
            );
        }
    }
}

impl Default for ToolchainAccount {
    fn default() -> Self {
        Self::new()
    }
}

/// Evaluate the first real discoverable DAG node under one sealed context.
pub fn cached(
    context: &Context,
    sink: &dyn metrics::Sink,
) -> Result<Arc<node::Validated<Attestation>>, node::Error> {
    if let Some(toolchain) = context.cached_toolchain() {
        return Ok(toolchain);
    }
    let limits = context.limits();
    let store = context.open_store()?;
    let selected = process::Native::selected_in(
        context.project(),
        context.environment(),
        context.graph_budget(),
    )
    .map_err(super::evidence_error)?;
    let recipe = context.discover::<Attestation>(&recipe(&selected), limits.node_output_bytes())?;
    let toolchain = node::Evaluator::new(&store, sink).discover_budgeted(
        &recipe,
        move |candidate, budget| candidate.revalidate(&selected, &limits, budget),
        || Attestation::discover(context, &limits),
    )?;
    Ok(context.retain_toolchain(Arc::new(toolchain)))
}

impl Attestation {
    #[cfg(test)]
    pub(crate) fn fixture_replay(context: &Context) {
        let limits = context.limits();
        let store = context.open_store().expect("fixture replay store");
        let recipe = context
            .discover::<Self>(&fixed_recipe(), limits.node_output_bytes())
            .expect("fixture replay recipe");
        let validated = node::Evaluator::new(&store, &metrics::Noop)
            .discover_budgeted(
                &recipe,
                move |value, budget| value.revalidate_files_budgeted(&limits, Some(budget)),
                || {
                    Err(node::Error::State(
                        "fixture toolchain replay unexpectedly missed",
                    ))
                },
            )
            .expect("fixture toolchain revalidates in its new graph without a child");
        context.retain_toolchain(Arc::new(validated));
    }

    #[cfg(test)]
    pub(crate) fn fixture_current(context: &Context, root: &Path) {
        let attestation = tests::fixture_attestation(root);
        let limits = context.limits();
        let store = context.open_store().expect("the fixture store opens");
        let recipe = context
            .discover::<Self>(&fixed_recipe(), limits.node_output_bytes())
            .unwrap();
        let validated = node::Evaluator::new(&store, &metrics::Noop)
            .discover_budgeted(
                &recipe,
                move |value, budget| value.revalidate_files_budgeted(&limits, Some(budget)),
                || Ok(attestation),
            )
            .expect("the fixture toolchain validates without a child");
        context.retain_toolchain(Arc::new(validated));
    }

    fn discover(context: &Context, limits: &crate::cargo::Limits) -> Result<Self, node::Error> {
        let budget = context.graph_budget();
        let native = process::Native::pinned_in(
            context.project(),
            context.environment(),
            limits.process_limits(),
            budget,
        )
        .map_err(super::evidence_error)?;
        let environment = context
            .environment()
            .entries()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let proof = Requirement::pinned()
            .verify_in(
                &native.production(),
                context.project(),
                environment,
                limits.process_limits(),
                context.target(),
                Some(budget),
            )
            .map_err(super::evidence_error)?;
        Self::capture(proof, limits, Some(budget))
    }

    fn capture(
        proof: Proof,
        limits: &crate::cargo::Limits,
        budget: Option<&node::GraphBudget>,
    ) -> Result<Self, node::Error> {
        let target_directory = target_directory(&proof.rustc.path, proof.target())?;
        validate_directory("toolchain target", &target_directory, budget)?;
        // One toolchain account per capture records the state targets
        // this capture crosses; nothing here refuses on one.
        let state = state_files_with_budget(
            &proof.rustc.path,
            limits,
            budget,
            &mut ToolchainAccount::new(),
        )?;
        let attestation = Self {
            proof,
            target_directory,
            state,
        };
        attestation.validate_fixed_in(budget)?;
        Ok(attestation)
    }

    fn revalidate(
        &self,
        selected: &process::Native,
        limits: &crate::cargo::Limits,
        budget: &node::GraphBudget,
    ) -> Result<Box<[u8]>, node::Error> {
        // The selection is part of the key; a record under it naming other
        // paths is damaged or tampered evidence, not an ordinary change.
        if process::Native::from_toolchain(&self.proof) != *selected {
            return Err(super::evidence_error(crate::Error::ResolutionDrift {
                command: "toolchain node validation",
                field: "pinned toolchain selector",
            }));
        }
        self.revalidate_files_budgeted(limits, Some(budget))
    }

    #[cfg(test)]
    fn revalidate_files(&self, limits: &crate::cargo::Limits) -> Result<Box<[u8]>, node::Error> {
        self.revalidate_files_budgeted(limits, None)
    }

    fn revalidate_files_budgeted(
        &self,
        limits: &crate::cargo::Limits,
        budget: Option<&node::GraphBudget>,
    ) -> Result<Box<[u8]>, node::Error> {
        self.validate_fixed_in(budget)?;
        validate_directory("toolchain target", &self.target_directory, budget)?;
        for expected in self.executables() {
            let actual = evidence_with_budget("toolchain executable", &expected.path, budget)?;
            if actual != expected {
                return Err(node::Error::Codec("toolchain executable evidence changed"));
            }
        }
        if state_files_with_budget(
            &self.proof.rustc.path,
            limits,
            budget,
            &mut ToolchainAccount::new(),
        )?
        .as_ref()
            != self.state.as_ref()
        {
            return Err(node::Error::Codec("toolchain installation state changed"));
        }
        self.encode(limits.node_output_bytes())
    }

    fn validate_fixed(&self) -> Result<(), node::Error> {
        let fixed = self.proof.channel.as_ref() == super::CHANNEL
            && self.proof.rustc_release.as_ref() == super::RUST_RELEASE
            && self.proof.rustc_commit.as_ref() == super::RUST_COMMIT
            && self.proof.rustdoc_release.as_ref() == super::RUST_RELEASE
            && self.proof.rustdoc_commit.as_ref() == super::RUST_COMMIT
            && self.proof.cargo_release.as_ref() == super::CARGO_RELEASE
            && self.proof.cargo_commit.as_ref() == super::CARGO_COMMIT
            && self.proof.host.as_ref() == super::HOST
            && self.proof.target.as_ref() == super::HOST
            && self.proof.components.as_ref()
                == super::REQUIRED_COMPONENTS.map(Box::<str>::from).as_slice()
            && self.proof.schemas == Schemas::current();
        if !fixed {
            return Err(node::Error::Codec("toolchain fixed support facts changed"));
        }
        if target_directory(&self.proof.rustc.path, self.proof.target())? != self.target_directory {
            return Err(node::Error::Codec(
                "toolchain target-directory identity changed",
            ));
        }
        if self.state.is_empty()
            || self
                .state
                .windows(2)
                .any(|pair| pair[0].path >= pair[1].path)
        {
            return Err(node::Error::Codec("toolchain state-file ordering changed"));
        }
        Ok(())
    }

    fn validate_fixed_in(&self, budget: Option<&node::GraphBudget>) -> Result<(), node::Error> {
        self.validate_fixed()?;
        // Historical decoding validates only the encoded shape. Actual capture
        // and reuse retain both current directory observations in their account.
        let derived = target_directory(&self.proof.rustc.path, self.proof.target())?;
        validate_directory("toolchain target", &derived, budget)?;
        validate_directory("toolchain target", &self.target_directory, budget)
    }

    fn executables(&self) -> impl Iterator<Item = FileEvidence> {
        let base = [
            FileEvidence::borrow(&self.proof.rustup),
            FileEvidence::borrow(&self.proof.cargo),
            FileEvidence::borrow(&self.proof.rustc),
            FileEvidence::borrow(&self.proof.rustdoc),
        ];
        base.into_iter().chain(
            #[cfg(feature = "rustc-driver")]
            std::iter::once(FileEvidence::borrow(&self.proof.driver)),
            #[cfg(not(feature = "rustc-driver"))]
            std::iter::empty(),
        )
    }

    pub(crate) const fn proof(&self) -> &Proof {
        &self.proof
    }
}

impl node::Value for Attestation {
    const KIND: node::Kind = node::Kind::Toolchain;
    const SCHEMA: u32 = if cfg!(feature = "rustc-driver") {
        DRIVER_SCHEMA
    } else {
        NO_DRIVER_SCHEMA
    };

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        let mut writer = Writer::new(limit);
        for value in [
            self.proof.channel.as_ref(),
            self.proof.rustc_release.as_ref(),
            self.proof.rustc_commit.as_ref(),
            self.proof.rustdoc_release.as_ref(),
            self.proof.rustdoc_commit.as_ref(),
            self.proof.cargo_release.as_ref(),
            self.proof.cargo_commit.as_ref(),
            self.proof.host.as_ref(),
            self.proof.target.as_ref(),
        ] {
            writer.string(value)?;
        }
        writer.u32(self.proof.components.len())?;
        for component in &self.proof.components {
            writer.string(component)?;
        }
        writer.u32_value(self.proof.schemas.probe)?;
        writer.u32_value(self.proof.schemas.unit)?;
        writer.u32_value(self.proof.schemas.graph)?;
        writer.file(&FileEvidence::borrow(&self.proof.rustup))?;
        writer.file(&FileEvidence::borrow(&self.proof.cargo))?;
        writer.file(&FileEvidence::borrow(&self.proof.rustc))?;
        writer.file(&FileEvidence::borrow(&self.proof.rustdoc))?;
        #[cfg(feature = "rustc-driver")]
        writer.file(&FileEvidence::borrow(&self.proof.driver))?;
        writer.path(&self.target_directory)?;
        writer.u32(self.state.len())?;
        for state in &self.state {
            writer.file(state)?;
        }
        Ok(writer.finish())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        let mut reader = Reader::new(bytes, limit)?;
        let channel = reader.string()?;
        let rustc_release = reader.string()?;
        let rustc_commit = reader.string()?;
        let rustdoc_release = reader.string()?;
        let rustdoc_commit = reader.string()?;
        let cargo_release = reader.string()?;
        let cargo_commit = reader.string()?;
        let host = reader.string()?;
        let target = reader.string()?;
        let components = reader.strings()?;
        let schemas = Schemas {
            probe: reader.u32()?,
            unit: reader.u32()?,
            graph: reader.u32()?,
        };
        let rustup = reader.executable()?;
        let cargo = reader.executable()?;
        let rustc = reader.executable()?;
        let rustdoc = reader.executable()?;
        #[cfg(feature = "rustc-driver")]
        let driver = reader.executable()?;
        let target_directory = reader.path()?;
        let state = reader.files()?;
        reader.finish()?;
        let attestation = Self {
            proof: Proof {
                channel,
                rustc_release,
                rustc_commit,
                rustdoc_release,
                rustdoc_commit,
                cargo_release,
                cargo_commit,
                host,
                target,
                components,
                schemas,
                rustup,
                cargo,
                rustc,
                rustdoc,
                #[cfg(feature = "rustc-driver")]
                driver,
            },
            target_directory,
            state,
        };
        attestation.validate_fixed()?;
        if attestation.encode(limit)?.as_ref() != bytes {
            return Err(node::Error::Codec("toolchain codec is not canonical"));
        }
        Ok(attestation)
    }
}

impl FileEvidence {
    fn borrow(executable: &Executable) -> Self {
        Self {
            path: executable.path.clone(),
            length: executable.length,
            digest: executable.digest,
        }
    }

    fn into_executable(self) -> Executable {
        Executable {
            path: self.path,
            length: self.length,
            digest: self.digest,
        }
    }
}

fn fixed_recipe() -> Box<[u8]> {
    let mut bytes = Vec::new();
    for value in [
        super::CHANNEL,
        super::RUST_RELEASE,
        super::RUST_COMMIT,
        super::CARGO_RELEASE,
        super::CARGO_COMMIT,
        super::HOST,
    ] {
        bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    bytes.extend_from_slice(&STATE_SCHEMA.to_le_bytes());
    bytes.extend_from_slice(&Schemas::current().probe.to_le_bytes());
    bytes.extend_from_slice(&Schemas::current().unit.to_le_bytes());
    bytes.extend_from_slice(&Schemas::current().graph.to_le_bytes());
    bytes.into_boxed_slice()
}

/// The fixed facts plus the selected installation's paths.
fn recipe(selected: &process::Native) -> Box<[u8]> {
    let mut bytes = fixed_recipe().into_vec();
    bytes.extend_from_slice(&selected.selection_key());
    bytes.into_boxed_slice()
}

fn target_directory(rustc: &Path, target: &str) -> Result<PathBuf, node::Error> {
    let root = rustc
        .parent()
        .and_then(Path::parent)
        .ok_or(node::Error::Codec("toolchain executable has no root"))?;
    Ok(root.join("lib/rustlib").join(target))
}

fn state_files_with_budget(
    rustc: &Path,
    limits: &crate::cargo::Limits,
    budget: Option<&node::GraphBudget>,
    account: &mut ToolchainAccount,
) -> Result<Box<[FileEvidence]>, node::Error> {
    let root = rustc
        .parent()
        .and_then(Path::parent)
        .ok_or(node::Error::Codec("toolchain executable has no root"))?;
    let rustlib = root.join("lib/rustlib");
    validate_directory("toolchain state", &rustlib, budget)?;
    if let Some(budget) = budget {
        budget.paths(2)?;
        crate::store::path::input(&rustlib, budget)?;
    }
    let directory = fs::read_dir(&rustlib).map_err(|source| node::Error::Io {
        operation: "read toolchain state directory",
        path: rustlib.clone(),
        source,
    })?;
    let mut directory = directory;
    let mut paths = Vec::new();
    let mut path_bytes = 0_usize;
    loop {
        if let Some(budget) = budget {
            budget.paths(1)?;
        }
        let Some(child) = directory.next() else {
            break;
        };
        let child = child.map_err(|source| node::Error::Io {
            operation: "read toolchain state entry",
            path: rustlib.clone(),
            source,
        })?;
        if let Some(budget) = budget {
            budget.paths(1)?;
            crate::store::path::input(&child.path(), budget)?;
        }
        let metadata = child.metadata().map_err(|source| node::Error::Io {
            operation: "inspect toolchain state entry",
            path: child.path(),
            source,
        })?;
        if metadata.is_file() {
            let path = child.path();
            // The state path bytes and file count are accounting targets
            // recorded on the toolchain account; every state file is still
            // retained. Checked-add overflow stays an arithmetic failure.
            path_bytes = path_bytes
                .checked_add(path.as_os_str().len())
                .ok_or(node::Error::Allocation)?;
            account.exceeded(
                "toolchain state path bytes",
                limits.node_string_bytes(),
                path_bytes,
            );
            paths.try_reserve(1).map_err(|_| node::Error::Allocation)?;
            paths.push(path);
            account.exceeded("toolchain state files", limits.node_items(), paths.len());
        }
    }
    paths.sort_unstable();
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(paths.len())
        .map_err(|_| node::Error::Allocation)?;
    let mut total = 0_u64;
    for path in paths {
        let evidence = evidence_with_budget("toolchain state file", &path, budget)?;
        total = total.saturating_add(evidence.length);
        if total > limits.node_input_bytes() as u64 {
            return Err(node::Error::Limit {
                resource: "toolchain state bytes",
                limit: limits.node_input_bytes(),
                actual: usize::try_from(total).unwrap_or(usize::MAX),
            });
        }
        retained.push(evidence);
    }
    if retained.is_empty() {
        return Err(node::Error::Codec("toolchain state files are absent"));
    }
    Ok(retained.into_boxed_slice())
}

fn evidence_with_budget(
    field: &'static str,
    path: &Path,
    budget: Option<&node::GraphBudget>,
) -> Result<FileEvidence, node::Error> {
    super::executable_evidence_in(field, path, budget)
        .map(|evidence| FileEvidence::borrow(&evidence))
        .map_err(super::evidence_error)
}

fn validate_directory(
    field: &'static str,
    path: &Path,
    budget: Option<&node::GraphBudget>,
) -> Result<(), node::Error> {
    if let Some(budget) = budget {
        budget.paths(1)?;
        crate::store::path::input(path, budget)?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| node::Error::Io {
        operation: "inspect toolchain directory",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(node::Error::Shape(path.to_path_buf()));
    }
    if let Some(budget) = budget {
        budget.paths(1)?;
        crate::store::path::canonical(path, budget)?;
    }
    let canonical = fs::canonicalize(path).map_err(|source| node::Error::Io {
        operation: "canonicalize toolchain directory",
        path: path.to_path_buf(),
        source,
    })?;
    if canonical != path {
        return Err(node::Error::Codec(field));
    }
    Ok(())
}

struct Writer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Writer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), node::Error> {
        let next = self.bytes.len().saturating_add(bytes.len());
        if next > self.limit {
            return Err(node::Error::Limit {
                resource: "toolchain node bytes",
                limit: self.limit,
                actual: next,
            });
        }
        self.bytes
            .try_reserve_exact(bytes.len())
            .map_err(|_| node::Error::Allocation)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn u32(&mut self, value: usize) -> Result<(), node::Error> {
        let value = u32::try_from(value)
            .map_err(|_| node::Error::Codec("toolchain collection is not encodable"))?;
        self.u32_value(value)
    }

    fn u32_value(&mut self, value: u32) -> Result<(), node::Error> {
        self.append(&value.to_le_bytes())
    }

    fn string(&mut self, value: &str) -> Result<(), node::Error> {
        self.u32(value.len())?;
        self.append(value.as_bytes())
    }

    fn path(&mut self, path: &Path) -> Result<(), node::Error> {
        let path = path
            .to_str()
            .ok_or(node::Error::Codec("toolchain path is not Unicode"))?;
        self.string(path)
    }

    fn file(&mut self, file: &FileEvidence) -> Result<(), node::Error> {
        self.path(&file.path)?;
        self.append(&file.length.to_le_bytes())?;
        self.append(&file.digest)
    }

    fn finish(self) -> Box<[u8]> {
        self.bytes.into_boxed_slice()
    }
}

struct Reader<'bytes> {
    bytes: &'bytes [u8],
    offset: usize,
}

impl<'bytes> Reader<'bytes> {
    fn new(bytes: &'bytes [u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "toolchain node bytes",
                limit,
                actual: bytes.len(),
            });
        }
        Ok(Self { bytes, offset: 0 })
    }

    fn take(&mut self, length: usize) -> Result<&'bytes [u8], node::Error> {
        let end = self.offset.saturating_add(length);
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(node::Error::Codec("toolchain node is truncated"))?;
        self.offset = end;
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32, node::Error> {
        self.take(4)?
            .try_into()
            .map(u32::from_le_bytes)
            .map_err(|_| node::Error::Codec("toolchain integer is truncated"))
    }

    fn string(&mut self) -> Result<Box<str>, node::Error> {
        let length = usize::try_from(self.u32()?)
            .map_err(|_| node::Error::Codec("toolchain string is not addressable"))?;
        let value = std::str::from_utf8(self.take(length)?)
            .map_err(|_| node::Error::Codec("toolchain string is not UTF-8"))?;
        Ok(value.into())
    }

    fn path(&mut self) -> Result<PathBuf, node::Error> {
        Ok(PathBuf::from(self.string()?.as_ref()))
    }

    fn file(&mut self) -> Result<FileEvidence, node::Error> {
        let path = self.path()?;
        let length = self
            .take(8)?
            .try_into()
            .map(u64::from_le_bytes)
            .map_err(|_| node::Error::Codec("toolchain file length is truncated"))?;
        let digest = self
            .take(32)?
            .try_into()
            .map_err(|_| node::Error::Codec("toolchain file digest is truncated"))?;
        Ok(FileEvidence {
            path,
            length,
            digest,
        })
    }

    fn executable(&mut self) -> Result<Executable, node::Error> {
        self.file().map(FileEvidence::into_executable)
    }

    fn strings(&mut self) -> Result<Box<[Box<str>]>, node::Error> {
        let count = usize::try_from(self.u32()?)
            .map_err(|_| node::Error::Codec("toolchain string count is not addressable"))?;
        if count > self.bytes.len().saturating_sub(self.offset) / 4 {
            return Err(node::Error::Codec(
                "toolchain string count exceeds its bytes",
            ));
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| node::Error::Allocation)?;
        for _ in 0..count {
            values.push(self.string()?);
        }
        Ok(values.into_boxed_slice())
    }

    fn files(&mut self) -> Result<Box<[FileEvidence]>, node::Error> {
        let count = usize::try_from(self.u32()?)
            .map_err(|_| node::Error::Codec("toolchain file count is not addressable"))?;
        if count > self.bytes.len().saturating_sub(self.offset) / 44 {
            return Err(node::Error::Codec("toolchain file count exceeds its bytes"));
        }
        let mut files = Vec::new();
        files
            .try_reserve_exact(count)
            .map_err(|_| node::Error::Allocation)?;
        for _ in 0..count {
            files.push(self.file()?);
        }
        Ok(files.into_boxed_slice())
    }

    fn finish(self) -> Result<(), node::Error> {
        if self.offset != self.bytes.len() {
            return Err(node::Error::Codec("toolchain node has trailing bytes"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime};

    use sha2::{Digest as _, Sha256};

    use super::*;

    const ROOT_ENV: &str = "RAWR_TOOLCHAIN_NODE_ROOT";
    const REPORT_ENV: &str = "RAWR_TOOLCHAIN_NODE_REPORT";
    const WORKER: &str = "cargo::toolchain::cached::tests::fresh_toolchain_worker";
    static TEST_ID: AtomicU64 = AtomicU64::new(1);

    #[derive(Default)]
    struct Counts {
        hit: AtomicUsize,
        miss: AtomicUsize,
        published: AtomicUsize,
        rejected: AtomicUsize,
        preflights: AtomicUsize,
        processes: AtomicUsize,
    }

    impl metrics::Sink for Counts {
        fn node(&self, kind: metrics::NodeKind, event: metrics::NodeEvent) {
            assert_eq!(kind, metrics::NodeKind::Toolchain);
            match event {
                metrics::NodeEvent::ValidatedHit => &self.hit,
                metrics::NodeEvent::ExecutedMiss => &self.miss,
                metrics::NodeEvent::Published => &self.published,
                metrics::NodeEvent::RejectedHit => &self.rejected,
            }
            .fetch_add(1, Ordering::Relaxed);
        }

        fn process(&self, kind: metrics::ProcessKind) {
            if matches!(
                kind,
                metrics::ProcessKind::RustupWhich
                    | metrics::ProcessKind::RustcVersion
                    | metrics::ProcessKind::RustdocVersion
                    | metrics::ProcessKind::CargoVersion
                    | metrics::ProcessKind::DriverVersion
                    | metrics::ProcessKind::ComponentList
                    | metrics::ProcessKind::TargetList
            ) {
                self.preflights.fetch_add(1, Ordering::Relaxed);
            }
            self.processes.fetch_add(1, Ordering::Relaxed);
        }

        fn milestone(&self, _milestone: metrics::Milestone, _observed_ns: Option<u64>) {}
        fn runner_owned(&self, _owned: &metrics::runner::Owned) {}
        fn scope_opened(&self) {}
        fn scope_joined(&self) {}
    }

    #[test]
    fn canonical_codec_round_trips_and_current_file_drift_rejects() {
        let root = temporary("toolchain-codec");
        let attestation = fixture_attestation(&root);
        let limit = crate::cargo::Limits::standard().node_output_bytes();
        let bytes = attestation.encode(limit).expect("the attestation encodes");
        let decoded = Attestation::decode(&bytes, limit).expect("the attestation decodes");
        assert_eq!(decoded.encode(limit).expect("the replay reencodes"), bytes);
        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert!(matches!(
            Attestation::decode(&trailing, limit),
            Err(node::Error::Codec(_))
        ));
        fs::write(root.join("lib/rustlib/components"), b"changed")
            .expect("the state fixture changes");
        assert!(matches!(
            decoded.revalidate_files(&crate::cargo::Limits::standard()),
            Err(node::Error::Codec("toolchain installation state changed"))
        ));
        fs::remove_dir_all(root).expect("the codec fixture retires");
    }

    #[test]
    fn historical_decode_does_not_observe_the_current_target_directory() {
        let root = temporary("toolchain-historical-decode");
        let attestation = fixture_attestation(&root);
        let limits = crate::cargo::Limits::standard();
        let bytes = attestation.encode(limits.node_output_bytes()).unwrap();
        fs::remove_dir_all(&attestation.target_directory)
            .expect("the historical target directory is removed");
        let decoded = Attestation::decode(&bytes, limits.node_output_bytes())
            .expect("historical decoding does not require a currently installed target");
        assert_eq!(decoded.encode(limits.node_output_bytes()).unwrap(), bytes);
        let budget = node::GraphBudget::new();
        assert!(matches!(
            decoded.revalidate_files_budgeted(&limits, Some(&budget)),
            Err(node::Error::Io {
                operation: "inspect toolchain directory",
                ..
            })
        ));
        assert_eq!(budget.census_usage().unwrap().1, 1);
        fs::remove_dir_all(root).expect("the historical fixture retires");
    }

    #[test]
    fn first_capture_and_warm_evidence_charge_the_same_graph() {
        let root = temporary("toolchain-capture-account");
        let budget = node::GraphBudget::new();
        budget.hash(7).expect("prior graph work is retained");
        let attestation = fixture_attestation_in(&root, Some(&budget));
        let hashed = attestation
            .executables()
            .map(|file| file.length)
            .chain(attestation.state.iter().map(|file| file.length))
            .sum::<u64>();
        let cold = budget
            .census_usage()
            .expect("the capture account is readable");
        assert_eq!(cold.2, 7 + hashed);
        assert!(
            cold.0 > 0,
            "directory canonicalization allocations are admitted"
        );
        assert_eq!(
            cold.1,
            5 * attestation.executables().count() + 20,
            "each small executable uses five operations; capture also validates directories and reads state"
        );
        let limits = crate::cargo::Limits::standard();
        let current = attestation
            .revalidate_files_budgeted(&limits, Some(&budget))
            .expect("the warm evidence is current");
        assert_eq!(
            current,
            attestation.encode(limits.node_output_bytes()).unwrap()
        );
        let warm = budget
            .census_usage()
            .expect("the replay account is readable");
        assert_eq!(
            warm.2,
            7 + hashed,
            "warm validation of unchanged identities hashes nothing"
        );
        assert!(warm.1 - cold.1 < cold.1);
        fs::remove_dir_all(root).expect("the capture fixture retires");
    }

    /// The toolchain state path bytes and file count are accounting
    /// targets on the toolchain account: exact figures record nothing, one
    /// under retains the identical state evidence and records only that axis
    /// with its truthful high water, and nothing refuses.
    #[test]
    fn toolchain_state_targets_record_without_refusal() {
        let root = temporary("toolchain-state-targets");
        let fixture = fixture_attestation(&root);
        assert_eq!(fixture.state.len(), 1);
        let path_bytes = fixture
            .state
            .iter()
            .map(|file| file.path.as_os_str().len())
            .sum::<usize>();
        let rustc = fixture.proof.rustc.path.clone();
        let exercise = |adjust: &dyn Fn(&mut crate::cargo::Limits)| {
            let mut limits = crate::cargo::Limits::standard();
            adjust(&mut limits);
            let mut account = ToolchainAccount::new();
            let state = state_files_with_budget(&rustc, &limits, None, &mut account)
                .expect("the state files are retained under every accounting target");
            assert!(state == fixture.state);
            account.exceedances()
        };
        let exact = |limits: &mut crate::cargo::Limits| {
            limits.items = 1;
            limits.string_bytes = path_bytes;
        };
        assert!(!exercise(&exact).is_exceeded());
        type Adjust<'a> = &'a dyn Fn(&mut crate::cargo::Limits);
        let cases: [(Adjust, &str, usize, usize); 2] = [
            (
                &|limits| {
                    exact(limits);
                    limits.items = 0;
                },
                "toolchain state files",
                0,
                1,
            ),
            (
                &|limits| {
                    exact(limits);
                    limits.string_bytes = path_bytes - 1;
                },
                "toolchain state path bytes",
                path_bytes - 1,
                path_bytes,
            ),
        ];
        for (adjust, axis, target, high_water) in cases {
            let record = exercise(adjust);
            assert_eq!(
                record.axes().count(),
                1,
                "{axis} is the only exceeding axis"
            );
            let recorded = record
                .axis(axis)
                .unwrap_or_else(|| panic!("{axis} records its lowered target"));
            assert_eq!(
                (
                    recorded.target,
                    recorded.first_requested,
                    recorded.high_water
                ),
                (target as u64, high_water as u64, high_water as u64)
            );
        }
        fs::remove_dir_all(root).expect("the state target fixture retires");
    }

    #[test]
    fn first_state_capture_records_hash_exceedance_in_the_original_account() {
        let root = temporary("toolchain-capture-exceedance");
        let fixture = fixture_attestation(&root);
        let state_bytes = fixture.state.iter().map(|file| file.length).sum::<u64>();
        let budget = node::GraphBudget::new();
        budget
            .hash((node::MAX_HASHED_BYTES_PER_GRAPH - state_bytes + 1) as usize)
            .expect("the shared account has one byte too little remaining");
        assert!(!budget.exceedances().is_exceeded());
        // Same bytes, new identity: the capture must read the state file again.
        fs::write(root.join("lib/rustlib/components"), b"fixture-components")
            .expect("the fixture state is rewritten");
        let before = super::super::EXECUTABLE_DIGEST_PASSES.with(std::cell::Cell::get);
        // Hashing the state file crosses the target by one byte; the
        // original shared account keeps the charge, records the axis, and the
        // capture still seals the same state evidence.
        let captured = Attestation::capture(
            fixture.proof,
            &crate::cargo::Limits::standard(),
            Some(&budget),
        )
        .expect("the state hash past the target still captures");
        assert!(captured.state == fixture.state);
        assert_eq!(
            super::super::EXECUTABLE_DIGEST_PASSES.with(std::cell::Cell::get),
            before + 1,
            "the state file is opened and hashed once past the target"
        );
        let hashed = budget
            .census_usage()
            .expect("the shared account is readable")
            .2;
        assert_eq!(hashed, node::MAX_HASHED_BYTES_PER_GRAPH + 1);
        let axis = budget
            .exceedances()
            .axis(node::HASHED_RESOURCE)
            .copied()
            .expect("the state hash records the hashed byte axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                node::MAX_HASHED_BYTES_PER_GRAPH,
                node::MAX_HASHED_BYTES_PER_GRAPH + 1,
                node::MAX_HASHED_BYTES_PER_GRAPH + 1
            )
        );
        fs::remove_dir_all(root).expect("the capture fixture retires");
    }

    #[test]
    fn current_toolchain_witness_participates_in_final_read_set_revalidation() {
        let root = temporary("toolchain-read-set");
        fs::create_dir(&root).expect("the read-set fixture root exists");
        fs::create_dir(root.join("project")).expect("the read-set project exists");
        fs::create_dir(root.join("cargo-home")).expect("the read-set Cargo home exists");
        let toolchain_root = root.join("toolchain");
        let attestation = fixture_attestation(&toolchain_root);
        let cargo_home = fs::canonicalize(root.join("cargo-home"))
            .expect("the read-set Cargo home canonicalizes");
        let environment = crate::cargo::config::Environment::from_pairs([(
            "CARGO_HOME",
            cargo_home
                .to_str()
                .expect("the read-set Cargo home is Unicode"),
        )])
        .expect("the read-set environment seals");
        let limits = crate::cargo::Limits::standard();
        let context = Context::capture(
            &root.join("project"),
            environment,
            super::super::HOST,
            &limits,
        )
        .expect("the read-set context seals");
        let store = context.open_store().expect("the typed store opens");
        let recipe = context
            .discover::<Attestation>(&fixed_recipe(), limits.node_output_bytes())
            .expect("the toolchain recipe seals");
        let counts = Counts::default();
        let validated = node::Evaluator::new(&store, &counts)
            .discover(
                &recipe,
                move |candidate| candidate.revalidate_files(&limits),
                || Ok(attestation),
            )
            .expect("the fixture toolchain node publishes");
        let retained = context.retain_toolchain(Arc::new(validated));
        let before = (
            counts.hit.load(Ordering::Relaxed),
            counts.miss.load(Ordering::Relaxed),
            counts.published.load(Ordering::Relaxed),
            counts.rejected.load(Ordering::Relaxed),
            counts.processes.load(Ordering::Relaxed),
        );
        let repeated = cached(&context, &counts).expect("the launch-local toolchain is reused");
        assert!(Arc::ptr_eq(&retained, &repeated));
        assert_eq!(
            (
                counts.hit.load(Ordering::Relaxed),
                counts.miss.load(Ordering::Relaxed),
                counts.published.load(Ordering::Relaxed),
                counts.rejected.load(Ordering::Relaxed),
                counts.processes.load(Ordering::Relaxed),
            ),
            before,
            "launch-local reuse performs no fifth lookup or subprocess"
        );
        let sealed_store = snapshot(store.root());
        let evaluator = node::Evaluator::new(&store, &counts);
        let proof = evaluator
            .seal_read_set([retained.receipt().read_set().clone()])
            .expect("the toolchain read set seals");
        evaluator
            .revalidate_read_set(&proof)
            .expect("the unchanged toolchain read set revalidates");
        assert_eq!(snapshot(store.root()), sealed_store);

        fs::write(toolchain_root.join("lib/rustlib/components"), b"changed")
            .expect("the toolchain state fixture changes");
        assert!(matches!(
            evaluator.revalidate_read_set(&proof),
            Err(node::Error::Codec("toolchain installation state changed"))
        ));
        assert_eq!(snapshot(store.root()), sealed_store);
        fs::remove_dir_all(root).expect("the toolchain-read-set fixture retires");
    }

    #[test]
    #[ignore = "launched only by the real fresh-process toolchain-node proof"]
    fn fresh_toolchain_worker() {
        let root = PathBuf::from(
            std::env::var_os(ROOT_ENV).expect("the worker receives its fixture root"),
        );
        let report_path = PathBuf::from(
            std::env::var_os(REPORT_ENV).expect("the worker receives its report path"),
        );
        let environment = crate::cargo::config::Environment::capture()
            .expect("the worker captures its complete environment")
            .removing(ROOT_ENV)
            .expect("the fixture root control is excluded")
            .removing(REPORT_ENV)
            .expect("the fixture report control is excluded")
            .setting(
                "CARGO_HOME",
                root.join("cargo-home")
                    .to_str()
                    .expect("the fixture Cargo home is Unicode"),
            )
            .expect("the fixture Cargo home is sealed");
        let context = Context::capture(
            &root.join("project"),
            environment,
            super::super::HOST,
            &crate::cargo::Limits::standard(),
        )
        .expect("the toolchain-node context seals");
        let counts = Arc::new(Counts::default());
        let scope = metrics::Scope::transform(Arc::clone(&counts) as Arc<dyn metrics::Sink>)
            .expect("the worker installs its exact metric sink");
        let attestation = cached(&context, counts.as_ref()).expect("the toolchain node validates");
        let _native = process::Native::from_attestation(&attestation);
        drop(scope);
        let report = format!(
            "hits={}\nmisses={}\npublished={}\nrejected={}\npreflights={}\nprocesses={}\n",
            counts.hit.load(Ordering::Relaxed),
            counts.miss.load(Ordering::Relaxed),
            counts.published.load(Ordering::Relaxed),
            counts.rejected.load(Ordering::Relaxed),
            counts.preflights.load(Ordering::Relaxed),
            counts.processes.load(Ordering::Relaxed),
        );
        fs::write(report_path, report.as_bytes()).expect("the worker writes its report");
    }

    #[test]
    #[ignore = "requires the exact rawr-import-driver prerequisite and installed pinned toolchain"]
    fn cached_toolchain_fresh_process_hit_launches_zero_subprocesses() {
        let root = temporary("toolchain-node");
        fs::create_dir(&root).expect("the fixture root exists");
        fs::create_dir(root.join("project")).expect("the fixture project exists");
        fs::create_dir(root.join("cargo-home")).expect("the fixture Cargo home exists");
        let first_report = root.with_extension("first.report");
        let second_report = root.with_extension("second.report");
        let first_elapsed = run_worker(&root, &first_report);
        let first = parse_report(&first_report);
        assert_eq!(first.get("hits"), Some(&0));
        assert_eq!(first.get("misses"), Some(&1));
        assert_eq!(first.get("published"), Some(&1));
        assert_eq!(first.get("rejected"), Some(&0));
        assert_eq!(first.get("preflights"), Some(&9));
        assert!(first
            .get("processes")
            .is_some_and(|processes| *processes >= 9));
        let sealed = snapshot(&root.join("cargo-home/rawr/v1"));

        let second_elapsed = run_worker(&root, &second_report);
        let second = parse_report(&second_report);
        assert_eq!(second.get("hits"), Some(&1));
        assert_eq!(second.get("misses"), Some(&0));
        assert_eq!(second.get("published"), Some(&0));
        assert_eq!(second.get("rejected"), Some(&0));
        assert_eq!(second.get("preflights"), Some(&0));
        assert_eq!(second.get("processes"), Some(&0));
        assert_eq!(snapshot(&root.join("cargo-home/rawr/v1")), sealed);
        let saved = first_elapsed.saturating_sub(second_elapsed);
        eprintln!(
            "TOOLCHAIN_NODE first_ms={} second_ms={} saved_ms={} first_preflights=9 first_processes={} second_preflights=0 second_processes=0",
            first_elapsed.as_millis(),
            second_elapsed.as_millis(),
            saved.as_millis(),
            first.get("processes").copied().unwrap_or_default(),
        );
        fs::remove_file(first_report).expect("the first report retires");
        fs::remove_file(second_report).expect("the second report retires");
        fs::remove_dir_all(root).expect("the toolchain-node fixture retires");
    }

    fn run_worker(root: &Path, report: &Path) -> Duration {
        let started = Instant::now();
        let output = Command::new(std::env::current_exe().expect("the test executable resolves"))
            .arg("--ignored")
            .arg("--exact")
            .arg(WORKER)
            .arg("--nocapture")
            .env(ROOT_ENV, root)
            .env(REPORT_ENV, report)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("the fresh toolchain worker launches");
        assert!(
            output.status.success(),
            "fresh toolchain worker failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        started.elapsed()
    }

    fn parse_report(path: &Path) -> BTreeMap<Box<str>, usize> {
        fs::read_to_string(path)
            .expect("the toolchain report exists")
            .lines()
            .map(|line| {
                let (key, value) = line.split_once('=').expect("the report row is key/value");
                (
                    key.to_owned().into_boxed_str(),
                    value.parse().expect("the report count is decimal"),
                )
            })
            .collect()
    }

    #[test]
    fn different_selections_keep_their_own_records() {
        let root = temporary("toolchain-selections");
        fs::create_dir_all(root.join("project")).unwrap();
        fs::create_dir(root.join("cargo-home")).unwrap();
        let cargo_home = fs::canonicalize(root.join("cargo-home")).unwrap();
        let environment = crate::cargo::config::Environment::from_pairs([(
            "CARGO_HOME",
            cargo_home.to_str().unwrap(),
        )])
        .unwrap();
        let limits = crate::cargo::Limits::standard();
        let context = Context::capture(
            &root.join("project"),
            environment,
            super::super::HOST,
            &limits,
        )
        .unwrap();
        let installs = [root.join("install-a"), root.join("install-b")];
        let counts = Counts::default();
        let evaluate = |attestation: Option<Attestation>, selected: process::Native| {
            let store = context.open_store().unwrap();
            let recipe = context
                .discover::<Attestation>(&recipe(&selected), limits.node_output_bytes())
                .unwrap();
            node::Evaluator::new(&store, &counts).discover_budgeted(
                &recipe,
                move |candidate, budget| candidate.revalidate(&selected, &limits, budget),
                move || attestation.ok_or(node::Error::State("unexpected miss")),
            )
        };
        let selections = installs.each_ref().map(|install| {
            let attestation = fixture_attestation(install);
            let selected = process::Native::from_toolchain(&attestation.proof);
            let publish = process::Native::from_toolchain(&attestation.proof);
            evaluate(Some(attestation), publish).expect("each install publishes its own record");
            selected
        });
        // Both records coexist: each install hits its own, rejecting nothing.
        for selected in selections {
            evaluate(None, selected).expect("each install reuses its own record");
        }
        assert_eq!(counts.miss.load(Ordering::Relaxed), 2);
        assert_eq!(counts.hit.load(Ordering::Relaxed), 2);
        assert_eq!(counts.rejected.load(Ordering::Relaxed), 0);
        fs::remove_dir_all(root).unwrap();
    }

    fn temporary(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rawr-{label}-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    pub(super) fn fixture_attestation(root: &Path) -> Attestation {
        fixture_attestation_in(root, None)
    }

    fn fixture_attestation_in(root: &Path, budget: Option<&node::GraphBudget>) -> Attestation {
        fs::create_dir(root).expect("the toolchain fixture root exists");
        let root = fs::canonicalize(root).expect("the toolchain fixture root canonicalizes");
        fs::create_dir(root.join("bin")).expect("the fixture bin exists");
        fs::create_dir_all(
            root.join("lib/rustlib")
                .join(super::super::HOST)
                .join("lib"),
        )
        .expect("the fixture target exists");
        fs::write(root.join("lib/rustlib/components"), b"fixture-components")
            .expect("the fixture state exists");
        for name in [
            "rustup",
            "cargo",
            "rustc",
            "rustdoc",
            #[cfg(feature = "rustc-driver")]
            "rawr-import-driver",
        ] {
            fs::write(root.join("bin").join(name), name.as_bytes())
                .expect("the fixture executable exists");
        }
        let executable = |name: &str| {
            super::super::executable_evidence_in(
                "fixture toolchain executable",
                &root.join("bin").join(name),
                budget,
            )
            .expect("the fixture executable seals")
        };
        let proof = Proof {
            channel: super::super::CHANNEL.into(),
            rustc_release: super::super::RUST_RELEASE.into(),
            rustc_commit: super::super::RUST_COMMIT.into(),
            rustdoc_release: super::super::RUST_RELEASE.into(),
            rustdoc_commit: super::super::RUST_COMMIT.into(),
            cargo_release: super::super::CARGO_RELEASE.into(),
            cargo_commit: super::super::CARGO_COMMIT.into(),
            host: super::super::HOST.into(),
            target: super::super::HOST.into(),
            components: super::super::REQUIRED_COMPONENTS
                .map(Box::<str>::from)
                .into(),
            schemas: Schemas::current(),
            rustup: executable("rustup"),
            cargo: executable("cargo"),
            rustc: executable("rustc"),
            rustdoc: executable("rustdoc"),
            #[cfg(feature = "rustc-driver")]
            driver: executable("rawr-import-driver"),
        };
        Attestation::capture(proof, &crate::cargo::Limits::standard(), budget)
            .expect("the fixture attestation seals")
    }

    #[derive(Debug, Eq, PartialEq)]
    struct Snapshot {
        directory: bool,
        bytes: u64,
        digest: [u8; 32],
        modified: Option<Duration>,
    }

    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Snapshot> {
        let mut records = BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for child in fs::read_dir(&directory).expect("the store tree is readable") {
                let path = child.expect("the store child is readable").path();
                let metadata = fs::symlink_metadata(&path).expect("the store child has metadata");
                assert!(!metadata.file_type().is_symlink());
                let relative = path
                    .strip_prefix(root)
                    .expect("the store child remains beneath its root")
                    .to_path_buf();
                let directory = metadata.is_dir();
                let (bytes, digest) = if metadata.is_file() {
                    let contents = fs::read(&path).expect("the store file is readable");
                    (metadata.len(), Sha256::digest(contents).into())
                } else {
                    (0, [0; 32])
                };
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok());
                records.insert(
                    relative,
                    Snapshot {
                        directory,
                        bytes,
                        digest,
                        modified,
                    },
                );
                if directory {
                    pending.push(path);
                }
            }
        }
        records
    }
}
