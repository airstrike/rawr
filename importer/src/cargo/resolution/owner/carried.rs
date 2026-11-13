//! What a resolution carries to the next demand round of its build in this
//! process (#1618).
//!
//! A demand changes what the probe extracts and nothing Cargo sees. A
//! round whose build is the one an earlier round of this process completed
//! reads the selected crates' staged tables under that round's harness, as
//! shared bindings: when a table lacks units the new demand reaches, the
//! earlier probe's plan runs again for it over the same sealed artifacts,
//! and the driver grows the tables. The round binds no harness, runs no
//! build and seals no bundle. Anything that does not replay goes the
//! ordinary way.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest as _, Sha256};

use crate::cargo::{self, unit, Request};
use crate::extract::plan::{Plan, Reprobe, Unreplayed};

/// The parts of one harness a shared replay reads.
pub(super) struct Basis<'a> {
    pub(super) plan: &'a Plan,
    pub(super) graph: &'a unit::BuildGraph,
    pub(super) metadata: &'a unit::Metadata,
    pub(super) selected: &'a [cargo::Selection],
    pub(super) toolchain: &'a cargo::toolchain::Proof,
    pub(super) manifest: &'a Path,
    pub(super) lock: &'a Path,
}

/// One completed round's harness and probe, for the next demand of its build.
pub(super) struct Carried {
    plan: Plan,
    graph: unit::BuildGraph,
    metadata: unit::Metadata,
    selected: Box<[cargo::Selection]>,
    toolchain: cargo::toolchain::Proof,
    manifest: PathBuf,
    lock: PathBuf,
    reprobe: Option<Reprobe>,
}

impl Carried {
    /// What `harness`'s round carries, before its probe.
    pub(super) fn of(
        harness: &cargo::Harness,
        manifest: &Path,
        lock: &Path,
        limits: &cargo::Limits,
    ) -> Self {
        Self {
            plan: harness.extraction_plan.carry(limits),
            graph: harness.initial.clone(),
            metadata: harness.metadata.clone(),
            selected: harness.selected.clone(),
            toolchain: harness.toolchain.clone(),
            manifest: manifest.to_path_buf(),
            lock: lock.to_path_buf(),
            reprobe: None,
        }
    }

    /// This round, once `reprobe` grew its tables.
    pub(super) fn probed(mut self, reprobe: Reprobe) -> Self {
        self.reprobe = Some(reprobe);
        self
    }

    /// This round's harness, with its plan for `plan`'s demand.
    pub(super) fn basis<'a>(&'a self, plan: &'a Plan) -> Basis<'a> {
        Basis {
            plan,
            graph: &self.graph,
            metadata: &self.metadata,
            selected: &self.selected,
            toolchain: &self.toolchain,
            manifest: &self.manifest,
            lock: &self.lock,
        }
    }

    pub(super) const fn plan(&self) -> &Plan {
        &self.plan
    }

    pub(super) const fn reprobe(&self) -> Option<&Reprobe> {
        self.reprobe.as_ref()
    }
}

impl super::Evaluation {
    /// The shared bindings of this demand round, when an earlier round of
    /// this process completed its build: its staged tables, grown first by
    /// its probe run again when they lack a unit this demand reaches. Only
    /// the tables the probe grew are replayed again: `tables` keeps the
    /// others' replays.
    pub(super) fn carried_parts(
        &self,
        attestation: &[u8; 32],
        tables: &mut crate::extract::plan::replayed::Tables,
    ) -> Option<super::Shared> {
        let carried = carried(&key(&self.request, attestation)?)?;
        let plan = carried
            .plan()
            .carry(&self.limits)
            .demand(&self.request.demand)
            .ok()?;
        match self.shared_parts(&carried.basis(&plan), tables) {
            Ok(parts) => return Some(parts),
            // The carried round's key names Cargo's view, not the crates'
            // files: after an edit to the project the key holds, but the
            // build is another, which its harness probes once (#1794).
            Err(Unreplayed::Changed) => return None,
            Err(Unreplayed::Unanswered) => {}
        }
        let rawr = self.cached.context.rawr();
        let native = crate::process::Native::from_attestation(&self.cached.attestation);
        carried
            .reprobe()?
            .run(
                &native.production(),
                &self.request.demand,
                &rawr.join("reprobe"),
                &self.limits,
            )
            .ok()?;
        crate::bindings::restage(&crate::bindings::staged(rawr), &plan.shared_keys());
        self.shared_parts(&carried.basis(&plan), tables).ok()
    }
}

/// The build each carried round is of: the request as Cargo sees it, with
/// no demand, under one pinned toolchain.
pub(super) fn key(request: &Request, attestation: &[u8; 32]) -> Option<[u8; 32]> {
    let catalog = request.universe.as_ref()?;
    let manifest = cargo::render_manifest(request, None).ok()?;
    let cargo = cargo::cargo_identity(request, catalog.current(), &manifest);
    let mut key = Sha256::new();
    key.update(b"rawr.resolution.carried/v1");
    key.update(cargo.as_bytes());
    key.update(attestation);
    key.update(request.target.as_bytes());
    key.update([0]);
    key.update(request.profile.as_bytes());
    Some(key.finalize().into())
}

/// The latest round of this process, by its build: one build is checked
/// at a time.
static CARRIED: Mutex<Option<([u8; 32], Arc<Carried>)>> = Mutex::new(None);

/// Keep `carried` for the next round of the build `key` names.
pub(super) fn keep(key: [u8; 32], carried: Carried) {
    if let Ok(mut kept) = CARRIED.lock() {
        *kept = Some((key, Arc::new(carried)));
    }
}

/// What the latest round of the build `key` names carried.
pub(super) fn carried(key: &[u8; 32]) -> Option<Arc<Carried>> {
    let kept = CARRIED.lock().ok()?;
    kept.as_ref()
        .filter(|(kept, _)| kept == key)
        .map(|(_, carried)| Arc::clone(carried))
}
