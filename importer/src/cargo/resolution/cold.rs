//! Direct Cargo commands inside an already-owned, unsealed resolution Layout.

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use super::layout;
use crate::cargo::{self, Limits, Request};
use crate::process::{ProductionReplay, Program, ToolPaths};
use crate::{Error, Result};

impl Request {
    /// Reparse the paired retained documents under the caller's current accounts
    /// and supplied lock/context association. This produces pending observations;
    /// input replay and protected completion remain separate transformations.
    #[allow(
        clippy::too_many_arguments,
        clippy::type_complexity,
        reason = "paired document interpretation preserves the existing owner associations and accounts"
    )]
    pub(in crate::cargo) fn bind_retained_documents_in(
        &self,
        layout: &layout::Paths,
        documents: cargo::DocumentBytes<'_>,
        definition: &cargo::universe::Definition,
        lock: cargo::universe::Candidate,
        request: cargo::Digest,
        context: cargo::universe::ContextProof,
        receipt: Option<cargo::universe::Receipt>,
        cargo_limits: &Limits,
        cargo_allocation: &mut cargo::unit::Allocation,
    ) -> Result<(
        cargo::unit::Metadata,
        Box<[cargo::Selection]>,
        cargo::internal::Recipe,
        cargo::unit::BuildGraph,
        cargo::universe::Pending,
    )> {
        cargo_allocation.require(cargo_limits);
        let paths = layout.final_form();
        let (metadata, observation) = cargo::unit::Metadata::parse_versioned_in(
            documents.metadata,
            &self.dependencies,
            cargo_limits,
            self.toolchain.schemas().graph(),
            paths,
            cargo_allocation,
            definition,
        )?;
        let (metadata, selected, internal) = self.bind_fresh_metadata(
            metadata,
            None,
            &lock,
            (paths.manifest(), paths.source()),
            cargo_limits,
            cargo_allocation,
        )?;
        let (initial, pending) = self.bind_versioned_units(
            documents.units,
            &metadata,
            cargo_limits,
            cargo_allocation,
            definition,
            observation,
            lock,
            request,
            context,
            receipt,
        )?;
        Ok((metadata, selected, internal, initial, pending))
    }

    /// Complete the checked Build in its reserved Layout without acquiring another owner.
    #[cfg(test)]
    pub(in crate::cargo) fn complete_harness_in(
        self,
        cached: &cargo::CachedToolchain,
        layout: &layout::Layout,
        built: cargo::Built,
        limits: &Limits,
        cargo_limits: &Limits,
    ) -> Result<cargo::Resolution> {
        let completed = self.complete_parts_in(cached, layout, built, limits, cargo_limits)?;
        Ok(completed.into_resolution(self))
    }

    /// Complete the same bounded cold tail while retaining the immutable request
    /// for subsequent current replay and graph validation.
    pub(in crate::cargo) fn complete_parts_in(
        &self,
        cached: &cargo::CachedToolchain,
        layout: &layout::Layout,
        built: cargo::Built,
        limits: &Limits,
        cargo_limits: &Limits,
    ) -> Result<cargo::CompletedHarness> {
        let cargo::Built { harness, build } = built;
        let native = self.native_for_harness_in(cached, layout, &harness, cargo_limits)?;
        require_configs(layout)?;
        let paths = layout.final_form();
        cargo::revalidate_harness(
            self,
            paths.manifest(),
            harness.manifest,
            paths.source(),
            harness.source,
            paths.lock(),
            harness.lock,
            limits,
            "reserved completion",
        )?;
        let manifest = paths.manifest().to_path_buf();
        let source = paths.source().to_path_buf();
        let lock = paths.lock().to_path_buf();
        self.complete_harness_parts(
            &native.production(),
            harness,
            build,
            limits,
            [manifest, source, lock],
            layout.receipt(),
            None,
            Some(layout),
        )
    }

    /// Consume a prepared Layout harness into ordinary Build observations with
    /// the unchanged allocation lineage.
    pub(in crate::cargo) fn build_harness_in(
        &self,
        cached: &cargo::CachedToolchain,
        layout: &layout::Layout,
        mut harness: cargo::Harness,
        limits: &Limits,
        cargo_limits: &Limits,
    ) -> Result<cargo::Built> {
        crate::progress::built();
        let native = self.native_for_harness_in(cached, layout, &harness, cargo_limits)?;
        let native = native.production();
        let paths = layout.final_form();
        let environment =
            cargo::cargo_authoritative_environment(&native, self, &harness.extraction_plan)?;
        harness
            .workspace
            .require(&mut crate::input::workspace::Account::bounded(limits))?;
        let (bytes, environment) = build(&native, self, layout, environment, limits)?;
        let (build, initial) = self.check_build(
            &bytes,
            environment,
            harness.initial,
            &harness.cargo_allocation,
            &harness.toolchain,
            [
                (paths.manifest(), harness.manifest),
                (paths.source(), harness.source),
                (paths.lock(), harness.lock),
            ],
            limits,
            cargo_limits,
            &harness.workspace,
        )?;
        harness.initial = initial;
        Ok(cargo::Built { harness, build })
    }

    fn native_for_harness_in(
        &self,
        cached: &cargo::CachedToolchain,
        layout: &layout::Layout,
        harness: &cargo::Harness,
        cargo_limits: &Limits,
    ) -> Result<crate::process::Native> {
        harness.cargo_allocation.require(cargo_limits);
        if &harness.toolchain != cached.attestation.value().proof() {
            return Err(Error::ResolutionDrift {
                command: "reserved Build preparation",
                field: "prepared harness toolchain",
            });
        }
        let paths = layout.final_form();
        if harness.dag.is_some()
            || harness.documents.is_none()
            || harness.harness_root.as_os_str() != paths.root().as_os_str()
            || harness.command_target.as_os_str() != paths.target().as_os_str()
            || harness.extraction_plan.target_directory().as_os_str() != paths.target().as_os_str()
            || harness.extraction_plan.protocol_directory().as_os_str()
                != layout.extraction().protocol().as_os_str()
        {
            return Err(Error::ResolutionDrift {
                command: "reserved Build preparation",
                field: "prepared harness and extraction Layout",
            });
        }
        Ok(crate::process::Native::from_attestation(
            &cached.attestation,
        ))
    }

    /// Prepare only a selected Fresh versioned harness under its existing owner.
    /// This is not a dispatcher, persisted replay, or publication constructor.
    #[cfg(test)]
    pub(in crate::cargo) fn prepare_fresh_harness_in(
        &self,
        layout: &layout::Layout,
        cached: &cargo::CachedToolchain,
        limits: &Limits,
        cargo_limits: &Limits,
        cargo_allocation: cargo::unit::Allocation,
    ) -> Result<cargo::Harness> {
        self.prepare_selected_harness_in(
            layout,
            cached,
            limits,
            cargo_limits,
            cargo_allocation,
            true,
        )
    }

    /// Reconstruct a selected current-universe harness in the reserved Layout.
    /// A persisted receipt selects input bytes, never portable completion authority.
    pub(in crate::cargo) fn prepare_harness_in(
        &self,
        layout: &layout::Layout,
        cached: &cargo::CachedToolchain,
        limits: &Limits,
        cargo_limits: &Limits,
        cargo_allocation: cargo::unit::Allocation,
    ) -> Result<cargo::Harness> {
        self.prepare_selected_harness_in(
            layout,
            cached,
            limits,
            cargo_limits,
            cargo_allocation,
            false,
        )
    }

    fn prepare_selected_harness_in(
        &self,
        layout: &layout::Layout,
        cached: &cargo::CachedToolchain,
        limits: &Limits,
        cargo_limits: &Limits,
        mut cargo_allocation: cargo::unit::Allocation,
        fresh_only: bool,
    ) -> Result<cargo::Harness> {
        cargo_allocation.require(cargo_limits);
        let catalog = self
            .universe
            .as_ref()
            .expect("a reserved harness belongs to a request with a resolution universe");
        let provisional_manifest = cargo::render_manifest(self, None)?;
        let placement = cargo::select_placement(self, catalog, &provisional_manifest)?;
        assert!(
            !fresh_only || matches!(placement.state, cargo::PlacementState::Fresh),
            "a fresh-only harness is prepared only for a Fresh placement"
        );
        let toolchain = cached.attestation.value().proof().clone();
        let native = crate::process::Native::from_attestation(&cached.attestation);
        let native = native.production();
        self.revalidate(limits, "toolchain verification")?;
        let definition = catalog.current();
        let context = definition.validate_context(
            &self.target,
            &self.profile,
            toolchain.rustc_commit(),
            toolchain.cargo_commit(),
            self.config.projection_schema(),
            self.toolchain.schemas().graph(),
            self.toolchain.schemas().unit(),
            cohort(self, layout.paths())?,
            self.config.universe_context(),
            self.config.digest(),
        )?;
        require_configs(layout)?;
        let provisional = layout.provisional();
        let paths = layout.final_form();
        // Cargo's outputs for this request when only sources or the demand
        // changed.
        let remembered_base = super::remembered::base(&[
            cargo::cargo_identity(self, definition, &provisional_manifest).as_bytes(),
            context.digest().as_bytes(),
            toolchain.rustc_commit().as_bytes(),
            toolchain.cargo_commit().as_bytes(),
            self.target.as_bytes(),
            self.profile.as_bytes(),
        ]);
        let reuse = matches!(placement.state, cargo::PlacementState::Fresh)
            .then(|| {
                super::remembered::lookup(&remembered_base, layout.root(), cached.context.rawr())
            })
            .flatten();
        let mut produced = (None, None);
        let (metadata_transport, candidate_lock, internal) = match placement.state {
            cargo::PlacementState::Fresh => {
                cargo::write_atomic(provisional.manifest(), provisional_manifest.as_bytes())?;
                cargo::write_atomic(provisional.source(), cargo::HARNESS_SOURCE.as_bytes())?;
                // The provisional resolution starts from the lock the candidate
                // starts from (the project's own, else the universe's seed), so
                // Cargo reads its index cache instead of querying the registry
                // for every package.
                cargo::write_atomic(provisional.lock(), self.starting_lock(definition))?;
                drop(provisional_manifest);
                let materialized = self.materialize_fresh(
                    limits,
                    &mut cargo_allocation,
                    definition,
                    (paths.manifest(), paths.source(), paths.lock()),
                    |phase| match (phase, &reuse) {
                        (cargo::MetadataPhase::Provisional, Some(reuse)) => {
                            Ok(reuse.provisional.clone())
                        }
                        (cargo::MetadataPhase::Candidate, Some(reuse)) => {
                            cargo::write_atomic(paths.lock(), &reuse.lock)?;
                            Ok(reuse.candidate.clone())
                        }
                        (cargo::MetadataPhase::Provisional, None) => {
                            crate::progress::resolved();
                            let bytes = provisional_metadata(&native, self, layout, limits)?;
                            produced.0 = Some(bytes.clone());
                            Ok(bytes)
                        }
                        (cargo::MetadataPhase::Candidate, None) => {
                            let bytes = metadata(&native, self, layout, false, limits)?;
                            produced.1 = Some(bytes.clone());
                            Ok(bytes)
                        }
                    },
                    |bytes, epoch| {
                        cargo::unit::Metadata::parse_in(
                            bytes,
                            &self.dependencies,
                            cargo_limits,
                            self.toolchain.schemas().graph(),
                            provisional,
                            epoch,
                        )
                    },
                )?;
                (
                    materialized.bytes,
                    materialized.lock,
                    Some(materialized.internal),
                )
            }
            cargo::PlacementState::Persisted { lock, receipt } => {
                // File reads are I/O retention bounded by physical
                // constants (manifest class: tail; lock class: capture).
                let manifest = cargo::read_bounded(
                    "selected persisted manifest",
                    &placement.root.join(cargo::HARNESS_MANIFEST),
                    crate::process::TAIL_BYTES,
                )?;
                let source = cargo::read_bounded(
                    "selected persisted source",
                    &placement.root.join("src/lib.rs"),
                    crate::process::TAIL_BYTES,
                )?;
                let locked = cargo::read_bounded(
                    "selected persisted lock",
                    &placement.root.join("Cargo.lock"),
                    crate::process::CAPTURE_BYTES,
                )?;
                receipt.preflight_files(
                    cargo::digest(&manifest),
                    cargo::digest(&source),
                    cargo::digest(&locked),
                )?;
                if locked.as_slice() != lock.bytes() {
                    return Err(Error::ResolutionDrift {
                        command: "reserved persisted materialization",
                        field: "selected whole lock",
                    });
                }
                cargo::write_atomic(paths.manifest(), &manifest)?;
                cargo::write_atomic(paths.source(), &source)?;
                cargo::write_atomic(paths.lock(), &locked)?;
                crate::progress::resolved();
                let bytes = metadata(&native, self, layout, true, limits)?;
                self.revalidate(limits, "reserved locked persisted metadata")?;
                cargo::require_exact_file(
                    "reserved persisted lock after metadata",
                    paths.lock(),
                    lock.bytes(),
                    crate::process::CAPTURE_BYTES,
                )?;
                (bytes, lock, None)
            }
        };
        let metadata_bytes = cargo_allocation.retain_transport(metadata_transport)?;
        let (metadata, observation) = cargo::unit::Metadata::parse_versioned_in(
            metadata_bytes.bytes(),
            &self.dependencies,
            cargo_limits,
            self.toolchain.schemas().graph(),
            paths,
            &mut cargo_allocation,
            definition,
        )?;
        let (metadata, selected, internal) = self.bind_fresh_metadata(
            metadata,
            internal,
            &candidate_lock,
            (paths.manifest(), paths.source()),
            cargo_limits,
            &mut cargo_allocation,
        )?;
        let manifest = cargo::digest(&cargo::read_bounded(
            "final-form harness manifest",
            paths.manifest(),
            crate::process::TAIL_BYTES,
        )?);
        let source = cargo::digest(&cargo::read_bounded(
            "generated harness source",
            paths.source(),
            crate::process::TAIL_BYTES,
        )?);
        let lock = candidate_lock.digest();
        let workspace = self.capture_workspace(&metadata, limits)?;
        let bytes = match &reuse {
            Some(reuse) => reuse.units.clone(),
            None => units(&native, self, layout, limits)?,
        };
        cargo::require_exact_file(
            "reserved final lock after units",
            paths.lock(),
            candidate_lock.bytes(),
            crate::process::CAPTURE_BYTES,
        )?;
        if let (None, (Some(provisional), Some(candidate))) = (&reuse, produced) {
            super::remembered::remember(
                remembered_base,
                layout.root(),
                &workspace.roots,
                super::remembered::Outputs {
                    provisional,
                    candidate,
                    lock: candidate_lock.bytes().into(),
                    units: bytes.clone(),
                },
                cached.context.rawr(),
            );
        }
        let bytes = cargo_allocation.retain_transport(bytes)?;
        let (initial, universe) = self.bind_versioned_units(
            bytes.bytes(),
            &metadata,
            cargo_limits,
            &mut cargo_allocation,
            definition,
            observation,
            candidate_lock,
            placement.request,
            context,
            None,
        )?;
        // Which selected crates share their tables per machine, before
        // the unit graph's bytes move into the documents.
        let selected_units = initial
            .extraction()
            .iter()
            .map(|unit| crate::extract::shared::Selected {
                unit_key: unit.semantic_key(),
                package_id: unit.package_id(),
                target: unit.target(),
                features: unit.features(),
            })
            .collect::<Vec<_>>();
        let shared = crate::extract::shared::keys_local(
            bytes.bytes(),
            &metadata,
            &selected_units,
            toolchain.rustc_commit(),
            toolchain.host(),
            &self.target,
        );
        drop(selected_units);
        let documents = cargo::Documents::new(metadata_bytes, bytes);
        let mut extraction_plan = crate::extract::plan::Plan::prepare_in(
            layout,
            &initial,
            &toolchain,
            &self.target,
            limits,
        )?
        .demand(&self.demand)?
        .reuse(&cached.context.rawr().join("extraction"))?
        .share(
            shared,
            crate::bindings::directory(cached.context.cargo_home()),
            cached.context.rawr().join("bindings"),
            crate::bindings::staged(cached.context.rawr()),
        );
        let command_target = extraction_plan.command_target()?;
        Ok(cargo::Harness {
            context: Arc::new(cached.context.clone()),
            harness_root: paths.root().to_path_buf(),
            manifest,
            source,
            lock,
            metadata,
            workspace,
            cargo_allocation,
            selected,
            toolchain,
            internal,
            documents: Some(documents),
            universe: Some(universe),
            initial,
            command_target,
            dag: None,
            extraction_plan,
            closure_units: None,
        })
    }
}

