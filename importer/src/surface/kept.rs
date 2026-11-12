//! The demand a project's last runner was built from (#1743, D936.5): a
//! request whose native items that runner's adapters all cover takes the
//! runner's demand instead of its own, so it names the same runner, and a
//! start after a save that dropped native uses builds nothing. The runner
//! keeps the adapters whose use sites are gone.
//!
//! The kept demand is no authority of its own: a request takes it only
//! when the widened request's build key is the kept runner's, which holds
//! the native inputs' proof, the crates' contract and the program's
//! profile (D936), so a runner it names was built from exactly these items.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{
    adapter, identity, unique_applications, unique_identities, Fingerprint, Hashed, Request,
    StoredCanonical, SurfaceCanonical, UniqueIdentities, SURFACE_SCHEMA,
};
use crate::bridge::BuildRequestKey;

/// The layout of a kept demand. One of another is read as none.
const SCHEMA: u32 = 1;

/// The demand of the runner a project last built or took from the store,
/// with the build key that names it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    schema: u32,
    build: [u8; 32],
    stored: StoredCanonical,
    plans: Box<[adapter::Plan]>,
}

/// A kept demand as written, borrowed from the request it keeps.
#[derive(Serialize)]
struct Written<'a> {
    schema: u32,
    build: &'a [u8; 32],
    stored: &'a StoredCanonical,
    plans: &'a [adapter::Plan],
}

impl Kept {
    /// The kept demand in `bytes`, as [`Kept::encode`] wrote it; `None` when
    /// it is of another layout or no kept demand.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice::<Self>(bytes)
            .ok()
            .filter(|kept| kept.schema == SCHEMA)
    }

    /// Whether `key` names the kept runner.
    #[must_use]
    pub fn names(&self, key: &BuildRequestKey) -> bool {
        *key.as_bytes() == self.build
    }

    /// What a project keeps of `request`, whose runner is `build`.
    pub(crate) fn encode<B>(
        request: &Request<B>,
        build: &BuildRequestKey,
    ) -> Result<Vec<u8>, super::Error> {
        let stored = serde_json::from_slice::<StoredCanonical>(&request.canonical)
            .map_err(|_| super::Error::Canonical)?;
        serde_json::to_vec(&Written {
            schema: SCHEMA,
            build: build.as_bytes(),
            stored: &stored,
            plans: &request.plans,
        })
        .map_err(|_| super::Error::Encoding)
    }

    /// `request` with this demand's native items where they cover its own:
    /// the same contract, builders and applications, and every native item
    /// it uses among this demand's. `key` names the runner a request needs;
    /// the widened request is taken only when it names the kept runner,
    /// and `request` comes back unchanged otherwise.
    pub(crate) fn widen<B>(
        &self,
        request: Request<B>,
        key: impl Fn(&Request<B>) -> Option<BuildRequestKey>,
    ) -> Request<B> {
        let Some(stored) = self.covering(&request) else {
            return request;
        };
        let Ok(canonical) = serde_json::to_vec(&stored) else {
            return request;
        };
        let Some(fingerprint) = fingerprint(&stored) else {
            return request;
        };
        let added = stored.entries.len() - request.entries.len();
        let planned = request
            .plans
            .iter()
            .map(adapter::Plan::identity)
            .collect::<BTreeSet<_>>();
        // The kept plans, read again rather than cloned: a plan is no
        // value to copy.
        let Some(mut plans) = serde_json::to_vec(&self.plans)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<adapter::Plan>>(&bytes).ok())
        else {
            return request;
        };
        plans.retain(|plan| !planned.contains(&plan.identity()));
        let extra = plans
            .iter()
            .map(adapter::Plan::identity)
            .collect::<BTreeSet<_>>();
        let Request {
            _brand,
            #[cfg(not(target_arch = "wasm32"))]
            runner,
            contract,
            snapshot,
            entries,
            plans: own,
            canonical: own_canonical,
            fingerprint: own_fingerprint,
            mut allocation,
        } = request;
        let grown = canonical.len().saturating_sub(own_canonical.len());
        let retained = allocation.retain(added, grown).is_ok();
        let mut widened_plans = own.into_vec();
        widened_plans.append(&mut plans);
        widened_plans.sort_unstable_by_key(adapter::Plan::identity);
        let widened = Request {
            _brand,
            #[cfg(not(target_arch = "wasm32"))]
            runner,
            contract,
            snapshot,
            entries: stored.entries,
            plans: widened_plans.into_boxed_slice(),
            canonical: canonical.into_boxed_slice(),
            fingerprint,
            allocation,
        };
        if retained && key(&widened).is_some_and(|key| self.names(&key)) {
            return widened;
        }
        // Another runner: the request's own demand, as it came.
        let Request {
            _brand,
            #[cfg(not(target_arch = "wasm32"))]
            runner,
            contract,
            snapshot,
            plans: widened_plans,
            allocation,
            ..
        } = widened;
        let mut plans = widened_plans.into_vec();
        plans.retain(|plan| !extra.contains(&plan.identity()));
        Request {
            _brand,
            #[cfg(not(target_arch = "wasm32"))]
            runner,
            contract,
            snapshot,
            entries,
            plans: plans.into_boxed_slice(),
            canonical: own_canonical,
            fingerprint: own_fingerprint,
            allocation,
        }
    }

    /// The kept demand at `request`'s snapshot, with `request`'s use sites
    /// and those of the kept items it no longer uses, when the kept runner
    /// covers `request` and has more; `None` otherwise.
    fn covering<B>(&self, request: &Request<B>) -> Option<StoredCanonical> {
        let current = serde_json::from_slice::<StoredCanonical>(&request.canonical).ok()?;
        let kept = &self.stored;
        if current.schema != kept.schema
            || current.contract != kept.contract
            || current.builders != kept.builders
            || unique_applications(&current.applications) != unique_applications(&kept.applications)
        {
            return None;
        }
        let have = unique_identities(&current.entries)
            .cloned()
            .collect::<BTreeSet<identity::Identity>>();
        let covered = unique_identities(&kept.entries)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !have.is_subset(&covered) || have.len() == covered.len() {
            return None;
        }
        let mut entries = current.entries.into_vec();
        entries.extend(
            kept.entries
                .iter()
                .filter(|entry| !have.contains(&entry.identity))
                .cloned(),
        );
        entries.sort_unstable();
        entries.dedup();
        Some(StoredCanonical {
            schema: current.schema,
            contract: current.contract,
            snapshot: current.snapshot,
            entries: entries.into_boxed_slice(),
            builders: current.builders,
            applications: current.applications,
        })
    }
}

/// The surface fingerprint of `stored`, as a sealed demand's is.
fn fingerprint(stored: &StoredCanonical) -> Option<Fingerprint> {
    let mut surface = Hashed {
        digest: Sha256::new(),
        bytes: 0,
        overflow: false,
    };
    serde_json::to_writer(
        &mut surface,
        &SurfaceCanonical {
            schema: SURFACE_SCHEMA,
            contract: &stored.contract,
            entries: UniqueIdentities(&stored.entries),
            builders: &stored.builders,
            applications: unique_applications(&stored.applications),
        },
    )
    .ok()?;
    (!surface.overflow).then(|| Fingerprint(surface.digest.finalize().into()))
}
