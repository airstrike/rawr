//! Complete retained-record interpretation without execution or publication.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::layout;
use crate::cargo::{self, internal, unit, universe, Limits, Request, Selection};
use crate::extract::plan::{namespace, Capture, Plan};
use crate::input::{build, stored};
use crate::progress::View;

#[cfg(test)]
thread_local! {
    /// What this test thread's replays did, by [`View`]: parsed, compared
    /// and kept.
    pub(in crate::cargo) static VIEWS: std::cell::Cell<[usize; 3]> =
        const { std::cell::Cell::new([0; 3]) };
}

/// Count what one replay did.
fn count(view: View) {
    crate::progress::view(view);
    #[cfg(test)]
    VIEWS.with(|views| {
        let mut counts = views.get();
        counts[view as usize] += 1;
        views.set(counts);
    });
}

/// Pure interpretation of one sealed record's exact bytes. The owner may
/// share it across the replays of every graph this process evaluates whose
/// observation names the same commitment. It grants no freshness: every
/// replay still observes the bundle, and compares its inputs unless every
/// stamp a kept comparison took still holds (`Kept`).
pub(in crate::cargo) struct Decoded {
    pub(in crate::cargo) metadata: unit::Metadata,
    pub(in crate::cargo) graph: unit::BuildGraph,
    pub(in crate::cargo) selected: Box<[Selection]>,
    pub(in crate::cargo) internal: internal::Recipe,
    retained: build::Retained,
    namespace: Option<Arc<namespace::Retained>>,
    stored: stored::Stored,
    pending: universe::Pending,
}

impl Decoded {
    /// The recorded input entries this interpretation's replay compares.
    pub(in crate::cargo) fn entries(&self) -> &[crate::input::proof::Entry] {
        self.stored.entries()
    }

    /// The context the universe evidence binds.
    pub(in crate::cargo) fn context(&self) -> universe::ContextProof {
        self.pending.context()
    }

    /// Whether `other` interprets the same record the same way, whatever
    /// account each was parsed under.
    #[cfg(test)]
    pub(in crate::cargo) fn same(&self, other: &Self) -> bool {
        format!("{:?}", self.metadata) == format!("{:?}", other.metadata)
            && self.graph.same_units(&other.graph)
            && self.selected == other.selected
            && self.internal == other.internal
            && self.stored.entries() == other.stored.entries()
            && self.namespace.is_some() == other.namespace.is_some()
            && self.context() == other.context()
    }
}

/// The Cargo documents of one record as the cold path or a decode bound
/// them: the final metadata, its selections and internal recipe, the unit
/// graph and the universe evidence a replay completes.
pub(in crate::cargo) struct Parsed {
    pub(in crate::cargo) metadata: unit::Metadata,
    pub(in crate::cargo) graph: unit::BuildGraph,
    pub(in crate::cargo) selected: Box<[Selection]>,
    pub(in crate::cargo) internal: internal::Recipe,
    pub(in crate::cargo) pending: universe::Pending,
}

/// The complete replay result, not a current node receipt or published owner.
pub(in crate::cargo) struct Replay {
    pub(in crate::cargo) decoded: Arc<Decoded>,
    pub(in crate::cargo) capture: Capture,
    pub(in crate::cargo) completion: universe::Completion,
    pub(in crate::cargo) input: stored::Completed,
    /// Stamps taken before this replay read its inputs.
    pub(in crate::cargo) watched: super::watched::Watched,
}

