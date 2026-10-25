//! Shared-store generated-runner bundle compilation and replay.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::node;
use crate::process::{ProductionReplay as _, ToolPaths as _};
use crate::runner::artifact::retained;

const DOMAIN: &[u8] = b"rawr.runner.bundle-request/v2";
const MAGIC: &[u8; 8] = b"RAWRRUN3";
const BUNDLE_MAGIC: &[u8; 8] = b"RAWRBNDL";
const SCHEMA: u32 = 3;
const BUNDLE_SCHEMA: u32 = 2;
const COMMAND_SCHEMA: u32 = 4;
const PRODUCT_BYTES: usize = 288;
const BUNDLE_HEADER_BYTES: usize = 296;

#[cfg(all(test, feature = "test-fixtures"))]
mod hot;

#[cfg(all(test, feature = "test-fixtures"))]
pub(crate) use hot::fixture_build;

pub(super) fn build<B>(
    mut manifest: super::super::Manifest<super::super::Generated, B>,
    cargo_owner: Option<&crate::cargo::Owner>,
    _cache: &crate::cache::Writer,
) -> Result<super::super::Manifest<super::super::Built, B>, super::Error> {
    let chain = manifest
        .chain
        .as_ref()
        .or_else(|| manifest.profile.node_chain())
        .cloned()
        .ok_or(node::Error::State(
            "cached runner build lost its generated-profile parent",
        ))?;
    let context = chain.context().clone();
    let toolchain = crate::cargo::toolchain::cached(&context, &crate::metrics::Current)?;
    let parallelism = crate::process::parallelism::Proof::discover()?;
    let inputs = Inputs::capture(&manifest)?;
    let authority = authority(&inputs, toolchain.as_ref(), parallelism)?;
    let parents = [chain.parent(), node::Parent::validated(toolchain.as_ref())];
    let recipe = context.discover_with_parents::<Product>(&authority, PRODUCT_BYTES, &parents)?;
    let validation = inputs.clone();
    let limits = context.limits();
    let store = context.open_store()?;
    let validation_store = store.clone();
    let native = crate::process::Native::from_attestation(toolchain.as_ref());
    let retention = chain
        .retention()
        .map(|parent| {
            parent.union(&node::read_set::Retention::from_validated(
                toolchain.as_ref(),
                None,
            )?)
        })
        .transpose()?;
    let validate = move |candidate: &Product, budget: &node::GraphBudget| {
        candidate.revalidate(&validation_store, &validation, budget)
    };
    let host = toolchain.value().proof().host();
    let execute = |attempt| {
        compile(
            &manifest,
            &inputs,
            &context,
            &native,
            host,
            parallelism,
            cargo_owner,
            attempt,
            store.graph_budget(),
            || {
                if retention.is_some() {
                    recipe.revalidate_parents(&store)
                } else {
                    Ok(())
                }
            },
        )
    };
    let evaluator = node::Evaluator::new(&store, &crate::metrics::Current);
    let product = match &retention {
        Some(retention) => {
            evaluator.discover_bundled_retained_budgeted(&recipe, retention, validate, execute)
        }
        None => evaluator.discover_bundled_budgeted(&recipe, validate, execute),
    }?;
    let pin = crate::store::bundle::pin_budgeted(
        store.root(),
        product.value().reference,
        store.graph_budget(),
    )?;
    let native = product
        .value()
        .native(&store, &inputs, &manifest.generated, &limits, pin)?;
    let runner_chain = retain_runner(&chain, &product)?;
    let read_set = prepare_union(&runner_chain, manifest.overlay.as_ref())?;
    manifest.chain = Some(runner_chain);
    Ok(manifest.built(native, Some(read_set)))
}

