//! The first cached Cargo-resolution output node.
//!
//! This deliberately narrow slice admits one locked dependency-free package.
//! Its complete mutable authority is therefore the root `Cargo.toml` and
//! `Cargo.lock`; broader workspace and dependency closures remain later DAG
//! work rather than implicit cache authority.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use super::toolchain::Attestation;
use crate::node::{self, Parent};
use crate::process::{self, ProductionReplay as _, ToolPaths as _};
use crate::{context::Context, metrics};

#[cfg(feature = "rustc-driver")]
mod build;
#[cfg(feature = "rustc-driver")]
pub(crate) use build::cached as cached_build;
#[cfg(feature = "rustc-driver")]
pub(crate) use build::Product as BuildProduct;

#[cfg(feature = "rustc-driver")]
pub(crate) mod layout;

#[cfg(feature = "rustc-driver")]
pub(crate) mod cold;

#[cfg(feature = "rustc-driver")]
mod relocate;

#[cfg(feature = "rustc-driver")]
pub(in crate::cargo) mod replay;

#[cfg(feature = "rustc-driver")]
pub(crate) mod bundle;

#[cfg(feature = "rustc-driver")]
pub(crate) mod content;

#[cfg(feature = "rustc-driver")]
pub(crate) mod owner;

#[cfg(feature = "rustc-driver")]
mod watched;

#[cfg(feature = "rustc-driver")]
mod remembered;

const OUTPUT_SCHEMA: u32 = 1;
const RECIPE_SCHEMA: u32 = 1;
const HEADER_BYTES: usize = 72;
const LOCKED_OUTPUT_SCHEMA: u32 = 2;
const LOCKED_RECIPE_SCHEMA: u32 = 1;
const UNIT_GRAPH_OUTPUT_SCHEMA: u32 = 3;

/// Canonical locked Cargo metadata plus the exact leaf inputs that produced it.
pub struct Metadata {
    manifest: [u8; 32],
    lock: [u8; 32],
    canonical: Box<[u8]>,
}

/// Resolve or reuse the exact locked dependency-free package under `context`.
pub fn cached(
    context: &Context,
    toolchain: &node::Validated<Attestation>,
    sink: &dyn metrics::Sink,
) -> Result<node::Validated<Metadata>, node::Error> {
    let limits = context.limits();
    let manifest = context.project().join("Cargo.toml");
    let lock = context.project().join("Cargo.lock");
    let target = context.project().join("target");
    let parent = Parent::validated(toolchain);
    let fixed = fixed_recipe(context, &manifest, &target)?;
    let recipe =
        context.discover_with_parents::<Metadata>(&fixed, limits.node_output_bytes(), &[parent])?;
    let validation_context = context.clone();
    node::Evaluator::new(&context.open_store()?, sink).discover_budgeted(
        &recipe,
        move |candidate, budget| candidate.revalidate(&validation_context, &limits, budget),
        || Metadata::resolve(context, toolchain, &manifest, &lock, &target),
    )
}

impl Metadata {
    fn resolve(
        context: &Context,
        toolchain: &node::Validated<Attestation>,
        manifest: &Path,
        lock: &Path,
        target: &Path,
    ) -> Result<Self, node::Error> {
        let limits = context.limits();
        require_configuration_free(context, &limits)?;
        let (manifest_digest, lock_digest) = input_digests(manifest, lock)?;
        let native = process::Native::from_attestation(toolchain);
        let production = native.production();
        let (prefix, tail) = command_parts(manifest, context.target());
        let environment = context.environment().command_values(
            target,
            production.cargo_tools().0,
            production.cargo_tools().1,
        )?;
        let command = process::Command::cargo(
            prefix,
            process::CargoAction::Metadata,
            tail,
            context.project().to_path_buf(),
            environment,
        )?;
        let raw = production
            .cargo()
            .run(&command, limits.process_limits())?
            .checked(process::Program::Cargo)?;
        let (current_manifest, current_lock) = input_digests(manifest, lock)?;
        if current_manifest != manifest_digest || current_lock != lock_digest {
            return Err(node::Error::Codec(
                "locked leaf inputs changed during Cargo metadata",
            ));
        }
        let canonical =
            super::unit::canonical_leaf_metadata(&raw, manifest, context.project(), &limits)?;
        let metadata = Self {
            manifest: manifest_digest,
            lock: lock_digest,
            canonical,
        };
        metadata.validate_canonical(&limits)?;
        Ok(metadata)
    }

    fn revalidate(
        &self,
        context: &Context,
        limits: &super::Limits,
        budget: &node::GraphBudget,
    ) -> Result<Box<[u8]>, node::Error> {
        require_configuration_free_with_budget(context, limits, Some(budget))?;
        self.validate_canonical(limits)?;
        let (manifest_digest, lock_digest) = input_digests_budgeted(
            &context.project().join("Cargo.toml"),
            &context.project().join("Cargo.lock"),
            Some(budget),
        )?;
        if manifest_digest != self.manifest || lock_digest != self.lock {
            return Err(node::Error::Codec("locked leaf Cargo authority changed"));
        }
        Ok(discovered_inputs(self.manifest, self.lock))
    }

    fn validate_canonical(&self, limits: &super::Limits) -> Result<(), node::Error> {
        let replayed = super::unit::canonical_metadata_replay(&self.canonical, limits)?;
        if replayed.as_ref() != self.canonical.as_ref() {
            return Err(node::Error::Codec(
                "stored leaf Cargo metadata is not canonical",
            ));
        }
        Ok(())
    }

    /// Borrow the canonical Cargo metadata output for the next DAG transform.
    #[must_use]
    pub fn canonical(&self) -> &[u8] {
        &self.canonical
    }
}

impl node::Value for Metadata {
    const KIND: node::Kind = node::Kind::Resolution;
    const SCHEMA: u32 = OUTPUT_SCHEMA;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        let length = HEADER_BYTES.saturating_add(self.canonical.len());
        if length > limit {
            return Err(node::Error::Limit {
                resource: "cached leaf Cargo metadata bytes",
                limit,
                actual: length,
            });
        }
        let canonical_length = u64::try_from(self.canonical.len())
            .map_err(|_| node::Error::Codec("Cargo metadata length is not encodable"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| node::Error::Allocation)?;
        bytes.extend_from_slice(&self.manifest);
        bytes.extend_from_slice(&self.lock);
        bytes.extend_from_slice(&canonical_length.to_le_bytes());
        bytes.extend_from_slice(&self.canonical);
        Ok(bytes.into_boxed_slice())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached leaf Cargo metadata bytes",
                limit,
                actual: bytes.len(),
            });
        }
        let manifest = bytes
            .get(..32)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(node::Error::Codec(
                "Cargo metadata manifest digest is truncated",
            ))?;
        let lock = bytes
            .get(32..64)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(node::Error::Codec(
                "Cargo metadata lock digest is truncated",
            ))?;
        let canonical_length = bytes
            .get(64..HEADER_BYTES)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u64::from_le_bytes)
            .and_then(|length| usize::try_from(length).ok())
            .ok_or(node::Error::Codec("Cargo metadata length is invalid"))?;
        let canonical = bytes
            .get(HEADER_BYTES..)
            .filter(|canonical| canonical.len() == canonical_length)
            .ok_or(node::Error::Codec("Cargo metadata payload is truncated"))?
            .to_vec()
            .into_boxed_slice();
        Ok(Self {
            manifest,
            lock,
            canonical,
        })
    }
}

fn input_digests(manifest: &Path, lock: &Path) -> Result<([u8; 32], [u8; 32]), node::Error> {
    input_digests_budgeted(manifest, lock, None)
}

fn input_digests_budgeted(
    manifest: &Path,
    lock: &Path,
    budget: Option<&node::GraphBudget>,
) -> Result<([u8; 32], [u8; 32]), node::Error> {
    // Manifest and lock reads are I/O retention bounded by physical
    // constants (manifest class: `TAIL_BYTES`; lock class: `CAPTURE_BYTES`),
    // never by an accounting target.
    let manifest = read_hashed_input(
        "cached leaf Cargo manifest",
        manifest,
        process::TAIL_BYTES,
        budget,
    )?;
    // Lock reads are I/O retention, bounded by the physical capture
    // constant rather than the Cargo JSON accounting target.
    let lock = read_hashed_input(
        "cached leaf Cargo lock",
        lock,
        process::CAPTURE_BYTES,
        budget,
    )?;
    Ok((Sha256::digest(manifest).into(), Sha256::digest(lock).into()))
}