impl Request {
    /// Join authenticated bundle bytes to current semantic replay, carrying the
    /// original input account through framing and every interpretation step.
    /// A `staged` interpretation skips only the pure decode of record bytes the
    /// observation has already hashed against their sealed commitment; the
    /// caller is responsible for matching every key component exactly.
    #[allow(
        clippy::too_many_arguments,
        reason = "one replay binds the complete owner inputs and original allocation accounts"
    )]
    pub(in crate::cargo) fn replay_observed_in(
        &self,
        cached: &cargo::CachedToolchain,
        observed: super::bundle::PendingReplay<'_, '_, '_>,
        cargo_home: Option<&Path>,
        definition: &universe::Definition,
        lock: universe::Candidate,
        request: cargo::Digest,
        context: universe::ContextProof,
        receipt: Option<universe::Receipt>,
        staged: Option<Arc<Decoded>>,
        limits: &Limits,
        cargo_limits: &Limits,
        cargo_allocation: &mut unit::Allocation,
    ) -> stored::Result<Replay> {
        let (paths, bytes, inventory, budget) = observed.into_parts();
        let result = (|| {
            let (decoded, view) = match staged {
                Some(decoded) => (decoded, View::Compared),
                None => {
                    let record = stored::Record::decode(&bytes, budget)?;
                    let decoded = self.decode_record_in(
                        &paths,
                        &record,
                        definition,
                        lock,
                        request,
                        context,
                        receipt,
                        cargo_limits,
                        cargo_allocation,
                        budget,
                    )?;
                    (decoded, View::Parsed)
                }
            };
            self.replay_decoded_in(
                cached, &paths, cargo_home, decoded, view, request, inventory, limits, budget,
            )
        })();
        result.map_err(|error| budget.failure(error))
    }

    /// Interpret all five historical sections using current supplied associations
    /// and the caller's existing accounts. This is pure over the record's exact
    /// bytes; bundle/current-node validation and every current input observation
    /// remain the responsibility of the replay that consumes the result.
    #[allow(
        clippy::too_many_arguments,
        reason = "one decode binds the complete owner inputs and original allocation accounts"
    )]
    fn decode_record_in(
        &self,
        paths: &layout::Paths,
        record: &stored::Record<'_>,
        definition: &universe::Definition,
        lock: universe::Candidate,
        request: cargo::Digest,
        context: universe::ContextProof,
        receipt: Option<universe::Receipt>,
        cargo_limits: &Limits,
        cargo_allocation: &mut unit::Allocation,
        budget: &mut stored::Budget<'_>,
    ) -> stored::Result<Arc<Decoded>> {
        budget.check()?;
        let (metadata, selected, internal, graph, pending) = self.bind_retained_documents_in(
            paths,
            cargo::DocumentBytes::from_record(record),
            definition,
            lock,
            request,
            context,
            receipt,
            cargo_limits,
            cargo_allocation,
        )?;
        self.decode_parsed_in(
            paths,
            record,
            Parsed {
                metadata,
                graph,
                selected,
                internal,
                pending,
            },
            budget,
        )
    }

    /// Interpret the record a build seals from the Cargo documents it bound
    /// from the same bytes, so its replay right after parses neither again
    /// (#1836). The record's other sections are decoded as any replay's.
    pub(in crate::cargo) fn decode_built_in(
        &self,
        paths: &layout::Paths,
        record: &stored::Record<'_>,
        parsed: Parsed,
        budget: &mut stored::Budget<'_>,
    ) -> stored::Result<Arc<Decoded>> {
        budget.check()?;
        self.decode_parsed_in(paths, record, parsed, budget)
    }

    /// Decode the record's build, namespace and input sections against its
    /// bound Cargo documents.
    fn decode_parsed_in(
        &self,
        paths: &layout::Paths,
        record: &stored::Record<'_>,
        parsed: Parsed,
        budget: &mut stored::Budget<'_>,
    ) -> stored::Result<Arc<Decoded>> {
        let Parsed {
            metadata,
            graph,
            selected,
            internal,
            pending,
        } = parsed;
        let retained = build::Retained::decode(
            record.build(),
            &graph,
            paths.final_form().build(),
            &self.target,
            budget,
        )?;
        let namespace = record.decode_namespace(budget)?.map(Arc::new);
        let stored = stored::Stored::decode(record.input(), budget)?;
        Ok(Arc::new(Decoded {
            metadata,
            graph,
            selected,
            internal,
            retained,
            namespace,
            stored,
            pending,
        }))
    }

    /// Run every current observation over the bundle and its inputs against one
    /// decoded interpretation, then join the exact universe completion. `cargo_home`
    /// borrows the owner's already-acquired normalized home for this Request's
    /// captured environment; replay does not acquire ambient path authority.
    /// The comparison of the recorded inputs and its completion are taken from
    /// this process's last replay of the same interpretation under the same
    /// request while every stamp that replay took still holds (`Kept`); else
    /// the inputs are compared, and `view` says what this replay did.
    #[allow(
        clippy::too_many_arguments,
        reason = "one replay binds the complete owner inputs and original allocation accounts"
    )]
    fn replay_decoded_in(
        &self,
        cached: &cargo::CachedToolchain,
        paths: &layout::Paths,
        cargo_home: Option<&Path>,
        decoded: Arc<Decoded>,
        view: View,
        request: cargo::Digest,
        inventory: super::bundle::Inventory<'_>,
        limits: &Limits,
        budget: &mut stored::Budget<'_>,
    ) -> stored::Result<Replay> {
        budget.check()?;
        let manifest =
            budget.digest(cargo::render_manifest(self, Some(&decoded.internal))?.as_bytes())?;
        let source = budget.digest(cargo::HARNESS_SOURCE.as_bytes())?;
        let against = Against {
            decoded: Arc::clone(&decoded),
            request,
            attestation: cached.attestation.content().digest().as_bytes(),
            cargo_home: cargo_home.map(Path::to_path_buf),
            project_root: self.project_root.clone(),
            manifest,
            source,
        };
        let kept = Kept::current(&against);
        let watched = match &kept {
            Some(kept) => kept.watched.clone(),
            None => super::watched::Watched::observe(
                paths.root(),
                decoded.stored.entries(),
                decoded.namespace.as_deref(),
            ),
        };
        let toolchain = cached.attestation.value().proof();
        let capture = Plan::replay_with_namespace(
            paths,
            &decoded.graph,
            toolchain,
            &self.target,
            decoded.retained.build(),
            &decoded.metadata,
            limits,
            budget.graph(),
            decoded.namespace.clone(),
            Some(inventory),
            &self.demand,
        )
        .map_err(|error| match error {
            crate::Error::Node { source, .. } => stored::Error::Bundle(*source),
            error => stored::Error::Input(error),
        })?;
        if let Some(kept) = kept {
            count(View::Kept);
            let replay = Replay {
                decoded,
                capture,
                completion: kept.completion.clone(),
                input: kept.input.completed(budget.graph()),
                watched,
            };
            kept.keep();
            return Ok(replay);
        }
        count(view);
        let final_form = paths.final_form();
        let compared = decoded.stored.require_inputs(
            &self.config,
            &self.inputs,
            &capture,
            [
                final_form.manifest(),
                final_form.source(),
                final_form.lock(),
            ],
            toolchain,
            &decoded.internal,
            &self.target,
            &self.profile,
            &decoded.retained,
            &decoded.metadata,
            self.config.env(),
            cargo_home,
            &self.project_root,
            &decoded.graph,
            final_form.build(),
            budget,
        )?;
        let (completion, input) = decoded.pending.complete_replayed(
            manifest,
            source,
            decoded.metadata.digest(),
            decoded.graph.digest(),
            compared,
            limits,
        )?;
        Kept::keep(Kept {
            against,
            watched: watched.clone(),
            completion: completion.clone(),
            input: input.kept(),
        });
        Ok(Replay {
            decoded,
            capture,
            completion,
            input,
            watched,
        })
    }
}