pub(in crate::cargo) fn cohort(request: &Request, layout: &layout::Paths) -> Result<cargo::Digest> {
    cargo::command_cohort(
        request.toolchain.schemas().graph(),
        &request.target,
        &request.profile,
        request.parallelism,
        || prefix(request, Path::new("$RAWR_FINAL_BUILD_CONFIG")),
        Some(layout.final_form().config_bytes()),
    )
}

pub(in crate::cargo) fn provisional_metadata(
    native: &impl ProductionReplay,
    request: &Request,
    layout: &layout::Layout,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    require_configs(layout)?;
    metadata_at(native, request, layout.provisional(), false, limits)
}

pub(in crate::cargo) fn metadata(
    native: &impl ProductionReplay,
    request: &Request,
    layout: &layout::Layout,
    locked: bool,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    require_configs(layout)?;
    metadata_at(native, request, layout.final_form(), locked, limits)
}

fn metadata_at(
    native: &impl ProductionReplay,
    request: &Request,
    paths: &layout::Cargo,
    locked: bool,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let prefix = prefix(request, paths.config())?;
    let command = cargo::cargo_metadata_command(
        native,
        request,
        paths.manifest(),
        paths.target(),
        prefix,
        locked,
    )?;
    cargo::run_cargo(native, &command, limits)
}