pub(super) fn replay_hot<B>(
    manifest: &mut super::super::Manifest<super::super::Generated, B>,
    live: &super::super::Manifest<super::super::Built, B>,
) -> Result<node::read_set::Prepared, node::Error> {
    let chain = manifest
        .profile
        .node_chain()
        .cloned()
        .ok_or(node::Error::State(
            "hot runner request has no current shared profile evidence",
        ))?;
    let overlay = manifest.overlay.as_ref().ok_or(node::Error::State(
        "hot runner request has no owned overlay evidence",
    ))?;
    let previous = live.chain.as_ref().ok_or(node::Error::State(
        "live runner has no exact shared action evidence",
    ))?;
    let context = chain.context();
    if context.identity() != previous.context().identity()
        || context.identity() != overlay.context().identity()
        || previous.parent().kind() != node::Kind::Runner
    {
        return Err(node::Error::State(
            "hot runner context or action kind changed",
        ));
    }
    let toolchain = context.cached_toolchain().ok_or(node::Error::State(
        "hot runner request has no current attested toolchain",
    ))?;
    let inputs = Inputs::capture(manifest)?;
    let parallelism = crate::process::parallelism::Proof::discover()
        .map_err(|error| node::Error::Importer(Box::new(error)))?;
    let authority = authority(&inputs, &toolchain, parallelism)?;
    let parents = [chain.parent(), node::Parent::validated(toolchain.as_ref())];
    let recipe = context.discover_with_parents::<Product>(&authority, PRODUCT_BYTES, &parents)?;
    let store = context.open_store()?;
    let validation_store = store.clone();
    let validate: node::read_set::Validator<Product> = std::sync::Arc::new(move |value, budget| {
        value.revalidate(&validation_store, &inputs, budget)
    });
    // Only the live action is admissible. This API has no discovery, execution,
    // Cargo-owner acquisition, or transform callback to fall back to.
    let action = previous.parent().action();
    let replay = node::Evaluator::new(&store, &crate::metrics::Current)
        .replay_discovered_exact(&recipe, &validate, action)
        .and_then(|value| {
            value
                .filter(|product| product.value().native == live.native_identity())
                .ok_or(node::Error::StaleReadSet { action })
        });
    crate::metrics::Sink::node(
        &crate::metrics::Current,
        crate::metrics::NodeKind::Runner,
        if replay.is_ok() {
            crate::metrics::NodeEvent::ValidatedHit
        } else {
            crate::metrics::NodeEvent::RejectedHit
        },
    );
    let product = replay?;
    let runner = retain_runner(&chain, &product)?;
    let prepared = prepare_union(&runner, Some(overlay))?;
    manifest.chain = Some(runner);
    Ok(prepared)
}

fn retain_runner(
    parent: &node::Chain,
    product: &node::Validated<Product>,
) -> Result<node::Chain, node::Error> {
    let Some(initial) = parent.retention() else {
        return Ok(parent.child(product));
    };
    let retained = initial.extend_bundled_current(product, product.value().bundle_commitment())?;
    node::Chain::validated_retained(parent.context().clone(), product, retained)
}

fn prepare_union(
    runner: &node::Chain,
    overlay: Option<&super::super::overlay::Overlay>,
) -> Result<node::read_set::Prepared, node::Error> {
    super::super::overlay::prepare_union(runner, overlay)
}

fn authority(
    inputs: &Inputs,
    toolchain: &node::Validated<crate::cargo::toolchain::Attestation>,
    parallelism: crate::process::parallelism::Proof,
) -> Result<Box<[u8]>, node::Error> {
    let retained = DOMAIN
        .len()
        .checked_add(32 * 3)
        .and_then(|length| length.checked_add(12))
        .ok_or(node::Error::Allocation)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(retained)
        .map_err(|_| node::Error::Allocation)?;
    bytes.extend_from_slice(DOMAIN);
    bytes.extend_from_slice(inputs.build.as_bytes());
    bytes.extend_from_slice(&inputs.manifest);
    bytes.extend_from_slice(&toolchain.content().digest().as_bytes());
    bytes.extend_from_slice(&COMMAND_SCHEMA.to_le_bytes());
    bytes.extend_from_slice(&parallelism.schema().to_le_bytes());
    bytes.extend_from_slice(&(parallelism.jobs().get() as u32).to_le_bytes());
    Ok(bytes.into_boxed_slice())
}