fn read_hashed_input(
    resource: &'static str,
    path: &Path,
    limit: usize,
    budget: Option<&node::GraphBudget>,
) -> Result<Vec<u8>, node::Error> {
    if let Some(budget) = budget {
        budget.paths(1)?;
        let metadata = fs::symlink_metadata(path).map_err(|source| node::Error::Io {
            operation: "inspect cached Cargo input before hashing",
            path: path.to_path_buf(),
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(node::Error::Shape(path.to_path_buf()));
        }
        let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if length > limit {
            return Err(node::Error::Limit {
                resource,
                limit,
                actual: length,
            });
        }
        budget.hash(length)?;
    }
    Ok(super::read_bounded(resource, path, limit)?)
}

fn require_configuration_free(
    context: &Context,
    limits: &super::Limits,
) -> Result<(), node::Error> {
    require_configuration_free_with_budget(context, limits, None)
}

fn require_configuration_free_with_budget(
    context: &Context,
    limits: &super::Limits,
    budget: Option<&node::GraphBudget>,
) -> Result<(), node::Error> {
    let mut directories = 0_usize;
    for ancestor in context.project().ancestors() {
        directories = directories.saturating_add(1);
        if directories > limits.depth {
            return Err(node::Error::Limit {
                resource: "cached leaf Cargo config search depth",
                limit: limits.depth,
                actual: directories,
            });
        }
        require_absent_budgeted(&ancestor.join(".cargo/config.toml"), budget)?;
        require_absent_budgeted(&ancestor.join(".cargo/config"), budget)?;
    }
    require_absent_budgeted(&context.cargo_home().join("config.toml"), budget)?;
    require_absent_budgeted(&context.cargo_home().join("config"), budget)
}

fn require_absent_budgeted(
    path: &Path,
    budget: Option<&node::GraphBudget>,
) -> Result<(), node::Error> {
    if let Some(budget) = budget {
        budget.paths(1)?;
    }
    require_absent(path)
}

fn require_absent(path: &Path) -> Result<(), node::Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(node::Error::Codec(
            "cached leaf Cargo resolution does not admit config files",
        )),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(node::Error::Io {
            operation: "inspect cached leaf Cargo config path",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn discovered_inputs(manifest: [u8; 32], lock: [u8; 32]) -> Box<[u8]> {
    let mut bytes = Vec::with_capacity(72);
    bytes.extend_from_slice(b"rawr.cargo.leaf-inputs/v1");
    bytes.extend_from_slice(&manifest);
    bytes.extend_from_slice(&lock);
    bytes.into_boxed_slice()
}

fn fixed_recipe(
    context: &Context,
    manifest: &Path,
    target: &Path,
) -> Result<Box<[u8]>, node::Error> {
    let (prefix, tail) = command_parts(manifest, context.target());
    let arguments =
        process::Command::cargo_arguments(prefix, process::CargoAction::Metadata, tail)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"rawr.cargo.locked-leaf-resolution/v1");
    bytes.extend_from_slice(&RECIPE_SCHEMA.to_le_bytes());
    retain_path(&mut bytes, target)?;
    let count = u32::try_from(arguments.len())
        .map_err(|_| node::Error::Codec("Cargo metadata argument count is not encodable"))?;
    bytes.extend_from_slice(&count.to_le_bytes());
    for argument in arguments {
        retain_bytes(&mut bytes, argument.as_os_str().as_encoded_bytes())?;
    }
    Ok(bytes.into_boxed_slice())
}

fn command_parts(manifest: &Path, target: &str) -> (Vec<OsString>, Vec<OsString>) {
    (
        vec![OsString::from("--offline")],
        super::cargo_metadata_arguments(1, manifest, target, true),
    )
}

fn retain_path(bytes: &mut Vec<u8>, path: &Path) -> Result<(), node::Error> {
    retain_bytes(bytes, path.as_os_str().as_encoded_bytes())
}

fn retain_bytes(bytes: &mut Vec<u8>, value: &[u8]) -> Result<(), node::Error> {
    let length = u32::try_from(value.len())
        .map_err(|_| node::Error::Codec("Cargo metadata recipe field is not encodable"))?;
    bytes
        .try_reserve_exact(4_usize.saturating_add(value.len()))
        .map_err(|_| node::Error::Allocation)?;
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(value);
    Ok(())
}

/// Path-bound exact Cargo metadata retained for the existing resolution parser.
pub(crate) struct Locked {
    lock: Evidence,
    manifests: Box<[Evidence]>,
    raw: Box<[u8]>,
}

/// Exact Cargo unit-graph bytes whose metadata/input parent already validated.
pub(crate) struct UnitGraph {
    raw: Box<[u8]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Evidence {
    path: PathBuf,
    length: u64,
    digest: [u8; 32],
}

/// Cache one exact locked metadata command while leaving its established
/// semantic parser and post-build revalidation unchanged.
pub(crate) fn cached_locked(
    context: &Context,
    authority: &[u8],
    manifest: &Path,
    lock: &Path,
    target: &Path,
    sink: &dyn metrics::Sink,
    execute: impl FnOnce() -> crate::Result<Box<[u8]>>,
) -> Result<node::Validated<Locked>, node::Error> {
    cached_locked_with_parents(
        context,
        authority,
        manifest,
        lock,
        target,
        &[],
        sink,
        execute,
    )
}

pub(crate) fn cached_locked_with_toolchain(
    context: &Context,
    parent: &node::Validated<Attestation>,
    authority: &[u8],
    manifest: &Path,
    lock: &Path,
    target: &Path,
    sink: &dyn metrics::Sink,
    execute: impl FnOnce() -> crate::Result<Box<[u8]>>,
) -> Result<node::Validated<Locked>, node::Error> {
    cached_locked_with_parents(
        context,
        authority,
        manifest,
        lock,
        target,
        &[Parent::validated(parent)],
        sink,
        execute,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "the locked metadata recipe binds exact path authority and its current parent set"
)]
fn cached_locked_with_parents(
    context: &Context,
    authority: &[u8],
    manifest: &Path,
    lock: &Path,
    target: &Path,
    parents: &[Parent],
    sink: &dyn metrics::Sink,
    execute: impl FnOnce() -> crate::Result<Box<[u8]>>,
) -> Result<node::Validated<Locked>, node::Error> {
    let limits = context.limits();
    let fixed = locked_recipe(authority, manifest, lock, target)?;
    let recipe =
        context.discover_with_parents::<Locked>(&fixed, limits.node_input_bytes(), parents)?;
    let validation_lock = lock.to_path_buf();
    node::Evaluator::new(&context.open_store()?, sink).discover_budgeted(
        &recipe,
        move |candidate, budget| candidate.revalidate(&validation_lock, &limits, budget),
        || {
            let raw = execute()?;
            Locked::capture(raw, manifest, lock, &limits)
        },
    )
}

/// Reuse one exact unit-graph command under the validated locked-metadata
/// parent. The established semantic unit parser remains the final consumer.
pub(crate) fn cached_unit_graph(
    context: &Context,
    parent: &node::Validated<Locked>,
    authority: &[u8],
    sink: &dyn metrics::Sink,
    execute: impl FnOnce() -> crate::Result<Box<[u8]>>,
) -> Result<node::Validated<UnitGraph>, node::Error> {
    let limits = context.limits();
    let parent = Parent::validated(parent);
    let recipe = context.direct_with_parents::<UnitGraph>(
        authority,
        limits.node_output_bytes(),
        &[parent],
    )?;
    node::Evaluator::new(&context.open_store()?, sink)
        .native(&recipe, || UnitGraph::capture(execute()?, &limits))
}

/// Reuse the exact post-build unit graph under the authoritative build tree.
#[cfg(feature = "rustc-driver")]
pub(crate) fn cached_post_unit_graph(
    context: &Context,
    parent: &node::Validated<build::Product>,
    authority: &[u8],
    sink: &dyn metrics::Sink,
    execute: impl FnOnce() -> crate::Result<Box<[u8]>>,
) -> Result<node::Validated<UnitGraph>, node::Error> {
    let limits = context.limits();
    let parent = Parent::validated(parent);
    let recipe = context.direct_with_parents::<UnitGraph>(
        authority,
        limits.node_output_bytes(),
        &[parent],
    )?;
    node::Evaluator::new(&context.open_store()?, sink)
        .native(&recipe, || UnitGraph::capture(execute()?, &limits))
}

/// Reuse the exact post-build locked metadata under the authoritative build
/// tree while retaining the existing manifest and lock evidence codec.
#[cfg(feature = "rustc-driver")]
#[allow(
    clippy::too_many_arguments,
    reason = "the post-build metadata node binds one parent and its three path authorities"
)]
pub(crate) fn cached_post_metadata(
    context: &Context,
    parent: &node::Validated<build::Product>,
    authority: &[u8],
    manifest: &Path,
    lock: &Path,
    target: &Path,
    sink: &dyn metrics::Sink,
    execute: impl FnOnce() -> crate::Result<Box<[u8]>>,
) -> Result<node::Validated<Locked>, node::Error> {
    let limits = context.limits();
    let parent = Parent::validated(parent);
    let fixed = locked_recipe(authority, manifest, lock, target)?;
    let recipe =
        context.direct_with_parents::<Locked>(&fixed, limits.node_input_bytes(), &[parent])?;
    node::Evaluator::new(&context.open_store()?, sink).native(&recipe, || {
        Locked::capture(execute()?, manifest, lock, &limits)
    })
}

impl Locked {
    fn capture(
        raw: Box<[u8]>,
        root_manifest: &Path,
        lock: &Path,
        limits: &super::Limits,
    ) -> Result<Self, node::Error> {
        let paths = super::unit::metadata_manifest_paths(&raw, limits)?;
        if !paths.iter().any(|path| path == root_manifest) {
            return Err(node::Error::Codec(
                "cached Cargo metadata omitted its root manifest",
            ));
        }
        let mut manifests = Vec::new();
        manifests
            .try_reserve_exact(paths.len())
            .map_err(|_| node::Error::Allocation)?;
        let mut retained = 0_usize;
        for path in paths.iter() {
            let evidence = Evidence::capture(path, process::TAIL_BYTES)?;
            retained =
                retained.saturating_add(usize::try_from(evidence.length).unwrap_or(usize::MAX));
            if retained > limits.node_input_bytes() {
                return Err(node::Error::Limit {
                    resource: "cached Cargo metadata input bytes",
                    limit: limits.node_input_bytes(),
                    actual: retained,
                });
            }
            manifests.push(evidence);
        }
        let lock = Evidence::capture(lock, process::CAPTURE_BYTES)?;
        retained = retained.saturating_add(usize::try_from(lock.length).unwrap_or(usize::MAX));
        if retained > limits.node_input_bytes() {
            return Err(node::Error::Limit {
                resource: "cached Cargo metadata input bytes",
                limit: limits.node_input_bytes(),
                actual: retained,
            });
        }
        let locked = Self {
            lock,
            manifests: manifests.into_boxed_slice(),
            raw,
        };
        locked.validate_shape(limits)?;
        Ok(locked)
    }

    fn revalidate(
        &self,
        lock: &Path,
        limits: &super::Limits,
        budget: &node::GraphBudget,
    ) -> Result<Box<[u8]>, node::Error> {
        self.validate_shape(limits)?;
        if self.lock.path != lock
            || self.lock != Evidence::capture_budgeted(lock, process::CAPTURE_BYTES, Some(budget))?
        {
            return Err(node::Error::Codec("cached Cargo lock evidence changed"));
        }
        for expected in self.manifests.iter() {
            if *expected
                != Evidence::capture_budgeted(&expected.path, process::TAIL_BYTES, Some(budget))?
            {
                return Err(node::Error::Codec(
                    "cached Cargo package-manifest evidence changed",
                ));
            }
        }
        self.discovered_inputs(limits)
    }

    fn validate_shape(&self, limits: &super::Limits) -> Result<(), node::Error> {
        let paths = super::unit::metadata_manifest_paths(&self.raw, limits)?;
        if paths.len() != self.manifests.len()
            || paths
                .iter()
                .zip(self.manifests.iter())
                .any(|(path, evidence)| path != &evidence.path)
        {
            return Err(node::Error::Codec(
                "cached Cargo metadata manifest evidence changed",
            ));
        }
        if self.manifests.is_empty()
            || self
                .manifests
                .windows(2)
                .any(|pair| pair[0].path >= pair[1].path)
        {
            return Err(node::Error::Codec(
                "cached Cargo metadata manifest ordering changed",
            ));
        }
        Ok(())
    }

    fn discovered_inputs(&self, limits: &super::Limits) -> Result<Box<[u8]>, node::Error> {
        let mut writer = LockedWriter::new(limits.node_input_bytes());
        writer.append(b"rawr.cargo.locked-metadata-inputs/v1")?;
        writer.evidence(&self.lock)?;
        writer.count(self.manifests.len())?;
        for manifest in self.manifests.iter() {
            writer.evidence(manifest)?;
        }
        Ok(writer.finish())
    }

    pub(crate) fn raw(&self) -> &[u8] {
        &self.raw
    }
}

impl UnitGraph {
    fn capture(raw: Box<[u8]>, limits: &super::Limits) -> Result<Self, node::Error> {
        super::unit::validate_unit_graph_transport(&raw, limits)?;
        Ok(Self { raw })
    }

    pub(crate) fn raw(&self) -> &[u8] {
        &self.raw
    }
}

impl node::Value for UnitGraph {
    const KIND: node::Kind = node::Kind::Resolution;
    const SCHEMA: u32 = UNIT_GRAPH_OUTPUT_SCHEMA;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        if self.raw.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached Cargo unit-graph bytes",
                limit,
                actual: self.raw.len(),
            });
        }
        Ok(self.raw.clone())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached Cargo unit-graph bytes",
                limit,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            raw: bytes.to_vec().into_boxed_slice(),
        })
    }
}

impl node::Value for Locked {
    const KIND: node::Kind = node::Kind::Resolution;
    const SCHEMA: u32 = LOCKED_OUTPUT_SCHEMA;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        let mut writer = LockedWriter::new(limit);
        writer.evidence(&self.lock)?;
        writer.count(self.manifests.len())?;
        for manifest in self.manifests.iter() {
            writer.evidence(manifest)?;
        }
        writer.bytes(&self.raw)?;
        Ok(writer.finish())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        let mut reader = LockedReader::new(bytes, limit)?;
        let lock = reader.evidence()?;
        let count = reader.count()?;
        if count > bytes.len().saturating_sub(reader.offset) / 44 {
            return Err(node::Error::Codec(
                "cached Cargo manifest count exceeds its bytes",
            ));
        }
        let mut manifests = Vec::new();
        manifests
            .try_reserve_exact(count)
            .map_err(|_| node::Error::Allocation)?;
        for _ in 0..count {
            manifests.push(reader.evidence()?);
        }
        let raw = reader.bytes()?;
        reader.finish()?;
        Ok(Self {
            lock,
            manifests: manifests.into_boxed_slice(),
            raw,
        })
    }
}

impl Evidence {
    fn capture(path: &Path, limit: usize) -> Result<Self, node::Error> {
        Self::capture_budgeted(path, limit, None)
    }