pub(in crate::cargo) fn units(
    native: &impl ProductionReplay,
    request: &Request,
    layout: &layout::Layout,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    require_configs(layout)?;
    let paths = layout.final_form();
    let prefix = prefix(request, paths.config())?;
    let command =
        cargo::cargo_unit_command(native, request, paths.manifest(), paths.target(), prefix)?;
    cargo::run_cargo(native, &command, limits)
}

#[allow(
    clippy::type_complexity,
    reason = "Build parsing retains the exact successful command environment"
)]
pub(in crate::cargo) fn build(
    native: &impl ProductionReplay,
    request: &Request,
    layout: &layout::Layout,
    environment: Box<[(OsString, OsString)]>,
    limits: &Limits,
) -> Result<(Box<[u8]>, Box<[(OsString, OsString)]>)> {
    require_configs(layout)?;
    let paths = layout.final_form();
    // The shared directory overrides the reserved config's bundle build root;
    // relocation then fills that root with the files this resolution reads.
    let prefix = prefix(request, paths.config())?;
    shared_build(
        native,
        request,
        prefix,
        paths.manifest(),
        environment,
        (paths.target(), paths.build()),
        limits,
    )
}

/// Build `manifest` in the pinned nightly's build directory and copy what
/// Cargo's messages name into `build`, the build root the unit graph names;
/// the messages returned name the copies. `target` is the Cargo target
/// directory, whose outputs are made the build's own (`relocate::unshare`).
#[allow(
    clippy::type_complexity,
    reason = "Build parsing retains the exact successful command environment"
)]
pub(in crate::cargo) fn shared_build(
    native: &impl ToolPaths,
    request: &Request,
    mut prefix: Vec<OsString>,
    manifest: &Path,
    environment: Box<[(OsString, OsString)]>,
    (target, build): (&Path, &Path),
    limits: &Limits,
) -> Result<(Box<[u8]>, Box<[(OsString, OsString)]>)> {
    // One build directory per machine per pinned toolchain, in rawr's home
    // (D700). Only this pinned nightly build uses it, so Cargo's freshness
    // there is this build's alone, `cargo build` never sees its units, and a
    // second project rechecks only what its own closure adds.
    let home = home(request)?;
    let shared = crate::deps::nightly_directory(&home);
    prefix.extend(shared_arguments(&shared));
    let tail = cargo::cargo_build_arguments(manifest, &request.target, &request.profile);
    let clean = prefix.clone();
    let command = cargo::cargo_build_command(request, prefix, tail, environment.clone())?;
    // Held across the Cargo run and the copy of what it wrote: another
    // project's build would rewrite those files, and cleanup skips a held
    // directory. Extraction then reads the bundle's copies.
    let lock = crate::deps::nightly_lock(&shared).map_err(|source| Error::Io {
        operation: "lock rawr's nightly build directory",
        path: shared.clone(),
        source,
    })?;
    // A package whose build script gave a project other outputs than its
    // run here is cleaned first, so Cargo runs the script again with this
    // build's environment (D742). One this manifest doesn't resolve stays
    // marked for a build that does.
    let stale = crate::bindings::Stale::of(&shared);
    for package in stale.pending() {
        let command = cargo::cargo_clean_command(
            request,
            clean.clone(),
            manifest,
            &package,
            environment.clone(),
        )?;
        if native
            .run(&command, limits.process)
            .and_then(|output| output.checked(Program::Cargo))
            .is_ok()
        {
            stale.cleaned(&package);
        }
    }
    let bytes = native
        .run(&command, limits.process)?
        .checked(Program::Cargo)?;
    super::relocate::unshare(target)?;
    let bytes = super::relocate::relocate(&bytes, &shared, build)?;
    drop(lock);
    crate::deps::retire_nightly(&home, &shared);
    Ok((bytes, command.into_environment()))
}