#[allow(clippy::too_many_arguments)]
fn compile<B>(
    manifest: &super::super::Manifest<super::super::Generated, B>,
    inputs: &Inputs,
    context: &crate::context::Context,
    native: &crate::process::Native,
    host: &str,
    parallelism: crate::process::parallelism::Proof,
    cargo_owner: Option<&crate::cargo::Owner>,
    attempt: crate::store::bundle::Attempt,
    budget: &node::GraphBudget,
    before_cargo: impl FnOnce() -> Result<(), node::Error>,
) -> Result<node::Bundled<Product>, node::Error> {
    let generated = attempt.generated();
    materialize(manifest, &generated)?;
    let target = attempt.target();
    let production = native.production();
    let (rustc, rustdoc) = production.cargo_tools();
    let project_build = crate::location::build_directory(context.project(), context.environment());
    let build = crate::deps::directory(&project_build).map_err(|source| node::Error::Io {
        operation: "create the project's Cargo build directory",
        path: project_build.clone(),
        source,
    })?;
    let work = work_directory(context.rawr());
    // Cargo builds the project's one runner in place; the sealed bundle
    // receives a copy of the executable it produced.
    let built = work.join("target");
    // Rustc reuses the unchanged adapter modules from its incremental
    // state for this one stable-path unit. The generated manifest makes only
    // that unit incremental, so `CARGO_INCREMENTAL` must not override it, and
    // every dependency is the non-incremental unit the harness build already
    // compiled for the same target.
    let environment = context
        .environment()
        .command_values(&built, rustc, rustdoc)?
        .into_iter()
        .filter(|(key, _)| key != "CARGO_INCREMENTAL")
        .collect::<Box<[_]>>();
    let build_value = toml_edit::Value::from(build.to_string_lossy().as_ref()).to_string();
    let prefix = vec![
        OsString::from("--config"),
        OsString::from(format!("build.build-dir={build_value}")),
    ];
    let tail = vec![
        // Artifacts as JSON name every unit this build used; rustc's
        // diagnostics still render on stderr.
        OsString::from("--message-format=json-render-diagnostics"),
        OsString::from("--offline"),
        OsString::from("--locked"),
        OsString::from("--target"),
        OsString::from(host),
        OsString::from("--manifest-path"),
        work.join("Cargo.toml").into_os_string(),
    ];
    let command = crate::process::Command::cargo(
        prefix,
        crate::process::CargoAction::RunnerBuild(parallelism),
        tail,
        work.clone(),
        environment,
    )?;
    let acquired = if cargo_owner.is_none() {
        Some(crate::cargo::Owner::acquire()?)
    } else {
        None
    };
    before_cargo()?;
    let shared = crate::deps::lock(context.rawr()).map_err(|source| node::Error::Io {
        operation: "lock shared runner build directory",
        path: context.rawr().join("build.lock"),
        source,
    })?;
    let carrier = manifest
        .generated
        .carrier()
        .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
    crate::bridge::generate::sync_sources(
        &work,
        manifest
            .files
            .iter()
            .map(|file| (file.path(), file.bytes()))
            .chain([("rawr-generated-manifest.carrier", carrier.as_ref())]),
    )
    .map_err(|source| node::Error::Io {
        operation: "update the stable generated runner sources",
        path: work.clone(),
        source,
    })?;
    let started = std::time::SystemTime::now();
    let result = production
        .cargo()
        .run(&command, context.limits().process_limits())
        .and_then(|output| output.checked(crate::process::Program::Cargo));
    let name = if cfg!(windows) {
        "rawr-generated-runner.exe"
    } else {
        "rawr-generated-runner"
    };
    let copied = result.as_ref().ok().map(|messages| {
        // The runner's dep-info names every source this build compiled.
        let output = built.join(host).join("debug");
        if let Ok(used) = fs::read(output.join("rawr-generated-runner.d")) {
            crate::deps::prune_rawr(&build, &used);
        }
        crate::deps::prune_superseded(&build, messages, started);
        // Copy (a clone on APFS) under the lock, before another build of this
        // project replaces the executable.
        let destination = target.join("debug");
        fs::create_dir_all(&destination)
            .and_then(|()| fs::copy(output.join(name), destination.join(name)))
            .map(|_| first_exec(&destination.join(name)))
            .map_err(|source| node::Error::Io {
                operation: "copy the built runner into its bundle",
                path: destination.join(name),
                source,
            })
    });
    drop((shared, acquired));
    result?;
    copied.transpose()?;
    let executable = crate::store::bundle::path(
        &target,
        Path::new(if cfg!(windows) {
            "debug/rawr-generated-runner.exe"
        } else {
            "debug/rawr-generated-runner"
        }),
        budget,
    )?;
    let artifact = super::super::artifact::BuiltArtifactProof::seal_budgeted(
        &manifest.generated,
        &executable,
        &target,
        &[],
        &context.limits(),
        budget,
    )
    .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
    // The executable read is a physical capture-class I/O bound; the node
    // product bound stays MAX_CONTENT_BYTES.
    let executable = Evidence::capture(&executable, crate::process::CAPTURE_BYTES, budget)?;
    let carrier = Evidence::capture(
        &crate::store::bundle::path(
            &generated,
            Path::new("rawr-generated-manifest.carrier"),
            budget,
        )?,
        context.limits().node_output_bytes(),
        budget,
    )?;
    let reference = attempt.reference();
    let retained = retained::Stored::capture(
        &attempt.root_budgeted(budget)?,
        reference,
        retained::Mode::Pending,
        budget,
    )?;
    let seal = bundle_seal(
        reference,
        manifest.generated.digest(),
        &carrier,
        &executable,
        artifact.digest(),
        &retained,
        budget,
    )?;
    let product = Product::capture(
        reference,
        frozen_mode(executable.mode),
        executable.bytes,
        executable.digest,
        &seal,
        manifest.generated.digest(),
        artifact.digest(),
        &retained,
        budget,
    )?;
    let pending = attempt.seal_budgeted(seal, budget)?;
    node::Bundled::validate(product, pending, |product, pending| {
        product.revalidate_pending(pending, inputs, budget)
    })
}

