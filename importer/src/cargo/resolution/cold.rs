//! Direct Cargo commands inside an already-owned, unsealed resolution Layout.

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use super::layout;
use crate::cargo::{self, Limits, Request, RequestAllocation};
use crate::process::{ProductionReplay, Program};
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
        limits: &Limits,
        cargo_limits: &Limits,
        allocation: &mut RequestAllocation,
        cargo_allocation: &mut cargo::unit::Allocation,
    ) -> Result<(
        cargo::unit::Metadata,
        Box<[cargo::Selection]>,
        cargo::internal::Recipe,
        cargo::unit::BuildGraph,
        cargo::universe::Pending,
    )> {
        self.require_limits(limits)?;
        cargo_allocation.require(cargo_limits)?;
        if !allocation.matches(limits)
            || allocation.items < self.allocation.items
            || allocation.bytes < self.allocation.bytes
        {
            return Err(Error::Request {
                field: "retained Cargo document allocation",
                reason: "does not retain the original request account".into(),
            });
        }
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
            limits,
            cargo_limits,
            cargo_allocation,
            allocation,
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
        built: cargo::Built<'_>,
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
        built: cargo::Built<'_>,
        limits: &Limits,
        cargo_limits: &Limits,
    ) -> Result<cargo::CompletedHarness> {
        let cargo::Built {
            harness,
            build,
            allocation,
        } = built;
        let native =
            self.native_for_harness_in(cached, layout, &harness, allocation, limits, cargo_limits)?;
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
        allocation.path(paths.manifest())?;
        let manifest = paths.manifest().to_path_buf();
        allocation.path(paths.source())?;
        let source = paths.source().to_path_buf();
        allocation.path(paths.lock())?;
        let lock = paths.lock().to_path_buf();
        self.complete_harness_parts(
            &native.production(),
            harness,
            build,
            allocation,
            limits,
            [manifest, source, lock],
            layout.receipt(),
            None,
            None,
            Some(layout),
        )
    }

    /// Consume a prepared Layout harness into ordinary Build observations with
    /// both exact postchecks and the unchanged allocation lineage.
    #[allow(
        clippy::too_many_arguments,
        reason = "the Build phase retains both original limit profiles"
    )]
    pub(in crate::cargo) fn build_harness_in<'allocation>(
        &self,
        cached: &cargo::CachedToolchain,
        layout: &layout::Layout,
        mut harness: cargo::Harness,
        allocation: &'allocation mut RequestAllocation,
        limits: &Limits,
        cargo_limits: &Limits,
    ) -> Result<cargo::Built<'allocation>> {
        let native =
            self.native_for_harness_in(cached, layout, &harness, allocation, limits, cargo_limits)?;
        let native = native.production();
        let paths = layout.final_form();
        let environment =
            cargo::cargo_authoritative_environment(&native, self, &harness.extraction_plan)?;
        harness
            .workspace
            .require(&mut crate::input::workspace::Account::borrow(
                allocation,
                cached.context.graph_budget(),
                limits,
            ))?;
        let (bytes, environment) = build(&native, self, layout, environment, allocation, limits)?;
        let (build, initial) = self.check_build(
            &bytes,
            environment,
            &harness.metadata,
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
            cached.context.graph_budget(),
            allocation,
            // The harness files and workspace manifests were revalidated
            // around the build and the inputs are captured after it: Cargo's
            // metadata and unit graph are functions of those, so asking Cargo
            // for them again only reproduced the bytes it gave before.
            None::<fn(cargo::Postcheck, &mut RequestAllocation) -> Result<Box<[u8]>>>,
        )?;
        harness.initial = initial;
        Ok(cargo::Built {
            harness,
            build,
            allocation,
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the native join retains both original limit profiles"
    )]
    fn native_for_harness_in(
        &self,
        cached: &cargo::CachedToolchain,
        layout: &layout::Layout,
        harness: &cargo::Harness,
        allocation: &RequestAllocation,
        limits: &Limits,
        cargo_limits: &Limits,
    ) -> Result<crate::process::Native> {
        self.require_limits(limits)?;
        harness.cargo_allocation.require(cargo_limits)?;
        if &harness.toolchain != cached.attestation.value().proof() {
            return Err(Error::ResolutionDrift {
                command: "reserved Build preparation",
                field: "prepared harness toolchain",
            });
        }
        if !allocation.matches(limits)
            || allocation.items < self.allocation.items
            || allocation.bytes < self.allocation.bytes
        {
            return Err(Error::Request {
                field: "reserved Build allocation",
                reason: "does not retain the original request account".into(),
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
        allocation: &mut RequestAllocation,
        cargo_allocation: cargo::unit::Allocation,
    ) -> Result<cargo::Harness> {
        self.prepare_selected_harness_in(
            layout,
            cached,
            limits,
            cargo_limits,
            allocation,
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
        allocation: &mut RequestAllocation,
        cargo_allocation: cargo::unit::Allocation,
    ) -> Result<cargo::Harness> {
        self.prepare_selected_harness_in(
            layout,
            cached,
            limits,
            cargo_limits,
            allocation,
            cargo_allocation,
            false,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "selected preparation retains both accounts and the fresh-only compatibility boundary"
    )]
    fn prepare_selected_harness_in(
        &self,
        layout: &layout::Layout,
        cached: &cargo::CachedToolchain,
        limits: &Limits,
        cargo_limits: &Limits,
        allocation: &mut RequestAllocation,
        mut cargo_allocation: cargo::unit::Allocation,
        fresh_only: bool,
    ) -> Result<cargo::Harness> {
        self.require_limits(limits)?;
        cargo_allocation.require(cargo_limits)?;
        if !allocation.matches(limits)
            || allocation.items < self.allocation.items
            || allocation.bytes < self.allocation.bytes
        {
            return Err(Error::Request {
                field: "reserved harness allocation",
                reason: "does not retain the original request account".into(),
            });
        }
        let catalog = self.universe.as_ref().ok_or_else(|| Error::Request {
            field: "reserved fresh harness",
            reason: "requires a versioned resolution universe".into(),
        })?;
        let provisional_manifest =
            cargo::render_manifest_accounted(self, None, limits, allocation)?;
        let placement = cargo::select_placement(self, catalog, &provisional_manifest, limits)?;
        if fresh_only && !matches!(placement.state, cargo::PlacementState::Fresh) {
            return Err(Error::Request {
                field: "reserved fresh harness",
                reason:
                    "requires a Fresh placement; persisted authority is not materialization input"
                        .into(),
            });
        }
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
            cohort(self, layout.paths(), allocation)?,
            self.config.universe_context(),
            self.config.digest(),
        )?;
        require_configs(layout)?;
        let provisional = layout.provisional();
        let paths = layout.final_form();
        // Cargo's outputs for this request when only sources changed.
        let remembered_base = super::remembered::base(&[
            placement.request.as_bytes(),
            context.digest().as_bytes(),
            toolchain.rustc_commit().as_bytes(),
            toolchain.cargo_commit().as_bytes(),
            self.target.as_bytes(),
            self.profile.as_bytes(),
        ]);
        let reuse = matches!(placement.state, cargo::PlacementState::Fresh)
            .then(|| super::remembered::lookup(&remembered_base, layout.root()))
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
                    allocation,
                    definition,
                    (paths.manifest(), paths.source(), paths.lock()),
                    |phase, allocation| match (phase, &reuse) {
                        (cargo::MetadataPhase::Provisional, Some(reuse)) => {
                            Ok(reuse.provisional.clone())
                        }
                        (cargo::MetadataPhase::Candidate, Some(reuse)) => {
                            cargo::write_atomic(paths.lock(), &reuse.lock)?;
                            Ok(reuse.candidate.clone())
                        }
                        (cargo::MetadataPhase::Provisional, None) => {
                            let bytes =
                                provisional_metadata(&native, self, layout, allocation, limits)?;
                            produced.0 = Some(bytes.clone());
                            Ok(bytes)
                        }
                        (cargo::MetadataPhase::Candidate, None) => {
                            let bytes = metadata(&native, self, layout, false, allocation, limits)?;
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
                // constants (manifest class: tail; lock class: capture), never
                // by an accounting target.
                let manifest = cargo::read_bounded_budgeted(
                    "selected persisted manifest",
                    &placement.root.join(cargo::HARNESS_MANIFEST),
                    crate::process::TAIL_BYTES,
                    allocation,
                )?;
                let source = cargo::read_bounded_budgeted(
                    "selected persisted source",
                    &placement.root.join("src/lib.rs"),
                    crate::process::TAIL_BYTES,
                    allocation,
                )?;
                let locked = cargo::read_bounded_budgeted(
                    "selected persisted lock",
                    &placement.root.join("Cargo.lock"),
                    crate::process::CAPTURE_BYTES,
                    allocation,
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
                let bytes = metadata(&native, self, layout, true, allocation, limits)?;
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
            limits,
            cargo_limits,
            &mut cargo_allocation,
            allocation,
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
        let workspace =
            self.capture_workspace(&metadata, allocation, cached.context.graph_budget(), limits)?;
        let bytes = match &reuse {
            Some(reuse) => reuse.units.clone(),
            None => units(&native, self, layout, allocation, limits)?,
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
        let documents = cargo::Documents::bind(metadata_bytes, bytes, &metadata, &initial)?;
        let mut extraction_plan = crate::extract::plan::Plan::prepare_in(
            layout,
            &initial,
            &toolchain,
            &self.target,
            limits,
        )?
        .demand(&self.demand)?
        .reuse(&cached.context.rawr().join("extraction"))?;
        let command_target = extraction_plan.command_target()?;
        allocation.path(paths.root())?;
        Ok(cargo::Harness {
            context: Arc::new(cached.context.clone()),
            harness_root: paths.root().to_path_buf(),
            manifest,
            source,
            lock,
            metadata,
            workspace,
            request_allocation: None,
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
        })
    }
}

pub(in crate::cargo) fn cohort(
    request: &Request,
    layout: &layout::Paths,
    allocation: &mut RequestAllocation,
) -> Result<cargo::Digest> {
    cargo::command_cohort(
        request.toolchain.schemas().graph(),
        &request.target,
        &request.profile,
        request.parallelism,
        || prefix(request, Path::new("$RAWR_FINAL_BUILD_CONFIG"), allocation),
        Some(layout.final_form().config_bytes()),
    )
}

pub(in crate::cargo) fn provisional_metadata(
    native: &impl ProductionReplay,
    request: &Request,
    layout: &layout::Layout,
    allocation: &mut RequestAllocation,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    require_configs(layout)?;
    metadata_at(
        native,
        request,
        layout.provisional(),
        false,
        allocation,
        limits,
    )
}

pub(in crate::cargo) fn metadata(
    native: &impl ProductionReplay,
    request: &Request,
    layout: &layout::Layout,
    locked: bool,
    allocation: &mut RequestAllocation,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    require_configs(layout)?;
    metadata_at(
        native,
        request,
        layout.final_form(),
        locked,
        allocation,
        limits,
    )
}

fn metadata_at(
    native: &impl ProductionReplay,
    request: &Request,
    paths: &layout::Cargo,
    locked: bool,
    allocation: &mut RequestAllocation,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let prefix = prefix(request, paths.config(), allocation)?;
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
    allocation: &mut RequestAllocation,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    require_configs(layout)?;
    let paths = layout.final_form();
    let prefix = prefix(request, paths.config(), allocation)?;
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
    allocation: &mut RequestAllocation,
    limits: &Limits,
) -> Result<(Box<[u8]>, Box<[(OsString, OsString)]>)> {
    require_configs(layout)?;
    let paths = layout.final_form();
    let rawr = crate::location::rawr_directory(&request.project_root, request.config.environment());
    let io = |operation, path: &Path| {
        let path = path.to_path_buf();
        move |source| Error::Io {
            operation,
            path,
            source,
        }
    };
    // Rawr's own build directory under `<target>/rawr`: only this pinned
    // nightly build uses it, so Cargo's freshness there is this build's
    // alone, and `cargo build` never sees its units (D609.2).
    let build = rawr.join("build");
    let shared = crate::deps::directory(&build)
        .map_err(io("create Rawr's Cargo build directory", &build))?;
    let mut prefix = prefix(request, paths.config(), allocation)?;
    // The shared directory overrides the reserved config's bundle build root;
    // relocation then fills that root with the files this resolution reads.
    prefix.extend(shared_arguments(&shared));
    let tail = cargo::cargo_build_arguments(paths.manifest(), &request.target, &request.profile);
    let command = cargo::cargo_build_command(request, prefix, tail, environment)?;
    let _lock = crate::deps::lock(&rawr).map_err(io("lock shared Cargo build directory", &rawr))?;
    let bytes = native
        .run(&command, limits.process)?
        .checked(Program::Cargo)?;
    crate::deps::prune_rawr(&shared, &bytes);
    super::relocate::unshare(paths.target())?;
    let bytes = super::relocate::relocate(&bytes, &shared, paths.build())?;
    Ok((bytes, command.into_environment()))
}

/// Build in `shared`, Rawr's own build directory, with Cargo's own
/// freshness rules, so a later resolution reuses this one's units.
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

pub(in crate::cargo) fn prefix(
    request: &Request,
    config: &Path,
    allocation: &mut RequestAllocation,
) -> Result<Vec<OsString>> {
    // The request string target is an accounting target; a longer
    // config path is recorded on the request account and the build continues.
    allocation.exceeded(
        cargo::REQUEST_STRING_RESOURCE,
        allocation.string_limit,
        config.as_os_str().len(),
    );
    allocation.string("--config")?;
    allocation.path(config)?;
    let mut prefix = cargo::config_arguments(request.config.explicit_configs());
    let required = prefix.len().checked_add(2).ok_or(Error::Limit {
        resource: "request retained bytes",
        limit: allocation.byte_limit,
        actual: usize::MAX,
    })?;
    if required > prefix.capacity() {
        let layout = std::alloc::Layout::array::<OsString>(required).map_err(|_| Error::Limit {
            resource: "request retained bytes",
            limit: allocation.byte_limit,
            actual: usize::MAX,
        })?;
        allocation.bytes(layout.size())?;
    }
    prefix
        .try_reserve_exact(2)
        .map_err(|error| Error::Request {
            field: "reserved Cargo command prefix",
            reason: error.to_string().into_boxed_str(),
        })?;
    // Cargo applies later --config values after the explicit config files.
    prefix.push("--config".into());
    prefix.push(config.as_os_str().to_owned());
    Ok(prefix)
}

#[cfg(test)]
mod tests;