    fn capture_budgeted(
        path: &Path,
        limit: usize,
        budget: Option<&node::GraphBudget>,
    ) -> Result<Self, node::Error> {
        let bytes = read_hashed_input("cached Cargo metadata input", path, limit, budget)?;
        Ok(Self {
            path: path.to_path_buf(),
            length: u64::try_from(bytes.len())
                .map_err(|_| node::Error::Codec("cached Cargo input length is invalid"))?,
            digest: Sha256::digest(bytes).into(),
        })
    }
}

fn locked_recipe(
    authority: &[u8],
    manifest: &Path,
    lock: &Path,
    target: &Path,
) -> Result<Box<[u8]>, node::Error> {
    let mut writer = LockedWriter::new(node::MAX_CONTENT_BYTES.min(16 * 1024 * 1024));
    writer.append(b"rawr.cargo.locked-metadata-resolution/v1")?;
    writer.append(&LOCKED_RECIPE_SCHEMA.to_le_bytes())?;
    writer.bytes(authority)?;
    writer.path(manifest)?;
    writer.path(lock)?;
    writer.path(target)?;
    Ok(writer.finish())
}

struct LockedWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl LockedWriter {
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
                resource: "cached locked Cargo metadata bytes",
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

    fn count(&mut self, value: usize) -> Result<(), node::Error> {
        let value = u32::try_from(value)
            .map_err(|_| node::Error::Codec("cached Cargo count is not encodable"))?;
        self.append(&value.to_le_bytes())
    }

    fn bytes(&mut self, value: &[u8]) -> Result<(), node::Error> {
        let length = u64::try_from(value.len())
            .map_err(|_| node::Error::Codec("cached Cargo bytes are not encodable"))?;
        self.append(&length.to_le_bytes())?;
        self.append(value)
    }

    fn path(&mut self, path: &Path) -> Result<(), node::Error> {
        let path = path
            .to_str()
            .ok_or(node::Error::Codec("cached Cargo path is not Unicode"))?;
        self.count(path.len())?;
        self.append(path.as_bytes())
    }

    fn evidence(&mut self, evidence: &Evidence) -> Result<(), node::Error> {
        self.path(&evidence.path)?;
        self.append(&evidence.length.to_le_bytes())?;
        self.append(&evidence.digest)
    }

    fn finish(self) -> Box<[u8]> {
        self.bytes.into_boxed_slice()
    }
}

struct LockedReader<'bytes> {
    bytes: &'bytes [u8],
    offset: usize,
}

impl<'bytes> LockedReader<'bytes> {
    fn new(bytes: &'bytes [u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached locked Cargo metadata bytes",
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
            .ok_or(node::Error::Codec("cached Cargo metadata is truncated"))?;
        self.offset = end;
        Ok(bytes)
    }

    fn count(&mut self) -> Result<usize, node::Error> {
        self.take(4)?
            .try_into()
            .map(u32::from_le_bytes)
            .ok()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(node::Error::Codec("cached Cargo count is invalid"))
    }

    fn bytes(&mut self) -> Result<Box<[u8]>, node::Error> {
        let length = self
            .take(8)?
            .try_into()
            .map(u64::from_le_bytes)
            .ok()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(node::Error::Codec("cached Cargo byte length is invalid"))?;
        Ok(self.take(length)?.to_vec().into_boxed_slice())
    }

    fn path(&mut self) -> Result<PathBuf, node::Error> {
        let length = self.count()?;
        let path = std::str::from_utf8(self.take(length)?)
            .map_err(|_| node::Error::Codec("cached Cargo path is not UTF-8"))?;
        Ok(PathBuf::from(path))
    }

    fn evidence(&mut self) -> Result<Evidence, node::Error> {
        let path = self.path()?;
        let length = self
            .take(8)?
            .try_into()
            .map(u64::from_le_bytes)
            .map_err(|_| node::Error::Codec("cached Cargo file length is invalid"))?;
        let digest = self
            .take(32)?
            .try_into()
            .map_err(|_| node::Error::Codec("cached Cargo file digest is invalid"))?;
        Ok(Evidence {
            path,
            length,
            digest,
        })
    }