fn materialize<B>(
    manifest: &super::super::Manifest<super::super::Generated, B>,
    generated: &Path,
) -> Result<(), node::Error> {
    for file in &manifest.files {
        let path = joined(generated, file.path())?;
        let parent = path
            .parent()
            .ok_or_else(|| node::Error::Shape(path.clone()))?;
        create_directories(generated, parent)?;
        write_new(&path, file.bytes())?;
    }
    let carrier = manifest
        .generated
        .carrier()
        .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
    write_new(&generated.join("rawr-generated-manifest.carrier"), &carrier)
}

/// The directory Cargo builds this project's runner from,
/// `<target>/rawr/runner`. Its path stays the same across edits, so Cargo keeps
/// one unit and rustc its incremental state for it. The sealed bundle keeps
/// its own copy of every file.
fn work_directory(rawr: &Path) -> PathBuf {
    rawr.join("runner")
}

#[derive(Clone)]
struct Inputs {
    build: crate::bridge::BuildRequestKey,
    manifest: [u8; 32],
    carrier: Box<[u8]>,
    files: Box<[InputFile]>,
}

#[derive(Clone)]
struct InputFile {
    path: Box<str>,
    bytes: usize,
    digest: [u8; 32],
}

impl Inputs {
    fn capture<B>(
        manifest: &super::super::Manifest<super::super::Generated, B>,
    ) -> Result<Self, node::Error> {
        let mut files = Vec::new();
        files
            .try_reserve_exact(manifest.files.len())
            .map_err(|_| node::Error::Allocation)?;
        for file in &manifest.files {
            files.push(InputFile {
                path: file.path().into(),
                bytes: file.bytes().len(),
                digest: *file.digest(),
            });
        }
        Ok(Self {
            build: manifest.generated.build_request_key(),
            manifest: manifest.generated.digest(),
            carrier: manifest
                .generated
                .carrier()
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?,
            files: files.into_boxed_slice(),
        })
    }

    fn validate(&self, root: &Path, budget: &node::GraphBudget) -> Result<(), node::Error> {
        let generated = crate::store::bundle::path(root, Path::new("generated"), budget)?;
        for file in &self.files {
            let path = crate::store::bundle::path(&generated, Path::new(&*file.path), budget)?;
            validate_file(&path, file.bytes, file.digest, budget)?;
        }
        budget.hash(self.carrier.len())?;
        validate_file(
            &crate::store::bundle::path(
                &generated,
                Path::new("rawr-generated-manifest.carrier"),
                budget,
            )?,
            self.carrier.len(),
            Sha256::digest(&self.carrier).into(),
            budget,
        )?;
        Ok(())
    }
}

struct Product {
    canonical: Box<[u8]>,
    reference: crate::store::bundle::BundleRef,
    mode: u32,
    executable_bytes: u64,
    executable_digest: [u8; 32],
    seal_bytes: usize,
    seal_digest: [u8; 32],
    manifest: [u8; 32],
    artifact: [u8; 32],
    retained: [u8; 32],
    native: [u8; 32],
}