/// Rawr's home for `request`'s environment.
pub(in crate::cargo) fn home(request: &Request) -> Result<std::path::PathBuf> {
    let cargo_home = request
        .config
        .environment()
        .cargo_home()?
        .ok_or_else(|| Error::Request {
            reason: "rawr's home needs the environment variable `CARGO_HOME` or `HOME`".into(),
        })?;
    Ok(crate::store::home(&cargo_home))
}

/// Build in `shared`, the pinned nightly's build directory, with Cargo's own
/// freshness rules, so a later resolution, of this project or another,
/// reuses this one's units.
pub(in crate::cargo) fn shared_arguments(shared: &Path) -> [OsString; 2] {
    let value = toml_edit::Value::from(shared.to_string_lossy().as_ref()).to_string();
    ["--config".into(), format!("build.build-dir={value}").into()]
}

pub(in crate::cargo) fn require_configs(layout: &layout::Layout) -> Result<()> {
    layout.require_configs().map_err(|source| Error::Node {
        operation: "reserved Cargo build configuration",
        source: Box::new(source),
    })
}

pub(in crate::cargo) fn prefix(request: &Request, config: &Path) -> Result<Vec<OsString>> {
    let mut prefix = cargo::config_arguments(request.config.explicit_configs());
    // Cargo applies later --config values after the explicit config files.
    prefix.push("--config".into());
    prefix.push(config.as_os_str().to_owned());
    Ok(prefix)
}

#[cfg(test)]
mod tests;