/// Everything a replay's comparison of the recorded inputs is a function of
/// besides the files its stamps cover: the interpretation it compared
/// (this process's own, by identity), the request, the toolchain, Cargo's
/// home, the project and the harness it rendered.
struct Against {
    decoded: Arc<Decoded>,
    request: cargo::Digest,
    attestation: [u8; 32],
    cargo_home: Option<PathBuf>,
    project_root: PathBuf,
    manifest: cargo::Digest,
    source: cargo::Digest,
}

impl PartialEq for Against {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.decoded, &other.decoded)
            && self.request == other.request
            && self.attestation == other.attestation
            && self.cargo_home == other.cargo_home
            && self.project_root == other.project_root
            && self.manifest == other.manifest
            && self.source == other.source
    }
}

/// One replay's comparison of the recorded inputs and its completion, with
/// the stamps it took before it read them (#1759). Each check of a running
/// owner opens a new graph, whose replay compared every input again: a
/// later replay against the same takes this one's result while every stamp
/// holds. The stamps are each watched path's device, inode, length, mode,
/// and modification and status-change times, of the bundle's whole tree
/// (the harness's `Cargo.toml` and `Cargo.lock` among them) and of every
/// path the record names: each workspace member's manifest, the project's
/// lock, every source file and directory, and the compiler namespace. An
/// edit to any of them moves its status-change time, which user space
/// cannot set back, so the next replay compares in full.
struct Kept {
    against: Against,
    watched: super::watched::Watched,
    completion: universe::Completion,
    input: stored::Kept,
}

/// The comparison this process kept last. One is enough: it names the
/// interpretation the owner's stage holds, the only one a later replay
/// takes from it.
#[cfg(not(test))]
static KEPT: std::sync::Mutex<Option<Kept>> = std::sync::Mutex::new(None);

#[cfg(test)]
thread_local! {
    /// A test thread keeps its comparison as a process would.
    static KEPT: std::cell::RefCell<Option<Kept>> = const { std::cell::RefCell::new(None) };
}

/// Run `with` over this process's kept comparison.
fn kept<T>(with: impl FnOnce(&mut Option<Kept>) -> T) -> Option<T> {
    #[cfg(test)]
    return Some(KEPT.with_borrow_mut(with));
    #[cfg(not(test))]
    KEPT.lock().ok().map(|mut kept| with(&mut kept))
}

impl Kept {
    /// The kept comparison made `against` the same, while its stamps hold.
    fn current(against: &Against) -> Option<Self> {
        let found = kept(|kept| kept.take_if(|kept| kept.against == *against))??;
        found.watched.unchanged().then_some(found)
    }

    /// Keep `self` in place of the comparison kept before.
    fn keep(self) {
        kept(|kept| *kept = Some(self));
    }
}