impl Product {
    pub(crate) fn bundle_commitment(&self) -> crate::store::bundle::BundleCommitment {
        crate::store::bundle::BundleCommitment::new(
            self.reference,
            self.seal_bytes,
            self.seal_digest,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn capture(
        reference: crate::store::bundle::BundleRef,
        mode: u32,
        executable_bytes: u64,
        executable_digest: [u8; 32],
        seal: &[u8],
        manifest: [u8; 32],
        artifact: [u8; 32],
        retained: &retained::Stored,
        budget: &node::GraphBudget,
    ) -> Result<Self, node::Error> {
        budget.decode(PRODUCT_BYTES)?;
        budget.hash(seal.len())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(PRODUCT_BYTES)
            .map_err(|_| node::Error::Allocation)?;
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&SCHEMA.to_le_bytes());
        bytes.extend_from_slice(&reference.execution().as_bytes());
        bytes.extend_from_slice(&reference.id().as_bytes());
        bytes.extend_from_slice(&mode.to_le_bytes());
        bytes.extend_from_slice(&executable_bytes.to_le_bytes());
        bytes.extend_from_slice(&executable_digest);
        bytes.extend_from_slice(&(seal.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&<[u8; 32]>::from(Sha256::digest(seal)));
        bytes.extend_from_slice(&manifest);
        bytes.extend_from_slice(&artifact);
        bytes.extend_from_slice(&retained.digest());
        bytes.extend_from_slice(&retained.native_identity(artifact));
        debug_assert_eq!(bytes.len(), PRODUCT_BYTES);
        Self::decode_canonical(bytes.into_boxed_slice())
    }

    fn decode_canonical(canonical: Box<[u8]>) -> Result<Self, node::Error> {
        validate(&canonical, PRODUCT_BYTES)?;
        let reference = crate::store::bundle::BundleRef::new(
            node::ExecutionKey::from_bytes(array_at(&canonical, 12)?),
            crate::store::bundle::BundleId::from_bytes(array_at(&canonical, 44)?),
        );
        Ok(Self {
            mode: u32_at(&canonical, 76)?,
            executable_bytes: u64_at(&canonical, 80)?,
            executable_digest: array_at(&canonical, 88)?,
            seal_bytes: usize::try_from(u64_at(&canonical, 120)?).unwrap_or(usize::MAX),
            seal_digest: array_at(&canonical, 128)?,
            manifest: array_at(&canonical, 160)?,
            artifact: array_at(&canonical, 192)?,
            retained: array_at(&canonical, 224)?,
            native: array_at(&canonical, 256)?,
            canonical,
            reference,
        })
    }

    fn revalidate(
        &self,
        store: &crate::store::Store,
        inputs: &Inputs,
        budget: &node::GraphBudget,
    ) -> Result<Box<[u8]>, node::Error> {
        if self.manifest != inputs.manifest {
            return Err(node::Error::Codec(
                "runner bundle generated manifest changed",
            ));
        }
        let key = node::content_digest::<Self>(&self.canonical);
        let root = crate::store::bundle::validate_complete(
            store.root(),
            self.reference,
            node::Kind::Runner,
            SCHEMA,
            key,
            self.seal_bytes,
            self.seal_digest,
            budget,
        )?;
        self.revalidate_root(&root, inputs, retained::Mode::Frozen, budget)?;
        budget.decode(self.canonical.len())?;
        Ok(self.canonical.clone())
    }

    fn revalidate_pending(
        &self,
        pending: &crate::store::bundle::Pending,
        inputs: &Inputs,
        budget: &node::GraphBudget,
    ) -> Result<Box<[u8]>, node::Error> {
        let root = pending.validate_unpublished_budgeted(
            self.reference,
            self.seal_bytes,
            self.seal_digest,
            budget,
        )?;
        self.revalidate_root(&root, inputs, retained::Mode::Pending, budget)?;
        budget.decode(self.canonical.len())?;
        Ok(self.canonical.clone())
    }

    fn revalidate_root(
        &self,
        root: &Path,
        inputs: &Inputs,
        mode: retained::Mode,
        budget: &node::GraphBudget,
    ) -> Result<retained::Stored, node::Error> {
        inputs.validate(root, budget)?;
        let retained = self.inventory(root, inputs, budget)?;
        retained.replay(root, mode, budget)?;
        let executable_path = crate::store::bundle::path(
            root,
            Path::new(if cfg!(windows) {
                "target/debug/rawr-generated-runner.exe"
            } else {
                "target/debug/rawr-generated-runner"
            }),
            budget,
        )?;
        // The executable read is a physical capture-class I/O bound; the node
        // product bound stays MAX_CONTENT_BYTES.
        let executable =
            Evidence::capture(&executable_path, crate::process::CAPTURE_BYTES, budget)?;
        if frozen_mode(executable.mode) != self.mode
            || executable.bytes != self.executable_bytes
            || executable.digest != self.executable_digest
        {
            return Err(node::Error::Corrupt {
                path: executable_path,
                reason: "runner bundle executable changed",
            });
        }
        Ok(retained)
    }

    fn inventory(
        &self,
        root: &Path,
        inputs: &Inputs,
        budget: &node::GraphBudget,
    ) -> Result<retained::Stored, node::Error> {
        let seal_path = crate::store::bundle::path(root, Path::new("seal"), budget)?;
        let seal = crate::store::read_file_budgeted(&seal_path, node::MAX_CONTENT_BYTES, budget)?;
        budget.hash(seal.len())?;
        budget.hash(inputs.carrier.len())?;
        if seal.len() != self.seal_bytes
            || <[u8; 32]>::from(Sha256::digest(&seal)) != self.seal_digest
            || seal.get(..8) != Some(BUNDLE_MAGIC)
            || u32_at(&seal, 8)? != BUNDLE_SCHEMA
            || array_at(&seal, 12)? != self.reference.execution().as_bytes()
            || array_at(&seal, 44)? != self.reference.id().as_bytes()
            || array_at(&seal, 76)? != self.manifest
            || self.manifest != inputs.manifest
            || u64_at(&seal, 108)? != inputs.carrier.len() as u64
            || array_at(&seal, 116)? != <[u8; 32]>::from(Sha256::digest(&inputs.carrier))
            || u64_at(&seal, 148)? != self.executable_bytes
            || array_at(&seal, 156)? != self.executable_digest
            || u32_at(&seal, 188)? != self.mode
            || array_at(&seal, 192)? != self.artifact
            || array_at(&seal, 224)? != self.retained
            || array_at(&seal, 256)? != self.native
            || u64_at(&seal, 288)? != seal.len().saturating_sub(BUNDLE_HEADER_BYTES) as u64
        {
            return Err(node::Error::Codec("runner complete bundle seal changed"));
        }
        let stored = retained::Stored::decode(
            seal.get(BUNDLE_HEADER_BYTES..)
                .ok_or(node::Error::Codec("runner inventory missing"))?,
            root,
            self.reference,
            budget,
        )?;
        if stored.digest() != self.retained || stored.native_identity(self.artifact) != self.native
        {
            return Err(node::Error::Codec("runner native identity changed"));
        }
        Ok(stored)
    }

    fn native(
        &self,
        store: &crate::store::Store,
        inputs: &Inputs,
        manifest: &super::super::manifest::GeneratedManifest,
        limits: &crate::cargo::Limits,
        pin: crate::store::bundle::Guard,
    ) -> Result<super::super::Native, super::Error> {
        self.revalidate(store, inputs, store.graph_budget())?;
        let root = crate::store::bundle::path(
            store.root(),
            &self.reference.relative_budgeted(store.graph_budget())?,
            store.graph_budget(),
        )?;
        let target = crate::store::bundle::path(&root, Path::new("target"), store.graph_budget())?;
        let executable = crate::store::bundle::path(
            &target,
            Path::new(if cfg!(windows) {
                "debug/rawr-generated-runner.exe"
            } else {
                "debug/rawr-generated-runner"
            }),
            store.graph_budget(),
        )?;
        let artifact = super::super::artifact::BuiltArtifactProof::seal_budgeted(
            manifest,
            &executable,
            &target,
            &[],
            limits,
            store.graph_budget(),
        )?;
        if artifact.digest() != self.artifact {
            return Err(node::Error::Corrupt {
                path: executable,
                reason: "runner bundle artifact proof changed",
            }
            .into());
        }
        let stored = self.inventory(&root, inputs, store.graph_budget())?;
        store
            .graph_budget()
            .decode(store.root().as_os_str().as_encoded_bytes().len())?;
        let retained = retained::Proof::validate(
            stored,
            store.clone(),
            retained::Control {
                schema: SCHEMA,
                key: node::content_digest::<Self>(&self.canonical),
                seal_bytes: self.seal_bytes,
                seal_digest: self.seal_digest,
            },
        )?;
        Ok(super::super::Native::Shared {
            artifact,
            retained,
            _pin: pin,
        })
    }
}

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::Runner;
    const SCHEMA: u32 = SCHEMA;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        validate(&self.canonical, limit)?;
        Ok(self.canonical.clone())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        validate(bytes, limit)?;
        Self::decode_canonical(bytes.to_vec().into_boxed_slice())
    }
}