    fn finish(self) -> Result<(), node::Error> {
        if self.offset != self.bytes.len() {
            return Err(node::Error::Codec(
                "cached Cargo metadata has trailing bytes",
            ));
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::node::Value as _;

    const ROOT_ENV: &str = "RAWR_RESOLUTION_NODE_ROOT";
    const REPORT_ENV: &str = "RAWR_RESOLUTION_NODE_REPORT";
    const MODE_ENV: &str = "RAWR_RESOLUTION_NODE_MODE";
    const WORKER: &str = "cargo::resolution::tests::fresh_resolution_worker";

    #[derive(Default)]
    struct Counts {
        toolchain_hit: AtomicUsize,
        toolchain_miss: AtomicUsize,
        resolution_hit: AtomicUsize,
        resolution_miss: AtomicUsize,
        resolution_published: AtomicUsize,
        native_hit: AtomicUsize,
        native_miss: AtomicUsize,
        native_published: AtomicUsize,
        api_hit: AtomicUsize,
        api_miss: AtomicUsize,
        api_published: AtomicUsize,
        facade_hit: AtomicUsize,
        facade_miss: AtomicUsize,
        facade_published: AtomicUsize,
        profile_hit: AtomicUsize,
        profile_miss: AtomicUsize,
        profile_published: AtomicUsize,
        profile_proof_hit: AtomicUsize,
        profile_proof_miss: AtomicUsize,
        profile_proof_published: AtomicUsize,
        generic_hit: AtomicUsize,
        generic_miss: AtomicUsize,
        generic_published: AtomicUsize,
        checked_hit: AtomicUsize,
        checked_miss: AtomicUsize,
        checked_published: AtomicUsize,
        overlay_hit: AtomicUsize,
        overlay_miss: AtomicUsize,
        overlay_published: AtomicUsize,
        runner_hit: AtomicUsize,
        runner_miss: AtomicUsize,
        runner_published: AtomicUsize,
        cargo: AtomicUsize,
        cargo_runner: AtomicUsize,
        runner_process: AtomicUsize,
        admitted: AtomicUsize,
        processes: AtomicUsize,
    }

    impl metrics::Sink for Counts {
        fn node(&self, kind: metrics::NodeKind, event: metrics::NodeEvent) {
            let counter = match (kind, event) {
                (metrics::NodeKind::Toolchain, metrics::NodeEvent::ValidatedHit) => {
                    Some(&self.toolchain_hit)
                }
                (metrics::NodeKind::Toolchain, metrics::NodeEvent::ExecutedMiss) => {
                    Some(&self.toolchain_miss)
                }
                (metrics::NodeKind::Resolution, metrics::NodeEvent::ValidatedHit) => {
                    Some(&self.resolution_hit)
                }
                (metrics::NodeKind::Resolution, metrics::NodeEvent::ExecutedMiss) => {
                    Some(&self.resolution_miss)
                }
                (metrics::NodeKind::Resolution, metrics::NodeEvent::Published) => {
                    Some(&self.resolution_published)
                }
                (metrics::NodeKind::NativeProduct, metrics::NodeEvent::ValidatedHit) => {
                    Some(&self.native_hit)
                }
                (metrics::NodeKind::NativeProduct, metrics::NodeEvent::ExecutedMiss) => {
                    Some(&self.native_miss)
                }
                (metrics::NodeKind::NativeProduct, metrics::NodeEvent::Published) => {
                    Some(&self.native_published)
                }
                (metrics::NodeKind::Api, metrics::NodeEvent::ValidatedHit) => Some(&self.api_hit),
                (metrics::NodeKind::Api, metrics::NodeEvent::ExecutedMiss) => Some(&self.api_miss),
                (metrics::NodeKind::Api, metrics::NodeEvent::Published) => {
                    Some(&self.api_published)
                }
                (metrics::NodeKind::Facade, metrics::NodeEvent::ValidatedHit) => {
                    Some(&self.facade_hit)
                }
                (metrics::NodeKind::Facade, metrics::NodeEvent::ExecutedMiss) => {
                    Some(&self.facade_miss)
                }
                (metrics::NodeKind::Facade, metrics::NodeEvent::Published) => {
                    Some(&self.facade_published)
                }
                (metrics::NodeKind::Profile, metrics::NodeEvent::ValidatedHit) => {
                    Some(&self.profile_hit)
                }
                (metrics::NodeKind::Profile, metrics::NodeEvent::ExecutedMiss) => {
                    Some(&self.profile_miss)
                }
                (metrics::NodeKind::Profile, metrics::NodeEvent::Published) => {
                    Some(&self.profile_published)
                }
                (metrics::NodeKind::ProfileProof, metrics::NodeEvent::ValidatedHit) => {
                    Some(&self.profile_proof_hit)
                }
                (metrics::NodeKind::ProfileProof, metrics::NodeEvent::ExecutedMiss) => {
                    Some(&self.profile_proof_miss)
                }
                (metrics::NodeKind::ProfileProof, metrics::NodeEvent::Published) => {
                    Some(&self.profile_proof_published)
                }
                (metrics::NodeKind::Generic, metrics::NodeEvent::ValidatedHit) => {
                    Some(&self.generic_hit)
                }
                (metrics::NodeKind::Generic, metrics::NodeEvent::ExecutedMiss) => {
                    Some(&self.generic_miss)
                }
                (metrics::NodeKind::Generic, metrics::NodeEvent::Published) => {
                    Some(&self.generic_published)
                }
                (metrics::NodeKind::Checked, metrics::NodeEvent::ValidatedHit) => {
                    Some(&self.checked_hit)
                }
                (metrics::NodeKind::Checked, metrics::NodeEvent::ExecutedMiss) => {
                    Some(&self.checked_miss)
                }
                (metrics::NodeKind::Checked, metrics::NodeEvent::Published) => {
                    Some(&self.checked_published)
                }
                (metrics::NodeKind::Overlay, metrics::NodeEvent::ValidatedHit) => {
                    Some(&self.overlay_hit)
                }
                (metrics::NodeKind::Overlay, metrics::NodeEvent::ExecutedMiss) => {
                    Some(&self.overlay_miss)
                }
                (metrics::NodeKind::Overlay, metrics::NodeEvent::Published) => {
                    Some(&self.overlay_published)
                }
                (metrics::NodeKind::Runner, metrics::NodeEvent::ValidatedHit) => {
                    Some(&self.runner_hit)
                }
                (metrics::NodeKind::Runner, metrics::NodeEvent::ExecutedMiss) => {
                    Some(&self.runner_miss)
                }
                (metrics::NodeKind::Runner, metrics::NodeEvent::Published) => {
                    Some(&self.runner_published)
                }
                _ => None,
            };
            if let Some(counter) = counter {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        }

        fn process(&self, kind: metrics::ProcessKind) {
            if kind == metrics::ProcessKind::CargoResolution {
                self.cargo.fetch_add(1, Ordering::Relaxed);
            }
            if kind == metrics::ProcessKind::CargoRunner {
                self.cargo_runner.fetch_add(1, Ordering::Relaxed);
            }
            if kind == metrics::ProcessKind::Runner {
                self.runner_process.fetch_add(1, Ordering::Relaxed);
            }
            self.processes.fetch_add(1, Ordering::Relaxed);
        }

        fn milestone(&self, milestone: metrics::Milestone, _observed_ns: Option<u64>) {
            if milestone == metrics::Milestone::Admitted {
                self.admitted.fetch_add(1, Ordering::Relaxed);
            }
        }
        fn runner_owned(&self, _owned: &metrics::runner::Owned) {}
        #[cfg(feature = "import-baseline")]
        fn profile(&self, _evidence: &metrics::profile::Evidence) {}
        #[cfg(feature = "import-baseline")]
        fn pipeline(&self, _evidence: &metrics::pipeline::Evidence) {}
        fn scope_opened(&self) {}
        fn scope_joined(&self) {}
    }

    struct CachedProfileChecked {
        chain: Option<crate::node::Chain>,
    }
    struct CachedProfileDefinition;

    impl crate::profile::Checked for CachedProfileChecked {
        fn profile_fingerprint(&self) -> [u8; 32] {
            [7; 32]
        }

        fn node_chain(&self) -> Option<&crate::node::Chain> {
            self.chain.as_ref()
        }
    }

    impl CachedProfileChecked {
        const fn direct() -> Self {
            Self { chain: None }
        }

        fn replayed(checked: &crate::checked::Cached) -> Self {
            Self {
                chain: Some(checked.node_chain().clone()),
            }
        }
    }

    impl crate::profile::Definition<CachedProfileChecked> for CachedProfileDefinition {
        fn identity(&self) -> crate::profile::Identity {
            crate::profile::Identity::new(
                "rawr.profile.fresh-process",
                &super::super::Limits::standard(),
            )
            .expect("the fresh-process profile identity seals")
        }

        fn version(&self) -> crate::profile::Version {
            crate::profile::Version::new(1).expect("the fresh-process profile version seals")
        }

        fn classify(
            &self,
            _graph: &crate::api::Graph<crate::api::Extracted>,
            _output: &mut crate::profile::contract::Builder<'_>,
        ) -> Result<(), crate::profile::Error> {
            Ok(())
        }

        fn demand(
            &self,
            _contract: &crate::profile::Contract,
            _checked: &CachedProfileChecked,
            _output: &mut crate::profile::demand::Builder<'_>,
        ) -> Result<(), crate::profile::Error> {
            Ok(())
        }

        fn generate(
            &self,
            _demand: &crate::profile::Demand,
            _output: &mut crate::profile::demand::Generator<'_>,
        ) -> Result<(), crate::profile::Error> {
            Ok(())
        }
    }

    fn runner_snapshot(
        limits: &super::super::Limits,
    ) -> (crate::surface::Snapshot, crate::supervisor::snapshot::Bytes) {
        const ENTRY: &str = "application.rs";
        const SOURCE: &[u8] = b"full cached owner DAG";
        let mut snapshot = crate::surface::Snapshot::builder(limits);
        snapshot
            .entry(ENTRY)
            .expect("the full-DAG snapshot entry is bounded");
        snapshot
            .file(ENTRY, SOURCE)
            .expect("the full-DAG snapshot source is bounded");
        let snapshot = snapshot.seal().expect("the full-DAG snapshot seals");

        let mut transport = crate::supervisor::snapshot::Bytes::capture(limits);
        transport
            .entry(ENTRY)
            .expect("the full-DAG transport entry is bounded");
        transport
            .file(ENTRY, SOURCE)
            .expect("the full-DAG transport source is bounded");
        let transport = transport
            .bind(snapshot, limits)
            .expect("the full-DAG transport matches its snapshot");
        (snapshot, transport)
    }

    #[test]
    fn canonical_codec_round_trips_and_rejects_trailing_bytes() {
        let metadata = Metadata {
            manifest: [1; 32],
            lock: [2; 32],
            canonical: br#"{"packages":[]}"#.to_vec().into_boxed_slice(),
        };
        let limit = super::super::Limits::standard().node_output_bytes();
        let bytes = metadata.encode(limit).expect("the metadata encodes");
        let decoded = Metadata::decode(&bytes, limit).expect("the metadata decodes");
        assert_eq!(
            decoded.encode(limit).expect("the metadata reencodes"),
            bytes
        );
        let mut trailing = bytes.into_vec();
        trailing.push(0);
        assert!(matches!(
            Metadata::decode(&trailing, limit),
            Err(node::Error::Codec("Cargo metadata payload is truncated"))
        ));
    }

    #[test]
    fn leaf_authority_rejects_project_or_cargo_home_config_presence() {
        let project = temporary("resolution-config");
        let root = project.path().to_path_buf();
        prepare_fixture(&root);
        let environment = super::super::config::Environment::from_pairs([(
            "CARGO_HOME",
            root.join("cargo-home")
                .to_str()
                .expect("the fixture Cargo home is Unicode"),
        )])
        .expect("the fixture environment seals");
        let limits = super::super::Limits::standard();
        let context = Context::capture(
            &root.join("project"),
            environment,
            super::super::toolchain::HOST,
            &limits,
        )
        .expect("the fixture context seals");
        require_configuration_free(&context, &limits)
            .expect("the exact manifest and lock are initially complete");

        fs::create_dir(root.join("project/.cargo")).expect("the project config directory exists");
        fs::write(
            root.join("project/.cargo/config.toml"),
            b"[net]\noffline = true\n",
        )
        .expect("the project config writes");
        assert!(matches!(
            require_configuration_free(&context, &limits),
            Err(node::Error::Codec(
                "cached leaf Cargo resolution does not admit config files"
            ))
        ));
        fs::remove_dir_all(root.join("project/.cargo"))
            .expect("the project config directory retires");

        fs::write(root.join("cargo-home/config"), b"[net]\noffline = true\n")
            .expect("the Cargo-home config writes");
        assert!(matches!(
            require_configuration_free(&context, &limits),
            Err(node::Error::Codec(
                "cached leaf Cargo resolution does not admit config files"
            ))
        ));
        fs::remove_dir_all(root).expect("the config fixture retires");
    }

    /// Package-manifest evidence is I/O retention bounded by the physical
    /// tail constant (locks by the capture constant); a lowered manifest
    /// accounting target never refuses it.
    #[test]
    fn locked_manifest_evidence_binds_to_the_physical_tail_bound() {
        assert_eq!(process::TAIL_BYTES, 1_048_576);
        assert_eq!(process::CAPTURE_BYTES, 256 * 1024 * 1024);
        let fixture = prepare_locked_fixture("locked-manifest-bound");
        let mut limits = super::super::Limits::standard();
        limits.manifest_bytes = 1;
        let raw = locked_metadata(&[&fixture.manifest, &fixture.dependency_manifest]);
        let locked = Locked::capture(raw, &fixture.manifest, &fixture.lock, &limits)
            .expect("manifests above the accounting target still capture");
        locked
            .revalidate(&fixture.lock, &limits, &node::GraphBudget::new())
            .expect("manifests above the accounting target still revalidate");
    }

    #[test]
    fn locked_codec_round_trips_and_rejects_manifest_or_shape_drift() {
        let fixture = prepare_locked_fixture("locked-codec");
        let limits = super::super::Limits::standard();
        let raw = locked_metadata(&[&fixture.manifest, &fixture.dependency_manifest]);
        let locked = Locked::capture(raw, &fixture.manifest, &fixture.lock, &limits)
            .expect("the locked metadata captures every manifest");
        let limit = limits.node_input_bytes();
        let bytes = locked.encode(limit).expect("the locked metadata encodes");
        let decoded = Locked::decode(&bytes, limit).expect("the locked metadata decodes");
        decoded
            .validate_shape(&limits)
            .expect("the decoded manifest closure is exact");
        decoded
            .revalidate(&fixture.lock, &limits, &node::GraphBudget::new())
            .expect("the decoded input closure revalidates");
        assert_eq!(
            decoded.encode(limit).expect("the metadata reencodes"),
            bytes
        );

        let mut reordered = Locked::decode(&bytes, limit).expect("the fixture decodes again");
        reordered.manifests.swap(0, 1);
        assert!(matches!(
            reordered.validate_shape(&limits),
            Err(node::Error::Codec(
                "cached Cargo metadata manifest evidence changed"
                    | "cached Cargo metadata manifest ordering changed"
            ))
        ));

        fs::write(
            &fixture.dependency_manifest,
            b"[package]\nname = \"rawr-resolution-dependency\"\nversion = \"0.0.1\"\n",
        )
        .expect("the dependency manifest mutates");
        assert!(matches!(
            decoded.revalidate(&fixture.lock, &limits, &node::GraphBudget::new()),
            Err(node::Error::Codec(
                "cached Cargo package-manifest evidence changed"
            ))
        ));

        let mut trailing = bytes.into_vec();
        trailing.push(0);
        assert!(matches!(
            Locked::decode(&trailing, limit),
            Err(node::Error::Codec(
                "cached Cargo metadata has trailing bytes"
            ))
        ));
        fs::remove_dir_all(fixture.root).expect("the locked-codec fixture retires");
    }

    #[test]
    fn fresh_locked_metadata_stages_hold_execution_and_failure_releases_without_publication() {
        fn held(path: &Path) -> bool {
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .expect("the execution lease exists");
            let mut lease = fd_lock::RwLock::new(file);
            match lease.try_write() {
                Ok(_) => false,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
                Err(error) => panic!("lease probe failed: {error}"),
            }
        }
        let fixture = prepare_locked_fixture("fresh-metadata-ownership");
        let raw = locked_metadata(&[&fixture.manifest, &fixture.dependency_manifest]);
        let counts = Counts::default();
        let authority = b"fresh-locked-metadata-ownership";
        for fail_at in 0..3 {
            let context = locked_context(&fixture);
            let store = context.open_store().expect("the stage store opens");
            let recipe = context
                .discover::<Locked>(
                    &locked_recipe(authority, &fixture.manifest, &fixture.lock, &fixture.target)
                        .expect("the recipe authority seals"),
                    context.limits().node_input_bytes(),
                )
                .expect("the locked recipe seals");
            let exact = store
                .root()
                .join(format!("locks/executions/{}.lock", recipe.execution()));
            let mut stages = Vec::new();
            let result = cached_locked(
                &context,
                authority,
                &fixture.manifest,
                &fixture.lock,
                &fixture.target,
                &counts,
                || {
                    for stage in ["provisional", "candidate", "locked"] {
                        assert!(held(&store.root().join("locks/heavy.lock")));
                        assert!(held(&exact));
                        assert!(!held(&store.root().join("locks/publication.lock")));
                        stages.push(stage);
                        if stages.len() == fail_at + 1 {
                            return Err(crate::Error::Owner {
                                state: "injected metadata failure",
                            });
                        }
                    }
                    Ok(raw.clone())
                },
            );
            assert!(matches!(result, Err(node::Error::Importer(_))));
            assert_eq!(stages.len(), fail_at + 1);
            assert_eq!(counts.resolution_published.load(Ordering::Relaxed), 0);
            assert!(!held(&store.root().join("locks/heavy.lock")));
            assert!(!held(&exact));
            assert_eq!(
                store.fixture_usage().expect("reservation cleanup reads").3,
                0
            );
        }
        let context = locked_context(&fixture);
        cached_locked(
            &context,
            authority,
            &fixture.manifest,
            &fixture.lock,
            &fixture.target,
            &counts,
            || Ok(raw.clone()),
        )
        .expect("a clean miss succeeds after every injected failure");
        let warm = locked_context(&fixture);
        let store = warm.open_store().expect("the warm store opens");
        let (sent, received) = std::sync::mpsc::channel();
        std::thread::scope(|threads| {
            let result = store
                .with_heavy_lease(|| {
                    threads.spawn(|| {
                        let result = cached_locked(
                            &warm,
                            authority,
                            &fixture.manifest,
                            &fixture.lock,
                            &fixture.target,
                            &counts,
                            || panic!("a healthy metadata hit skips every construction stage"),
                        );
                        sent.send(result.is_ok())
                            .expect("warm stage result reports");
                    });
                    Ok(received.recv_timeout(Duration::from_secs(5)))
                })
                .expect("the external heavy lease releases");
            assert!(result.expect("warm metadata returns while heavy is held"));
        });
        assert_eq!(counts.resolution_published.load(Ordering::Relaxed), 1);
        fs::remove_dir_all(fixture.root).expect("the fresh stage fixture retires");
    }

    #[test]
    fn cached_locked_reuses_without_execution_and_invalidates_dependency_manifest() {
        let fixture = prepare_locked_fixture("locked-reuse");
        let raw = locked_metadata(&[&fixture.manifest, &fixture.dependency_manifest]);
        let executions = AtomicUsize::new(0);
        let counts = Counts::default();

        let first_context = locked_context(&fixture);
        let first = cached_locked(
            &first_context,
            b"locked-metadata-authority",
            &fixture.manifest,
            &fixture.lock,
            &fixture.target,
            &counts,
            || {
                executions.fetch_add(1, Ordering::Relaxed);
                Ok(raw.clone())
            },
        )
        .expect("the cold locked metadata node executes and publishes");
        assert_eq!(first.value().raw(), raw.as_ref());
        drop(first);
        assert_eq!(executions.load(Ordering::Relaxed), 1);
        assert_eq!(counts.resolution_miss.load(Ordering::Relaxed), 1);
        assert_eq!(counts.resolution_published.load(Ordering::Relaxed), 1);
        let sealed = snapshot(&fixture.project.join("target/rawr/store"));

        let warm_context = locked_context(&fixture);
        let warm = cached_locked(
            &warm_context,
            b"locked-metadata-authority",
            &fixture.manifest,
            &fixture.lock,
            &fixture.target,
            &counts,
            || -> crate::Result<Box<[u8]>> {
                executions.fetch_add(1, Ordering::Relaxed);
                panic!("a validated warm locked-metadata node must not execute")
            },
        )
        .expect("the warm locked metadata node validates from the store");
        assert_eq!(warm.value().raw(), raw.as_ref());
        drop(warm);
        assert_eq!(executions.load(Ordering::Relaxed), 1);
        assert_eq!(counts.resolution_hit.load(Ordering::Relaxed), 1);
        assert_eq!(snapshot(&fixture.project.join("target/rawr/store")), sealed);

        fs::write(
            &fixture.dependency_manifest,
            b"[package]\nname = \"rawr-resolution-dependency\"\nversion = \"0.0.1\"\n",
        )
        .expect("the dependency manifest mutates");
        let changed_context = locked_context(&fixture);
        let changed = cached_locked(
            &changed_context,
            b"locked-metadata-authority",
            &fixture.manifest,
            &fixture.lock,
            &fixture.target,
            &counts,
            || {
                executions.fetch_add(1, Ordering::Relaxed);
                Ok(raw.clone())
            },
        )
        .expect("dependency-manifest drift executes a new node");
        assert_eq!(changed.value().raw(), raw.as_ref());
        assert_eq!(executions.load(Ordering::Relaxed), 2);
        assert_eq!(counts.resolution_miss.load(Ordering::Relaxed), 2);
        assert_eq!(counts.resolution_published.load(Ordering::Relaxed), 2);
        fs::remove_dir_all(fixture.root).expect("the locked-reuse fixture retires");
    }

    #[test]
    fn cached_unit_graph_reuses_parented_output_and_misses_after_parent_drift() {
        let fixture = prepare_locked_fixture("unit-reuse");
        let metadata = locked_metadata(&[&fixture.manifest, &fixture.dependency_manifest]);
        let unit = br#"{"version":1,"units":[],"roots":[0]}"#
            .to_vec()
            .into_boxed_slice();
        let metadata_executions = AtomicUsize::new(0);
        let unit_executions = AtomicUsize::new(0);
        let counts = Counts::default();

        let cold_context = locked_context(&fixture);
        let cold_metadata = cached_locked(
            &cold_context,
            b"locked-metadata-authority",
            &fixture.manifest,
            &fixture.lock,
            &fixture.target,
            &counts,
            || {
                metadata_executions.fetch_add(1, Ordering::Relaxed);
                Ok(metadata.clone())
            },
        )
        .expect("the cold metadata parent publishes");
        let cold_unit = cached_unit_graph(
            &cold_context,
            &cold_metadata,
            b"unit-graph-authority",
            &counts,
            || {
                unit_executions.fetch_add(1, Ordering::Relaxed);
                Ok(unit.clone())
            },
        )
        .expect("the cold unit graph publishes");
        assert_eq!(cold_unit.value().raw(), unit.as_ref());
        drop(cold_unit);
        drop(cold_metadata);

        let warm_context = locked_context(&fixture);
        let warm_metadata = cached_locked(
            &warm_context,
            b"locked-metadata-authority",
            &fixture.manifest,
            &fixture.lock,
            &fixture.target,
            &counts,
            || -> crate::Result<Box<[u8]>> { panic!("the warm metadata parent must not execute") },
        )
        .expect("the warm metadata parent validates");
        let warm_unit = cached_unit_graph(
            &warm_context,
            &warm_metadata,
            b"unit-graph-authority",
            &counts,
            || -> crate::Result<Box<[u8]>> {
                panic!("the warm parented unit graph must not execute")
            },
        )
        .expect("the warm unit graph validates");
        assert_eq!(warm_unit.value().raw(), unit.as_ref());
        drop(warm_unit);
        drop(warm_metadata);
        assert_eq!(metadata_executions.load(Ordering::Relaxed), 1);
        assert_eq!(unit_executions.load(Ordering::Relaxed), 1);

        fs::write(
            &fixture.dependency_manifest,
            b"[package]\nname = \"rawr-resolution-dependency\"\nversion = \"0.0.1\"\n",
        )
        .expect("the dependency manifest mutates");
        let changed_context = locked_context(&fixture);
        let changed_metadata = cached_locked(
            &changed_context,
            b"locked-metadata-authority",
            &fixture.manifest,
            &fixture.lock,
            &fixture.target,
            &counts,
            || {
                metadata_executions.fetch_add(1, Ordering::Relaxed);
                Ok(metadata.clone())
            },
        )
        .expect("the changed metadata parent republishes");
        let changed_unit = cached_unit_graph(
            &changed_context,
            &changed_metadata,
            b"unit-graph-authority",
            &counts,
            || {
                unit_executions.fetch_add(1, Ordering::Relaxed);
                Ok(unit.clone())
            },
        )
        .expect("the changed parent forces a new unit graph");
        assert_eq!(changed_unit.value().raw(), unit.as_ref());
        assert_eq!(metadata_executions.load(Ordering::Relaxed), 2);
        assert_eq!(unit_executions.load(Ordering::Relaxed), 2);
        drop(changed_unit);
        drop(changed_metadata);

        assert!(matches!(
            UnitGraph::capture(
                br#"{"version":1,"units":[],"roots":[]}"#.to_vec().into_boxed_slice(),
                &super::super::Limits::standard(),
            ),
            Err(node::Error::Importer(_))
        ));
        fs::remove_dir_all(fixture.root).expect("the unit-reuse fixture retires");
    }

    #[test]
    #[ignore = "launched only by the real fresh-process resolution-node proof"]
    fn fresh_resolution_worker() {
        let root = PathBuf::from(
            std::env::var_os(ROOT_ENV).expect("the worker receives its fixture root"),
        );
        let report_path = PathBuf::from(
            std::env::var_os(REPORT_ENV).expect("the worker receives its report path"),
        );
        let mode = std::env::var(MODE_ENV).expect("the worker receives its mode");
        let environment = super::super::config::Environment::capture()
            .expect("the worker environment captures")
            .removing(ROOT_ENV)
            .expect("the root control is excluded")
            .removing(REPORT_ENV)
            .expect("the report control is excluded")
            .removing(MODE_ENV)
            .expect("the mode control is excluded")
            .setting(
                "CARGO_HOME",
                root.join("cargo-home")
                    .to_str()
                    .expect("the Cargo home is Unicode"),
            )
            .expect("the Cargo home is sealed");
        let context = Context::capture(
            &root.join("project"),
            environment,
            super::super::toolchain::HOST,
            &super::super::Limits::standard(),
        )
        .expect("the resolution context seals");
        let counts = Arc::new(Counts::default());
        let scope = metrics::Scope::transform(Arc::clone(&counts) as Arc<dyn metrics::Sink>)
            .expect("the worker installs its metric sink");
        let toolchain = super::super::toolchain::cached(&context, counts.as_ref())
            .expect("the toolchain parent validates");
        if mode == "resolution" {
            let resolved = cached(&context, &toolchain, counts.as_ref())
                .expect("the leaf resolution validates");
            assert!(!resolved.value().canonical().is_empty());
        } else if mode == "chain"
            || mode == "build-chain"
            || mode == "api-chain"
            || mode == "facade-chain"
            || mode == "profile-chain"
            || mode == "runner-chain"
        {
            let manifest = context.project().join("Cargo.toml");
            let lock = context.project().join("Cargo.lock");
            let target = context.project().join("target");
            let metadata = cached_locked_with_toolchain(
                &context,
                &toolchain,
                b"fresh-process-locked-metadata/v1",
                &manifest,
                &lock,
                &target,
                counts.as_ref(),
                || execute_leaf_metadata(&context, &toolchain, &manifest, &target),
            )
            .expect("the path-bound metadata node validates");
            let unit = cached_unit_graph(
                &context,
                &metadata,
                b"fresh-process-unit-graph/v1",
                counts.as_ref(),
                || execute_leaf_unit_graph(&context, &toolchain, &manifest, &target),
            )
            .expect("the parented unit-graph node validates");
            assert!(!metadata.value().raw().is_empty());
            assert!(!unit.value().raw().is_empty());
            if mode == "build-chain"
                || mode == "api-chain"
                || mode == "facade-chain"
                || mode == "profile-chain"
                || mode == "runner-chain"
            {
                let protocol = context.project().join("protocol");
                fs::create_dir_all(&target).expect("the build target exists");
                fs::create_dir_all(&protocol).expect("the protocol root exists");
                let build = cached_build(
                    &context,
                    &unit,
                    b"fresh-process-authoritative-build/v1",
                    &target,
                    &protocol,
                    counts.as_ref(),
                    || {
                        let raw = execute_leaf_build(&context, &toolchain, &manifest, &target)?;
                        fs::write(protocol.join("sidecar.json"), b"sidecar\n").map_err(
                            |source| crate::Error::Io {
                                operation: "write build-chain protocol fixture",
                                path: protocol.join("sidecar.json"),
                                source,
                            },
                        )?;
                        Ok(raw)
                    },
                )
                .expect("the authoritative build product validates");
                assert!(!build.value().raw().is_empty());
                assert!(target.join("debug").is_dir());
                assert_eq!(
                    fs::read(protocol.join("sidecar.json")).expect("the restored sidecar reads"),
                    b"sidecar\n"
                );
                let post_unit = cached_post_unit_graph(
                    &context,
                    &build,
                    b"fresh-process-post-build-unit/v1",
                    counts.as_ref(),
                    || execute_leaf_unit_graph(&context, &toolchain, &manifest, &target),
                )
                .expect("the post-build unit graph validates");
                let post_metadata = cached_post_metadata(
                    &context,
                    &build,
                    b"fresh-process-post-build-metadata/v1",
                    &manifest,
                    &lock,
                    &target,
                    counts.as_ref(),
                    || execute_leaf_metadata(&context, &toolchain, &manifest, &target),
                )
                .expect("the post-build metadata validates");
                assert_eq!(post_unit.value().raw(), unit.value().raw());
                assert_eq!(post_metadata.value().raw(), metadata.value().raw());
                if mode == "api-chain"
                    || mode == "facade-chain"
                    || mode == "profile-chain"
                    || mode == "runner-chain"
                {
                    let (api, graph) = crate::api::cached::evaluate(
                        &context,
                        &[42; 32],
                        &crate::extract::Policy::empty()
                            .cache_identity(context.graph_budget())
                            .expect("fixture policy identity"),
                        &build,
                        &post_unit,
                        &post_metadata,
                        counts.as_ref(),
                        |stored| {
                            let limits = super::super::Limits::standard();
                            let mut allocation = crate::extract::Allocation::new(&limits);
                            let context = crate::api::Context::fixture();
                            if let Some(stored) = stored {
                                return stored.replay(
                                    &context,
                                    Vec::new(),
                                    &limits,
                                    &mut allocation,
                                );
                            }
                            crate::api::Graph::seal(
                                &context,
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                limits.node_canonical_bytes(),
                                &mut allocation,
                            )
                        },
                    )
                    .expect("the captured API graph validates");
                    assert!(!graph.canonical_bytes().is_empty());
                    if mode == "facade-chain" {
                        let chain = crate::node::Chain::validated(context.clone(), &api);
                        let graph = graph
                            .publish_with_chain(crate::input::proof::Proof::fixture(), Some(chain))
                            .expect("the fixture API graph publishes with its current chain");
                        let contract = graph
                            .classify(
                                &crate::classify::Policy::standard(),
                                &super::super::Limits::standard(),
                            )
                            .expect("the fixture API graph classifies");
                        let facade = crate::surface::facade::Plan::encode_cached(
                            &contract,
                            &super::super::Limits::standard(),
                        )
                        .expect("the facade node validates");
                        assert!(!facade.bytes().is_empty());
                    } else if mode == "profile-chain" {
                        let chain = crate::node::Chain::validated(context.clone(), &api);
                        let graph = graph
                            .publish_with_chain(crate::input::proof::Proof::fixture(), Some(chain))
                            .expect("the fixture API graph publishes with its current chain");
                        let profile = crate::profile::classify(
                            &CachedProfileDefinition,
                            &graph,
                            &super::super::Limits::standard(),
                        )
                        .expect("the profile classification node validates");
                        assert_eq!(profile.graph_fingerprint(), graph.fingerprint());
                        assert!(profile.node_chain().is_some());
                        let checked = CachedProfileChecked::direct();
                        let demand = crate::profile::demand(
                            &CachedProfileDefinition,
                            &profile,
                            &checked,
                            &super::super::Limits::standard(),
                        )
                        .expect("the profile demand node validates");
                        assert!(demand.node_chain().is_some());
                        let proof = crate::profile::Proof::generate(
                            &CachedProfileDefinition,
                            &demand,
                            &super::super::Limits::standard(),
                        )
                        .expect("the generated profile proof node validates");
                        assert!(proof.node_chain().is_some());
                    } else if mode == "runner-chain" {
                        let limits = super::super::Limits::standard();
                        let chain = crate::node::Chain::validated(context.clone(), &api);
                        let graph = graph
                            .publish_with_chain(crate::input::proof::Proof::fixture(), Some(chain))
                            .expect("the full-DAG API graph publishes with its current chain");
                        let profile =
                            crate::profile::classify(&CachedProfileDefinition, &graph, &limits)
                                .expect("the full-DAG profile classification validates");
                        let contract = graph
                            .classify(&crate::classify::Policy::standard(), &limits)
                            .expect("the full-DAG generic classification validates");
                        let facade =
                            crate::surface::facade::Plan::encode_cached(&contract, &limits)
                                .expect("the full-DAG facade validates");
                        let (snapshot, transport) = runner_snapshot(&limits);
                        let checked = crate::checked::evaluate(
                            &contract,
                            &profile,
                            &facade,
                            snapshot,
                            &limits,
                            || {
                                let compiler =
                                    contract.compiler((), snapshot, &limits).map_err(|_| ())?;
                                let demand = compiler.seal().map_err(|_| ())?;
                                Ok::<_, ()>((demand, CachedProfileChecked::direct()))
                            },
                        )
                        .unwrap_or_else(|_| panic!("the full-DAG checked node validates"));
                        let checked_evidence = CachedProfileChecked::replayed(&checked);
                        let demand = checked
                            .replay_demand((), &contract, snapshot, &limits)
                            .expect("the full-DAG checked demand replays");
                        let profile_demand = crate::profile::demand(
                            &CachedProfileDefinition,
                            &profile,
                            &checked_evidence,
                            &limits,
                        )
                        .expect("the full-DAG profile demand validates");
                        let profile = crate::profile::Proof::generate(
                            &CachedProfileDefinition,
                            &profile_demand,
                            &limits,
                        )
                        .expect("the full-DAG generated profile validates");
                        let mut supervisor = crate::supervisor::Owner::new();
                        let request = crate::bridge::fixture_generation_from_demand(
                            demand,
                            profile,
                            context.project(),
                            supervisor
                                .revision()
                                .expect("the full-DAG runner revision seals"),
                        )
                        .expect("the full-DAG generation request seals");
                        let generated = request
                            .generate(&limits)
                            .expect("the full-DAG overlay validates");
                        let cache = crate::cache::Writer::new(context.project())
                            .expect("the full-DAG local runner cache binds");
                        let cargo = super::super::Owner::acquire()
                            .expect("the full-DAG Cargo runner lane acquires");
                        let built = generated
                            .build(&cargo, &cache)
                            .expect("the full-DAG runner validates");
                        drop(cargo);
                        let process = crate::process::Owner::new(context.project())
                            .expect("the full-DAG process socket seals");
                        let admitted = supervisor
                            .bootstrap(built, transport, &process)
                            .expect("the full-DAG runner reaches admitted state");
                        admitted
                            .shutdown()
                            .expect("the full-DAG runner exits and reaps");
                    }
                }
            }
        } else {
            assert_eq!(mode, "toolchain");
        }
        drop(scope);
        let report = format!(
            "toolchain_hits={}\ntoolchain_misses={}\nresolution_hits={}\nresolution_misses={}\nresolution_published={}\nnative_hits={}\nnative_misses={}\nnative_published={}\napi_hits={}\napi_misses={}\napi_published={}\ngeneric_hits={}\ngeneric_misses={}\ngeneric_published={}\nfacade_hits={}\nfacade_misses={}\nfacade_published={}\nchecked_hits={}\nchecked_misses={}\nchecked_published={}\nprofile_hits={}\nprofile_misses={}\nprofile_published={}\nprofile_proof_hits={}\nprofile_proof_misses={}\nprofile_proof_published={}\noverlay_hits={}\noverlay_misses={}\noverlay_published={}\nrunner_hits={}\nrunner_misses={}\nrunner_published={}\ncargo={}\ncargo_runner={}\nrunner_process={}\nadmitted={}\nprocesses={}\n",
            counts.toolchain_hit.load(Ordering::Relaxed),
            counts.toolchain_miss.load(Ordering::Relaxed),
            counts.resolution_hit.load(Ordering::Relaxed),
            counts.resolution_miss.load(Ordering::Relaxed),
            counts.resolution_published.load(Ordering::Relaxed),
            counts.native_hit.load(Ordering::Relaxed),
            counts.native_miss.load(Ordering::Relaxed),
            counts.native_published.load(Ordering::Relaxed),
            counts.api_hit.load(Ordering::Relaxed),
            counts.api_miss.load(Ordering::Relaxed),
            counts.api_published.load(Ordering::Relaxed),
            counts.generic_hit.load(Ordering::Relaxed),
            counts.generic_miss.load(Ordering::Relaxed),
            counts.generic_published.load(Ordering::Relaxed),
            counts.facade_hit.load(Ordering::Relaxed),
            counts.facade_miss.load(Ordering::Relaxed),
            counts.facade_published.load(Ordering::Relaxed),
            counts.checked_hit.load(Ordering::Relaxed),
            counts.checked_miss.load(Ordering::Relaxed),
            counts.checked_published.load(Ordering::Relaxed),
            counts.profile_hit.load(Ordering::Relaxed),
            counts.profile_miss.load(Ordering::Relaxed),
            counts.profile_published.load(Ordering::Relaxed),
            counts.profile_proof_hit.load(Ordering::Relaxed),
            counts.profile_proof_miss.load(Ordering::Relaxed),
            counts.profile_proof_published.load(Ordering::Relaxed),
            counts.overlay_hit.load(Ordering::Relaxed),
            counts.overlay_miss.load(Ordering::Relaxed),
            counts.overlay_published.load(Ordering::Relaxed),
            counts.runner_hit.load(Ordering::Relaxed),
            counts.runner_miss.load(Ordering::Relaxed),
            counts.runner_published.load(Ordering::Relaxed),
            counts.cargo.load(Ordering::Relaxed),
            counts.cargo_runner.load(Ordering::Relaxed),
            counts.runner_process.load(Ordering::Relaxed),
            counts.admitted.load(Ordering::Relaxed),
            counts.processes.load(Ordering::Relaxed),
        );
        fs::write(report_path, report.as_bytes()).expect("the worker report writes");
    }

    #[test]
    #[ignore = "requires the installed pinned toolchain"]
    fn cached_leaf_resolution_fresh_process_hit_launches_zero_cargo_children() {
        let project = temporary("resolution-node");
        let root = project.path().to_path_buf();
        prepare_fixture(&root);
        let setup_report = root.with_extension("setup.report");
        let first_report = root.with_extension("first.report");
        let second_report = root.with_extension("second.report");
        let _ = run_worker(&root, &setup_report, "toolchain");
        let setup = parse_report(&setup_report);
        assert_eq!(setup.get("toolchain_misses"), Some(&1));

        let first_elapsed = run_worker(&root, &first_report, "resolution");
        let first = parse_report(&first_report);
        assert_eq!(first.get("toolchain_hits"), Some(&1));
        assert_eq!(first.get("resolution_hits"), Some(&0));
        assert_eq!(first.get("resolution_misses"), Some(&1));
        assert_eq!(first.get("resolution_published"), Some(&1));
        assert_eq!(first.get("cargo"), Some(&1));
        assert!(first
            .get("processes")
            .is_some_and(|processes| *processes >= 1));
        let sealed = snapshot(&root.join("project/target/rawr/store"));

        let second_elapsed = run_worker(&root, &second_report, "resolution");
        let second = parse_report(&second_report);
        assert_eq!(second.get("toolchain_hits"), Some(&1));
        assert_eq!(second.get("resolution_hits"), Some(&1));
        assert_eq!(second.get("resolution_misses"), Some(&0));
        assert_eq!(second.get("resolution_published"), Some(&0));
        assert_eq!(second.get("cargo"), Some(&0));
        assert_eq!(second.get("processes"), Some(&0));
        assert_eq!(snapshot(&root.join("project/target/rawr/store")), sealed);
        let saved = first_elapsed.saturating_sub(second_elapsed);
        eprintln!(
            "RESOLUTION_NODE first_ms={} second_ms={} saved_ms={} first_cargo=1 first_processes={} second_cargo=0 second_processes=0",
            first_elapsed.as_millis(),
            second_elapsed.as_millis(),
            saved.as_millis(),
            first.get("processes").copied().unwrap_or_default(),
        );
        for report in [setup_report, first_report, second_report] {
            fs::remove_file(report).expect("the report retires");
        }
        fs::remove_dir_all(root).expect("the resolution fixture retires");
    }

    #[test]
    #[ignore = "requires the installed pinned toolchain"]
    fn cached_resolution_chain_warm_process_launches_zero_cargo_children() {
        let project = temporary("resolution-chain");
        let root = project.path().to_path_buf();
        prepare_fixture(&root);
        let setup_report = root.with_extension("chain-setup.report");
        let first_report = root.with_extension("chain-first.report");
        let second_report = root.with_extension("chain-second.report");
        let _ = run_worker(&root, &setup_report, "toolchain");

        let first_elapsed = run_worker(&root, &first_report, "chain");
        let first = parse_report(&first_report);
        assert_eq!(first.get("toolchain_hits"), Some(&1));
        assert_eq!(first.get("resolution_hits"), Some(&0));
        assert_eq!(first.get("resolution_misses"), Some(&2));
        assert_eq!(first.get("resolution_published"), Some(&2));
        assert_eq!(first.get("cargo"), Some(&2));
        assert!(first
            .get("processes")
            .is_some_and(|processes| *processes >= 2));
        let sealed = snapshot(&root.join("project/target/rawr/store"));

        let second_elapsed = run_worker(&root, &second_report, "chain");
        let second = parse_report(&second_report);
        assert_eq!(second.get("toolchain_hits"), Some(&1));
        assert_eq!(second.get("resolution_hits"), Some(&2));
        assert_eq!(second.get("resolution_misses"), Some(&0));
        assert_eq!(second.get("resolution_published"), Some(&0));
        assert_eq!(second.get("cargo"), Some(&0));
        assert_eq!(second.get("processes"), Some(&0));
        assert_eq!(snapshot(&root.join("project/target/rawr/store")), sealed);
        let saved = first_elapsed.saturating_sub(second_elapsed);
        eprintln!(
            "RESOLUTION_CHAIN first_ms={} second_ms={} saved_ms={} first_cargo=2 first_processes={} second_cargo=0 second_processes=0",
            first_elapsed.as_millis(),
            second_elapsed.as_millis(),
            saved.as_millis(),
            first.get("processes").copied().unwrap_or_default(),
        );
        for report in [setup_report, first_report, second_report] {
            fs::remove_file(report).expect("the chain report retires");
        }
        fs::remove_dir_all(root).expect("the resolution-chain fixture retires");
    }

    #[test]
    #[ignore = "requires the installed pinned toolchain"]
    fn cached_build_chain_warm_process_restores_artifacts_with_zero_cargo_children() {
        let project = temporary("build-chain");
        let root = project.path().to_path_buf();
        prepare_fixture(&root);
        let setup_report = root.with_extension("build-setup.report");
        let first_report = root.with_extension("build-first.report");
        let second_report = root.with_extension("build-second.report");
        let _ = run_worker(&root, &setup_report, "toolchain");

        let first_elapsed = run_worker(&root, &first_report, "build-chain");
        let first = parse_report(&first_report);
        assert_eq!(first.get("toolchain_hits"), Some(&1));
        assert_eq!(first.get("resolution_misses"), Some(&4));
        assert_eq!(first.get("native_misses"), Some(&1));
        assert_eq!(first.get("native_published"), Some(&1));
        assert_eq!(first.get("cargo"), Some(&5));
        assert!(first
            .get("processes")
            .is_some_and(|processes| *processes >= 3));
        let sealed = snapshot(&root.join("project/target/rawr/store"));
        fs::remove_dir_all(root.join("project/target")).expect("cold target retires");
        fs::remove_dir_all(root.join("project/protocol")).expect("cold protocol retires");

        let second_elapsed = run_worker(&root, &second_report, "build-chain");
        let second = parse_report(&second_report);
        assert_eq!(second.get("toolchain_hits"), Some(&1));
        assert_eq!(second.get("resolution_hits"), Some(&4));
        assert_eq!(second.get("native_hits"), Some(&1));
        assert_eq!(second.get("resolution_misses"), Some(&0));
        assert_eq!(second.get("native_misses"), Some(&0));
        assert_eq!(second.get("native_published"), Some(&0));
        assert_eq!(second.get("cargo"), Some(&0));
        assert_eq!(second.get("processes"), Some(&0));
        assert!(root.join("project/target/debug").is_dir());
        assert_eq!(
            fs::read(root.join("project/protocol/sidecar.json"))
                .expect("the warm sidecar restores"),
            b"sidecar\n"
        );
        assert_eq!(snapshot(&root.join("project/target/rawr/store")), sealed);
        let saved = first_elapsed.saturating_sub(second_elapsed);
        eprintln!(
            "BUILD_CHAIN first_ms={} second_ms={} saved_ms={} first_cargo=5 first_processes={} second_cargo=0 second_processes=0",
            first_elapsed.as_millis(),
            second_elapsed.as_millis(),
            saved.as_millis(),
            first.get("processes").copied().unwrap_or_default(),
        );
        for report in [setup_report, first_report, second_report] {
            fs::remove_file(report).expect("the report retires");
        }
        retire_bundled_fixture(&root, "the build-chain fixture retires");
    }

    #[test]
    #[ignore = "requires the installed pinned toolchain"]
    fn cached_api_graph_warm_process_reuses_with_zero_transform_or_cargo_children() {
        let project = temporary("api-chain");
        let root = project.path().to_path_buf();
        prepare_fixture(&root);
        let setup_report = root.with_extension("api-setup.report");
        let first_report = root.with_extension("api-first.report");
        let second_report = root.with_extension("api-second.report");
        let _ = run_worker(&root, &setup_report, "toolchain");

        let first_elapsed = run_worker(&root, &first_report, "api-chain");
        let first = parse_report(&first_report);
        assert_eq!(first.get("resolution_misses"), Some(&4));
        assert_eq!(first.get("native_misses"), Some(&1));
        assert_eq!(first.get("api_hits"), Some(&0));
        assert_eq!(first.get("api_misses"), Some(&1));
        assert_eq!(first.get("api_published"), Some(&1));
        assert_eq!(first.get("cargo"), Some(&5));
        assert!(first
            .get("processes")
            .is_some_and(|processes| *processes >= 3));
        let sealed = snapshot(&root.join("project/target/rawr/store"));
        fs::remove_dir_all(root.join("project/target")).expect("cold target retires");
        fs::remove_dir_all(root.join("project/protocol")).expect("cold protocol retires");

        let second_elapsed = run_worker(&root, &second_report, "api-chain");
        let second = parse_report(&second_report);
        assert_eq!(second.get("resolution_hits"), Some(&4));
        assert_eq!(second.get("native_hits"), Some(&1));
        assert_eq!(second.get("api_hits"), Some(&1));
        assert_eq!(second.get("resolution_misses"), Some(&0));
        assert_eq!(second.get("native_misses"), Some(&0));
        assert_eq!(second.get("api_misses"), Some(&0));
        assert_eq!(second.get("api_published"), Some(&0));
        assert_eq!(second.get("cargo"), Some(&0));
        assert_eq!(second.get("processes"), Some(&0));
        assert!(root.join("project/target/debug").is_dir());
        assert_eq!(
            fs::read(root.join("project/protocol/sidecar.json"))
                .expect("the warm sidecar restores"),
            b"sidecar\n"
        );
        assert_eq!(snapshot(&root.join("project/target/rawr/store")), sealed);
        let saved = first_elapsed.saturating_sub(second_elapsed);
        eprintln!(
            "API_CHAIN first_ms={} second_ms={} saved_ms={} first_cargo=5 first_processes={} first_api_transforms=1 second_cargo=0 second_processes=0 second_api_transforms=0",
            first_elapsed.as_millis(),
            second_elapsed.as_millis(),
            saved.as_millis(),
            first.get("processes").copied().unwrap_or_default(),
        );
        for report in [setup_report, first_report, second_report] {
            fs::remove_file(report).expect("the API-chain report retires");
        }
        retire_bundled_fixture(&root, "the API-chain fixture retires");
    }

    #[test]
    #[ignore = "requires the installed pinned toolchain"]
    fn cached_facade_warm_process_reuses_api_and_facade_with_zero_children() {
        let project = temporary("facade-chain");
        let root = project.path().to_path_buf();
        prepare_fixture(&root);
        let setup_report = root.with_extension("facade-setup.report");
        let first_report = root.with_extension("facade-first.report");
        let second_report = root.with_extension("facade-second.report");
        let _ = run_worker(&root, &setup_report, "toolchain");

        let first_elapsed = run_worker(&root, &first_report, "facade-chain");
        let first = parse_report(&first_report);
        assert_eq!(first.get("api_misses"), Some(&1));
        assert_eq!(first.get("api_published"), Some(&1));
        assert_eq!(first.get("facade_misses"), Some(&1));
        assert_eq!(first.get("facade_published"), Some(&1));
        assert_eq!(first.get("cargo"), Some(&5));
        let sealed = snapshot(&root.join("project/target/rawr/store"));
        fs::remove_dir_all(root.join("project/target")).expect("cold target retires");
        fs::remove_dir_all(root.join("project/protocol")).expect("cold protocol retires");

        let second_elapsed = run_worker(&root, &second_report, "facade-chain");
        let second = parse_report(&second_report);
        assert_eq!(second.get("resolution_hits"), Some(&4));
        assert_eq!(second.get("native_hits"), Some(&1));
        assert_eq!(second.get("api_hits"), Some(&1));
        assert_eq!(second.get("facade_hits"), Some(&1));
        assert_eq!(second.get("resolution_misses"), Some(&0));
        assert_eq!(second.get("native_misses"), Some(&0));
        assert_eq!(second.get("api_misses"), Some(&0));
        assert_eq!(second.get("facade_misses"), Some(&0));
        assert_eq!(second.get("cargo"), Some(&0));
        assert_eq!(second.get("processes"), Some(&0));
        assert_eq!(snapshot(&root.join("project/target/rawr/store")), sealed);
        let saved = first_elapsed.saturating_sub(second_elapsed);
        eprintln!(
            "FACADE_CHAIN first_ms={} second_ms={} saved_ms={} first_cargo=5 first_processes={} first_api=1 first_facade=1 second_cargo=0 second_processes=0 second_api=0 second_facade=0",
            first_elapsed.as_millis(),
            second_elapsed.as_millis(),
            saved.as_millis(),
            first.get("processes").copied().unwrap_or_default(),
        );
        for report in [setup_report, first_report, second_report] {
            fs::remove_file(report).expect("the facade-chain report retires");
        }
        retire_bundled_fixture(&root, "the facade-chain fixture retires");
    }

    #[test]
    #[ignore = "requires the installed pinned toolchain"]
    fn cached_profile_proof_warm_process_reuses_nodes_with_zero_children() {
        let project = temporary("profile-chain");
        let root = project.path().to_path_buf();
        prepare_fixture(&root);
        let setup_report = root.with_extension("profile-setup.report");
        let first_report = root.with_extension("profile-first.report");
        let second_report = root.with_extension("profile-second.report");
        let _ = run_worker(&root, &setup_report, "toolchain");

        let first_elapsed = run_worker(&root, &first_report, "profile-chain");
        let first = parse_report(&first_report);
        assert_eq!(first.get("api_misses"), Some(&1));
        assert_eq!(first.get("profile_misses"), Some(&1));
        assert_eq!(first.get("profile_published"), Some(&1));
        assert_eq!(first.get("profile_proof_misses"), Some(&2));
        assert_eq!(first.get("profile_proof_published"), Some(&2));
        assert_eq!(first.get("cargo"), Some(&5));
        let sealed = snapshot(&root.join("project/target/rawr/store"));
        fs::remove_dir_all(root.join("project/target")).expect("cold target retires");
        fs::remove_dir_all(root.join("project/protocol")).expect("cold protocol retires");

        let second_elapsed = run_worker(&root, &second_report, "profile-chain");
        let second = parse_report(&second_report);
        assert_eq!(second.get("resolution_hits"), Some(&4));
        assert_eq!(second.get("native_hits"), Some(&1));
        assert_eq!(second.get("api_hits"), Some(&1));
        assert_eq!(second.get("profile_hits"), Some(&1));
        assert_eq!(second.get("profile_misses"), Some(&0));
        assert_eq!(second.get("profile_proof_hits"), Some(&2));
        assert_eq!(second.get("profile_proof_misses"), Some(&0));
        assert_eq!(second.get("cargo"), Some(&0));
        assert_eq!(second.get("processes"), Some(&0));
        assert_eq!(snapshot(&root.join("project/target/rawr/store")), sealed);
        let saved = first_elapsed.saturating_sub(second_elapsed);
        eprintln!(
            "PROFILE_PROOF_CHAIN first_ms={} second_ms={} saved_ms={} first_cargo=5 first_processes={} first_api=1 first_profile=1 first_demand=1 first_generated=1 second_cargo=0 second_processes=0 second_api=0 second_profile=0 second_demand=0 second_generated=0",
            first_elapsed.as_millis(),
            second_elapsed.as_millis(),
            saved.as_millis(),
            first.get("processes").copied().unwrap_or_default(),
        );
        for report in [setup_report, first_report, second_report] {
            fs::remove_file(report).expect("the profile-chain report retires");
        }
        retire_bundled_fixture(&root, "the profile-chain fixture retires");
    }

    #[test]
    #[ignore = "requires the installed pinned toolchain and one private runner socket"]
    fn full_owner_dag_warm_process_reuses_every_transform_and_admits_without_cargo() {
        let project = temporary("full-owner-dag");
        let root = project.path().to_path_buf();
        prepare_fixture(&root);
        let harness = crate::bridge::fixture_runner_harness(&root.join("project"))
            .expect("the full-DAG runner harness materializes");
        let lock = Command::new("cargo")
            .arg("+nightly-2026-07-11")
            .arg("generate-lockfile")
            .arg("--offline")
            .arg("--manifest-path")
            .arg(&harness)
            .status()
            .expect("the full-DAG harness lock command launches");
        assert!(lock.success(), "the full-DAG harness lock seals");

        let setup_report = root.with_extension("full-owner-setup.report");
        let first_report = root.with_extension("full-owner-first.report");
        let second_report = root.with_extension("full-owner-second.report");
        let _ = run_worker(&root, &setup_report, "toolchain");

        let first_elapsed = run_worker(&root, &first_report, "runner-chain");
        let first = parse_report(&first_report);
        for key in [
            "api_misses",
            "generic_misses",
            "facade_misses",
            "checked_misses",
            "profile_misses",
            "overlay_misses",
            "runner_misses",
        ] {
            assert_eq!(first.get(key), Some(&1), "cold `{key}` must execute once");
        }
        assert_eq!(first.get("profile_proof_misses"), Some(&2));
        assert_eq!(first.get("cargo"), Some(&5));
        assert_eq!(first.get("cargo_runner"), Some(&1));
        assert_eq!(first.get("runner_process"), Some(&1));
        assert_eq!(first.get("admitted"), Some(&1));
        let sealed = snapshot(&root.join("project/target/rawr/store"));
        fs::remove_dir_all(root.join("project/target")).expect("the cold target retires");
        fs::remove_dir_all(root.join("project/protocol")).expect("the cold protocol retires");
        fs::remove_dir_all(root.join("project/.rawr/cache/runners"))
            .expect("the cold local runner materialization retires");

        let second_elapsed = run_worker(&root, &second_report, "runner-chain");
        let second = parse_report(&second_report);
        assert_eq!(second.get("resolution_hits"), Some(&4));
        assert_eq!(second.get("native_hits"), Some(&1));
        assert_eq!(second.get("profile_proof_hits"), Some(&2));
        for key in [
            "api_hits",
            "generic_hits",
            "facade_hits",
            "checked_hits",
            "profile_hits",
            "overlay_hits",
            "runner_hits",
        ] {
            assert_eq!(second.get(key), Some(&1), "warm `{key}` must validate once");
        }
        for key in [
            "resolution_misses",
            "native_misses",
            "api_misses",
            "generic_misses",
            "facade_misses",
            "checked_misses",
            "profile_misses",
            "profile_proof_misses",
            "overlay_misses",
            "runner_misses",
            "cargo",
            "cargo_runner",
        ] {
            assert_eq!(second.get(key), Some(&0), "warm `{key}` must stay zero");
        }
        assert_eq!(second.get("runner_process"), Some(&1));
        assert_eq!(second.get("admitted"), Some(&1));
        assert_eq!(second.get("processes"), Some(&1));
        assert_eq!(snapshot(&root.join("project/target/rawr/store")), sealed);

        let saved = first_elapsed.saturating_sub(second_elapsed);
        let reduction = if first_elapsed.is_zero() {
            0.0
        } else {
            saved.as_secs_f64() * 100.0 / first_elapsed.as_secs_f64()
        };
        eprintln!(
            "FULL_OWNER_DAG_READY cold_ms={} warm_ms={} saved_ms={} reduction_percent={reduction:.1} warm_resolution=4 warm_native=1 warm_api=1 warm_generic=1 warm_facade=1 warm_checked=1 warm_profile=1 warm_profile_proof=2 warm_overlay=1 warm_runner=1 warm_cargo=0 warm_cargo_runner=0 warm_owner_process=1 warm_admitted=1",
            first_elapsed.as_millis(),
            second_elapsed.as_millis(),
            saved.as_millis(),
        );
        assert!(
            second_elapsed < first_elapsed,
            "the full warm DAG must be faster"
        );

        for report in [setup_report, first_report, second_report] {
            fs::remove_file(report).expect("the full-DAG report retires");
        }
        retire_bundled_fixture(&root, "the full-owner DAG fixture retires");
    }

    fn execute_leaf_metadata(
        context: &Context,
        toolchain: &node::Validated<Attestation>,
        manifest: &Path,
        target: &Path,
    ) -> crate::Result<Box<[u8]>> {
        execute_leaf_cargo(
            context,
            toolchain,
            target,
            vec!["--offline".into()],
            process::CargoAction::Metadata,
            super::super::cargo_metadata_arguments(1, manifest, context.target(), true),
        )
    }

    fn execute_leaf_unit_graph(
        context: &Context,
        toolchain: &node::Validated<Attestation>,
        manifest: &Path,
        target: &Path,
    ) -> crate::Result<Box<[u8]>> {
        let mut prefix = vec!["--offline".into()];
        prefix.extend(["-Z".into(), "unstable-options".into()]);
        execute_leaf_cargo(
            context,
            toolchain,
            target,
            prefix,
            process::CargoAction::UnitBuild(process::parallelism::Proof::discover()?),
            super::super::cargo_unit_arguments(manifest, context.target(), "dev"),
        )
    }

    fn execute_leaf_build(
        context: &Context,
        toolchain: &node::Validated<Attestation>,
        manifest: &Path,
        target: &Path,
    ) -> crate::Result<Box<[u8]>> {
        execute_leaf_cargo(
            context,
            toolchain,
            target,
            vec!["--offline".into()],
            process::CargoAction::AuthoritativeBuild(process::parallelism::Proof::discover()?),
            super::super::cargo_build_arguments(manifest, context.target(), "dev"),
        )
    }

    fn execute_leaf_cargo(
        context: &Context,
        toolchain: &node::Validated<Attestation>,
        target: &Path,
        prefix: Vec<OsString>,
        action: process::CargoAction,
        tail: Vec<OsString>,
    ) -> crate::Result<Box<[u8]>> {
        let native = process::Native::from_attestation(toolchain);
        let production = native.production();
        let tools = production.cargo_tools();
        let environment = context
            .environment()
            .command_values(target, tools.0, tools.1)?;
        let command = process::Command::cargo(
            prefix,
            action,
            tail,
            context.project().to_path_buf(),
            environment,
        )?;
        production
            .cargo()
            .run(&command, context.limits().process_limits())?
            .checked(process::Program::Cargo)
    }

    fn prepare_fixture(root: &Path) {
        fs::create_dir(root.join("cargo-home")).expect("the fixture Cargo home exists");
        fs::create_dir_all(root.join("project/src")).expect("the fixture project exists");
        fs::write(
            root.join("project/Cargo.toml"),
            b"[package]\nname = \"rawr-resolution-leaf\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[lib]\npath = \"src/lib.rs\"\n",
        )
        .expect("the fixture manifest writes");
        fs::write(
            root.join("project/Cargo.lock"),
            b"# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"rawr-resolution-leaf\"\nversion = \"0.0.0\"\n",
        )
        .expect("the fixture lock writes");
        fs::write(root.join("project/src/lib.rs"), b"pub fn leaf() {}\n")
            .expect("the fixture source writes");
    }

    struct LockedFixture {
        root: PathBuf,
        cargo_home: PathBuf,
        project: PathBuf,
        manifest: PathBuf,
        dependency_manifest: PathBuf,
        lock: PathBuf,
        target: PathBuf,
        _project: crate::store::TestProject,
    }

    fn prepare_locked_fixture(label: &str) -> LockedFixture {
        let guard = temporary(label);
        let root = guard.path().to_path_buf();
        let cargo_home = root.join("cargo-home");
        let project = root.join("project");
        let dependency = root.join("dependency");
        fs::create_dir(&cargo_home).expect("the locked fixture Cargo home exists");
        fs::create_dir_all(project.join("src")).expect("the locked fixture project exists");
        fs::create_dir_all(dependency.join("src")).expect("the locked fixture dependency exists");
        let manifest = project.join("Cargo.toml");
        let dependency_manifest = dependency.join("Cargo.toml");
        let lock = project.join("Cargo.lock");
        fs::write(
            &manifest,
            b"[package]\nname = \"rawr-resolution-root\"\nversion = \"0.0.0\"\n",
        )
        .expect("the root manifest writes");
        fs::write(
            &dependency_manifest,
            b"[package]\nname = \"rawr-resolution-dependency\"\nversion = \"0.0.0\"\n",
        )
        .expect("the dependency manifest writes");
        fs::write(
            &lock,
            b"# This file is automatically @generated by Cargo.\nversion = 4\n",
        )
        .expect("the fixture lock writes");
        fs::write(project.join("src/lib.rs"), b"pub fn root() {}\n")
            .expect("the root source writes");
        fs::write(dependency.join("src/lib.rs"), b"pub fn dependency() {}\n")
            .expect("the dependency source writes");
        let target = project.join("target");
        LockedFixture {
            root,
            cargo_home,
            project,
            manifest,
            dependency_manifest,
            lock,
            target,
            _project: guard,
        }
    }

    fn locked_context(fixture: &LockedFixture) -> Context {
        let environment = super::super::config::Environment::from_pairs([(
            "CARGO_HOME",
            fixture
                .cargo_home
                .to_str()
                .expect("the locked fixture Cargo home is Unicode"),
        )])
        .expect("the locked fixture environment seals");
        Context::capture(
            &fixture.project,
            environment,
            super::super::toolchain::HOST,
            &super::super::Limits::standard(),
        )
        .expect("the locked fixture context seals")
    }

    fn locked_metadata(manifests: &[&Path]) -> Box<[u8]> {
        let packages = manifests
            .iter()
            .map(|manifest| {
                serde_json::json!({
                    "manifest_path": manifest
                        .to_str()
                        .expect("the locked fixture manifest path is Unicode")
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_vec(&serde_json::json!({ "packages": packages }))
            .expect("the locked fixture metadata serializes")
            .into_boxed_slice()
    }

    fn run_worker(root: &Path, report: &Path, mode: &str) -> Duration {
        let started = Instant::now();
        let output = Command::new(std::env::current_exe().expect("the test executable resolves"))
            .arg("--ignored")
            .arg("--exact")
            .arg(WORKER)
            .arg("--nocapture")
            .env(ROOT_ENV, root)
            .env(REPORT_ENV, report)
            .env(MODE_ENV, mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("the fresh resolution worker launches");
        assert!(
            output.status.success(),
            "fresh resolution worker failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        started.elapsed()
    }

    fn parse_report(path: &Path) -> BTreeMap<Box<str>, usize> {
        fs::read_to_string(path)
            .expect("the report exists")
            .lines()
            .map(|line| {
                let (key, value) = line.split_once('=').expect("the report row is key/value");
                (
                    Box::<str>::from(key),
                    value.parse().expect("the report value is numeric"),
                )
            })
            .collect()
    }

    fn retire_bundled_fixture(root: &Path, reason: &str) {
        crate::store::Store::open(&root.join("project/target/rawr/store"), [0_u8; 32])
            .expect("the fixture store opens for retirement")
            .retire_fixture()
            .expect("the immutable fixture store retires");
        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("{reason}: {error}"));
    }

    fn temporary(label: &str) -> crate::store::TestProject {
        crate::store::TestProject::new(&format!("rawr-{label}"))
    }

    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Box<[u8]>> {
        let mut pending = vec![root.to_path_buf()];
        let mut retained = BTreeMap::new();
        while let Some(path) = pending.pop() {
            let metadata = fs::symlink_metadata(&path).expect("the store entry inspects");
            if metadata.is_dir() {
                let mut children = fs::read_dir(&path)
                    .expect("the store directory reads")
                    .map(|entry| entry.expect("the store child reads").path())
                    .collect::<Vec<_>>();
                children.sort_unstable();
                pending.extend(children.into_iter().rev());
            } else if metadata.is_file() {
                retained.insert(
                    path.strip_prefix(root)
                        .expect("the store path remains rooted")
                        .to_path_buf(),
                    fs::read(path)
                        .expect("the store file reads")
                        .into_boxed_slice(),
                );
            }
        }
        retained
    }
}