struct Evidence {
    bytes: u64,
    digest: [u8; 32],
    mode: u32,
}

impl Evidence {
    fn capture(path: &Path, limit: usize, budget: &node::GraphBudget) -> Result<Self, node::Error> {
        let parent = path
            .parent()
            .ok_or_else(|| node::Error::Shape(path.to_path_buf()))?;
        if retained::directory(parent, budget)? != parent {
            return Err(node::Error::OutsideRoot(path.to_path_buf()));
        }
        budget.paths(1)?;
        crate::store::path::input(path, budget)?;
        let metadata = fs::symlink_metadata(path).map_err(|source| node::Error::Io {
            operation: "inspect runner bundle file",
            path: path.to_path_buf(),
            source,
        })?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || crate::store::bundle::hard_linked(&metadata)
        {
            return Err(node::Error::Shape(path.to_path_buf()));
        }
        let digest = retained::hash(path, &metadata, limit, budget)?;
        Ok(Self {
            bytes: metadata.len(),
            digest,
            mode: mode(&metadata),
        })
    }
}

fn bundle_seal(
    reference: crate::store::bundle::BundleRef,
    manifest: [u8; 32],
    carrier: &Evidence,
    executable: &Evidence,
    artifact: [u8; 32],
    retained: &retained::Stored,
    budget: &node::GraphBudget,
) -> Result<Box<[u8]>, node::Error> {
    let length = BUNDLE_HEADER_BYTES
        .checked_add(retained.bytes().len())
        .ok_or(node::Error::Allocation)?;
    if length > node::MAX_CONTENT_BYTES {
        return Err(node::Error::Limit {
            resource: "runner bundle seal bytes",
            limit: node::MAX_CONTENT_BYTES,
            actual: length,
        });
    }
    budget.decode(length)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| node::Error::Allocation)?;
    bytes.extend_from_slice(BUNDLE_MAGIC);
    bytes.extend_from_slice(&BUNDLE_SCHEMA.to_le_bytes());
    bytes.extend_from_slice(&reference.execution().as_bytes());
    bytes.extend_from_slice(&reference.id().as_bytes());
    bytes.extend_from_slice(&manifest);
    bytes.extend_from_slice(&carrier.bytes.to_le_bytes());
    bytes.extend_from_slice(&carrier.digest);
    bytes.extend_from_slice(&executable.bytes.to_le_bytes());
    bytes.extend_from_slice(&executable.digest);
    bytes.extend_from_slice(&frozen_mode(executable.mode).to_le_bytes());
    bytes.extend_from_slice(&artifact);
    bytes.extend_from_slice(&retained.digest());
    bytes.extend_from_slice(&retained.native_identity(artifact));
    bytes.extend_from_slice(&(retained.bytes().len() as u64).to_le_bytes());
    debug_assert_eq!(bytes.len(), BUNDLE_HEADER_BYTES);
    bytes.extend_from_slice(retained.bytes());
    Ok(bytes.into_boxed_slice())
}

fn validate(bytes: &[u8], limit: usize) -> Result<(), node::Error> {
    if bytes.len() > limit || bytes.len() != PRODUCT_BYTES {
        return Err(node::Error::Limit {
            resource: "cached runner bundle product bytes",
            limit: PRODUCT_BYTES.min(limit),
            actual: bytes.len(),
        });
    }
    if bytes.get(..8) != Some(MAGIC) || u32_at(bytes, 8)? != SCHEMA {
        return Err(node::Error::Codec(
            "cached runner bundle product schema changed",
        ));
    }
    validate_mode(u32_at(bytes, 76)?)?;
    if !(BUNDLE_HEADER_BYTES + 1..=node::MAX_CONTENT_BYTES)
        .contains(&usize::try_from(u64_at(bytes, 120)?).unwrap_or(usize::MAX))
    {
        return Err(node::Error::Codec("runner bundle seal length changed"));
    }
    Ok(())
}

fn validate_file(
    path: &Path,
    expected: usize,
    digest: [u8; 32],
    budget: &node::GraphBudget,
) -> Result<(), node::Error> {
    let evidence = Evidence::capture(path, expected, budget)?;
    if usize::try_from(evidence.bytes).unwrap_or(usize::MAX) != expected
        || evidence.digest != digest
    {
        return Err(node::Error::Corrupt {
            path: path.to_path_buf(),
            reason: "runner bundle generated input changed",
        });
    }
    Ok(())
}

fn joined(root: &Path, relative: &str) -> Result<PathBuf, node::Error> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(node::Error::OutsideRoot(relative.to_path_buf()));
    }
    Ok(root.join(relative))
}

fn create_directories(root: &Path, target: &Path) -> Result<(), node::Error> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| node::Error::OutsideRoot(target.to_path_buf()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(node::Error::OutsideRoot(target.to_path_buf()));
        };
        current.push(component);
        match fs::create_dir(&current) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata =
                    fs::symlink_metadata(&current).map_err(|source| node::Error::Io {
                        operation: "inspect generated runner directory",
                        path: current.clone(),
                        source,
                    })?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(node::Error::Shape(current));
                }
            }
            Err(source) => {
                return Err(node::Error::Io {
                    operation: "create generated runner directory",
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(())
}

/// macOS assesses a newly written executable on its first exec, about 0.9 s
/// for Gallery's runner; later execs of the same file start in a few
/// milliseconds. Exec the bundle's copy once now, without the protocol
/// environment so it exits at once, and that cost overlaps hashing, sealing
/// and publication instead of delaying the swap.
fn first_exec(executable: &Path) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let child = std::process::Command::new(executable)
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        std::thread::spawn(move || drop(child.wait()));
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), node::Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| node::Error::Io {
            operation: "create generated runner file",
            path: path.to_path_buf(),
            source,
        })?;
    // Not synced: the bundle is a recomputable cache whose
    // members are validated before use, and its freeze syncs the root.
    file.write_all(bytes).map_err(|source| node::Error::Io {
        operation: "write generated runner file",
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(all(test, feature = "test-fixtures"))]
fn executable(target: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "rawr-generated-runner.exe"
    } else {
        "rawr-generated-runner"
    };
    target.join("debug").join(name)
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, node::Error> {
    bytes
        .get(offset..offset.saturating_add(4))
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(node::Error::Codec("runner bundle u32 is truncated"))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, node::Error> {
    bytes
        .get(offset..offset.saturating_add(8))
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(node::Error::Codec("runner bundle u64 is truncated"))
}

fn array_at(bytes: &[u8], offset: usize) -> Result<[u8; 32], node::Error> {
    bytes
        .get(offset..offset.saturating_add(32))
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(node::Error::Codec("runner bundle digest is truncated"))
}

#[cfg(unix)]
fn mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode() & 0o777
}

#[cfg(not(unix))]
fn mode(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(unix)]
const fn frozen_mode(mode: u32) -> u32 {
    if mode & 0o111 != 0 {
        0o555
    } else {
        0o444
    }
}

#[cfg(not(unix))]
const fn frozen_mode(_mode: u32) -> u32 {
    1
}

#[cfg(unix)]
fn validate_mode(mode: u32) -> Result<(), node::Error> {
    if matches!(mode, 0o444 | 0o555) {
        Ok(())
    } else {
        Err(node::Error::Codec("runner bundle mode is invalid"))
    }
}

#[cfg(not(unix))]
fn validate_mode(mode: u32) -> Result<(), node::Error> {
    if mode == 1 {
        Ok(())
    } else {
        Err(node::Error::Codec("runner bundle mode is invalid"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Value as _;

    #[test]
    fn runner_product_requires_complete_schema_and_bounded_inventory() {
        let mut bytes = vec![0; PRODUCT_BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..12].copy_from_slice(&SCHEMA.to_le_bytes());
        bytes[76..80].copy_from_slice(&frozen_mode(0o755).to_le_bytes());
        bytes[120..128].copy_from_slice(&((BUNDLE_HEADER_BYTES + 1000) as u64).to_le_bytes());
        let product = Product::decode(&bytes, PRODUCT_BYTES).unwrap();
        assert_eq!(product.encode(PRODUCT_BYTES).unwrap().as_ref(), bytes);
        let mut old = bytes.clone();
        old[8..12].copy_from_slice(&2_u32.to_le_bytes());
        assert!(Product::decode(&old, PRODUCT_BYTES).is_err());
        assert!(Product::decode(&bytes[..224], PRODUCT_BYTES).is_err());
        bytes[120..128].copy_from_slice(&228_u64.to_le_bytes());
        assert!(Product::decode(&bytes, PRODUCT_BYTES).is_err());
        bytes[120..128].copy_from_slice(&((node::MAX_CONTENT_BYTES + 1) as u64).to_le_bytes());
        assert!(Product::decode(&bytes, PRODUCT_BYTES).is_err());
    }
}
