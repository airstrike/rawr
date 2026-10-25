//! Deterministic miss-side reclamation of whole released DAG closures.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::accounting::{self, Allocation, Class, Reference, Registered};
use super::{bundle, entry, index, lease, pins, recovery};
use crate::node::codec::{Envelope, MAX_ENVELOPE_BYTES};
use crate::node::{ActionKey, Digest, Error, Kind};

#[derive(Clone, Eq, PartialEq)]
struct Candidate {
    project: Digest,
    runner: Reference,
    action: Registered,
    physical: bool,
    allocations: Box<[Registered]>,
}

#[derive(Clone, Eq, PartialEq)]
struct Interrupted {
    allocations: Box<[Registered]>,
}

pub(super) fn recover_one(
    store: &Path,
    requesting: Digest,
    local_only: bool,
    keep_roots: bool,
    budget: &mut recovery::Budget,
) -> Result<bool, Error> {
    if recover_interrupted(store, budget)? {
        return Ok(true);
    }
    if recover_garbage(store, budget)? {
        return Ok(true);
    }
    let selected = lease::with_publication(store, |publication| {
        current_candidates(
            publication,
            store,
            requesting,
            local_only,
            keep_roots,
            budget,
        )
    })?;
    for candidate in selected {
        let mut evictions = Vec::new();
        let mut blocked = false;
        for registered in candidate
            .allocations
            .iter()
            .filter(|registered| registered.allocation().class == Class::Bundle)
        {
            let Some(eviction) = bundle::prepare_eviction(store, registered.allocation())? else {
                blocked = true;
                break;
            };
            evictions.push((eviction, *registered));
        }
        if blocked {
            continue;
        }
        let recovered = lease::with_publication(store, |publication| {
            let registrations = accounting::registrations_recovery(store, budget)?;
            let snapshot = pins::snapshot(publication, store, &registrations, budget)?;
            if !candidates(
                &snapshot,
                &registrations,
                requesting,
                local_only,
                keep_roots,
            )?
            .contains(&candidate)
            {
                return Ok(false);
            }
            let physical = pins::retire_project_runner(
                publication,
                store,
                snapshot,
                candidate.project,
                candidate.runner,
                &registrations,
                budget,
            )?;
            if physical != candidate.physical {
                return Err(Error::State(
                    "typed store runner reachability changed during eviction",
                ));
            }
            if !physical {
                if !candidate.allocations.is_empty() || !evictions.is_empty() {
                    return Err(Error::State(
                        "logical runner retirement retained a physical deletion plan",
                    ));
                }
                return Ok(true);
            }
            if candidate.allocations.is_empty() {
                return Err(Error::State(
                    "physical runner retirement lost its released closure",
                ));
            }
            accounting::begin_eviction(publication, store, &candidate.allocations, budget)?;
            for (eviction, registered) in evictions {
                budget.bundle()?;
                budget.deletion(registered.allocation().charge)?;
                eviction.retire(publication, store)?;
            }
            retire_allocations(publication, store, &candidate.allocations, false, budget)?;
            accounting::finish_eviction(publication, store, budget)?;
            Ok(true)
        })?;
        if recovered {
            return Ok(true);
        }
    }
    // Size targets never reach unrooted non-runner nodes this process may hold.
    if keep_roots {
        return Ok(false);
    }
    recover_unrooted(store, budget)
}

/// Repair only a rejected action's unrooted bundle; unrelated allocations remain
/// subject to ordinary physical validation during every control mutation.
pub(super) fn recover_rejected(
    store: &Path,
    kind: Kind,
    schema: u32,
    actions: &[Option<(ActionKey, crate::node::Corruption)>],
    parents: &[crate::node::read_set::Entry],
    budget: &mut recovery::Budget,
) -> Result<(), Error> {
    for (action, proof) in actions.iter().flatten().copied() {
        let selected = lease::with_publication(store, |publication| {
            recovery::recover_temporaries(publication, store, budget)?;
            let registrations = accounting::registrations_recovery(store, budget)?;
            let snapshot = pins::snapshot(publication, store, &registrations, budget)?;
            let parents = accounting::references_recovery(store, parents, budget)?;
            rejected_closure(&snapshot, &registrations, kind, schema, action, &parents)
        })?;
        let Some(selected) = selected else { continue };
        let allocation = selected
            .iter()
            .find(|entry| entry.allocation().class == Class::Bundle)
            .ok_or(Error::State("rejected bundle closure omitted its bundle"))?
            .allocation();
        if allocation != proof.allocation() {
            return Err(Error::CleanupRequired(
                "rejected candidate changed its authenticated allocation",
            ));
        }
        let Some(eviction) = bundle::prepare_eviction(store, allocation)? else {
            continue;
        };
        if !eviction.matches_reference(store, proof.reference()) {
            return Err(Error::CleanupRequired(
                "rejected candidate changed its authenticated bundle root",
            ));
        }
        lease::with_publication(store, |publication| {
            let registrations = accounting::registrations_recovery(store, budget)?;
            let snapshot = pins::snapshot(publication, store, &registrations, budget)?;
            let parents = accounting::references_recovery(store, parents, budget)?;
            if rejected_closure(&snapshot, &registrations, kind, schema, action, &parents)?
                .as_deref()
                != Some(selected.as_ref())
            {
                return Ok(());
            }
            let repair = eviction.into_repair(allocation, budget)?;
            accounting::begin_repair_eviction(publication, store, &selected, &repair, budget)?;
            repair.retire(publication, store, budget)?;
            retire_allocations(publication, store, &selected, false, budget)?;
            accounting::finish_eviction(publication, store, budget)
        })?;
    }
    Ok(())
}

fn rejected_closure(
    snapshot: &pins::lock::Snapshot,
    registrations: &[Registered],
    kind: Kind,
    schema: u32,
    action: ActionKey,
    parents: &[Reference],
) -> Result<Option<Box<[Registered]>>, Error> {
    let mut rooted = BTreeSet::new();
    rooted.extend(parents.iter().copied());
    rooted.extend(snapshot.protected().iter().copied());
    for pin in snapshot.active() {
        rooted.extend(pin.references().iter().copied());
    }
    for pin in snapshot.stale() {
        rooted.extend(pin.references().iter().copied());
    }
    let Some(action) = registrations.iter().copied().find(|entry| {
        let allocation = entry.allocation();
        allocation.class == Class::Action
            && allocation.kind == kind
            && allocation.schema == schema
            && allocation.key == Digest::from_bytes(action.as_bytes())
    }) else {
        return Ok(None);
    };
    if rooted.contains(&action.reference()) {
        return Ok(None);
    }
    let Some(bundle) = registrations
        .iter()
        .copied()
        .find(|entry| action_owns_bundle(action, *entry))
    else {
        return Ok(None);
    };
    if rooted.contains(&bundle.reference()) {
        return Ok(None);
    }
    let mut released = BTreeSet::new();
    released.insert(action.reference());
    released.insert(bundle.reference());
    if let Some(content) = registrations.iter().copied().find(|entry| {
        let allocation = entry.allocation();
        allocation.class == Class::Content
            && allocation.kind == kind
            && allocation.schema == schema
            && entry.content() == action.content()
    }) && !rooted.contains(&content.reference())
    {
        released.insert(content.reference());
    }
    deletion_plan(&released, &rooted, registrations).map(Some)
}

pub(super) fn recover_pending(store: &Path, budget: &mut recovery::Budget) -> Result<(), Error> {
    recover_interrupted(store, budget).map(|_| ())
}

fn recover_garbage(store: &Path, budget: &mut recovery::Budget) -> Result<bool, Error> {
    lease::with_publication(store, |publication| {
        let Some((path, charge)) = accounting::garbage_recovery(store, budget)? else {
            return Ok(false);
        };
        budget.deletion(charge)?;
        let admitted = super::control::Admitted::seal(
            publication,
            store,
            super::control::Plan::retirement(&path),
        )?;
        let temporary = entry::stage_admitted_retirement(admitted, publication, store, &path)?;
        fs::remove_dir_all(&temporary).map_err(|source| Error::Io {
            operation: "remove typed store legacy or unregistered allocation",
            path: temporary.clone(),
            source,
        })?;
        entry::sync_directory(temporary.parent().ok_or(Error::State(
            "typed store garbage has no temporary directory",
        ))?)?;
        Ok(true)
    })
}

fn recover_unrooted(store: &Path, budget: &mut recovery::Budget) -> Result<bool, Error> {
    let selected = lease::with_publication(store, |publication| {
        let registrations = accounting::registrations_recovery(store, budget)?;
        let snapshot = pins::snapshot(publication, store, &registrations, budget)?;
        unrooted(&snapshot, &registrations)
    })?;
    let Some(selected) = selected else {
        return Ok(false);
    };
    let mut evictions = Vec::new();
    for registered in selected
        .iter()
        .filter(|registered| registered.allocation().class == Class::Bundle)
    {
        let Some(eviction) = bundle::prepare_eviction(store, registered.allocation())? else {
            return Ok(false);
        };
        evictions.push((eviction, *registered));
    }
    lease::with_publication(store, |publication| {
        let registrations = accounting::registrations_recovery(store, budget)?;
        let snapshot = pins::snapshot(publication, store, &registrations, budget)?;
        if unrooted(&snapshot, &registrations)?.as_deref() != Some(selected.as_ref()) {
            return Ok(false);
        }
        accounting::begin_eviction(publication, store, &selected, budget)?;
        for (eviction, registered) in evictions {
            budget.bundle()?;
            budget.deletion(registered.allocation().charge)?;
            eviction.retire(publication, store)?;
        }
        retire_allocations(publication, store, &selected, false, budget)?;
        accounting::finish_eviction(publication, store, budget)?;
        Ok(true)
    })
}

/// Resolution bundles a project keeps when no root holds them: the
/// current one and a few an undo or redo returns to.
pub(super) const RESOLUTIONS_KEPT: usize = 4;

/// Retire the requesting project's oldest unrooted resolution unit (action,
/// its bundle, its content when nothing else names it) while it has more than
/// `keep`. Every edit publishes one; nothing else ever retires them, and each
/// holds the build outputs it relocated.
pub(super) fn recover_resolution_excess(
    store: &Path,
    requesting: Digest,
    kind: Kind,
    keep: usize,
    budget: &mut recovery::Budget,
) -> Result<bool, Error> {
    let selected = lease::with_publication(store, |publication| {
        let registrations = accounting::registrations_recovery(store, budget)?;
        let snapshot = pins::snapshot(publication, store, &registrations, budget)?;
        resolution_excess(&snapshot, &registrations, requesting, kind, keep)
    })?;
    let Some(selected) = selected else {
        return Ok(false);
    };
    let mut evictions = Vec::new();
    for registered in selected
        .iter()
        .filter(|registered| registered.allocation().class == Class::Bundle)
    {
        let Some(eviction) = bundle::prepare_eviction(store, registered.allocation())? else {
            return Ok(false);
        };
        evictions.push((eviction, *registered));
    }
    lease::with_publication(store, |publication| {
        let registrations = accounting::registrations_recovery(store, budget)?;
        let snapshot = pins::snapshot(publication, store, &registrations, budget)?;
        if resolution_excess(&snapshot, &registrations, requesting, kind, keep)?.as_deref()
            != Some(selected.as_ref())
        {
            return Ok(false);
        }
        accounting::begin_eviction(publication, store, &selected, budget)?;
        for (eviction, registered) in evictions {
            budget.bundle()?;
            budget.deletion(registered.allocation().charge)?;
            eviction.retire(publication, store)?;
        }
        retire_allocations(publication, store, &selected, false, budget)?;
        accounting::finish_eviction(publication, store, budget)?;
        Ok(true)
    })
}

fn resolution_excess(
    snapshot: &pins::lock::Snapshot,
    registrations: &[Registered],
    requesting: Digest,
    kind: Kind,
    keep: usize,
) -> Result<Option<Box<[Registered]>>, Error> {
    let mut rooted = BTreeSet::new();
    rooted.extend(snapshot.protected().iter().copied());
    for root in snapshot.active() {
        rooted.extend(root.references().iter().copied());
    }
    for root in snapshot.stale() {
        rooted.extend(root.references().iter().copied());
    }
    let mut actions = registrations
        .iter()
        .copied()
        .filter(|registered| {
            registered.allocation().class == Class::Action
                && registered.allocation().kind == kind
                && registered.publisher() == requesting
                && !rooted.contains(&registered.reference())
        })
        .collect::<Vec<_>>();
    if actions.len() <= keep {
        return Ok(None);
    }
    actions.sort_unstable_by_key(|registered| (registered.sequence(), registered.allocation().key));
    for action in &actions[..actions.len() - keep] {
        let owned_bundle = registrations
            .iter()
            .copied()
            .find(|candidate| action_owns_bundle(*action, *candidate));
        if owned_bundle.is_some_and(|bundle| rooted.contains(&bundle.reference())) {
            continue;
        }
        let mut released = BTreeSet::new();
        released.insert(action.reference());
        if let Some(bundle) = owned_bundle {
            released.insert(bundle.reference());
        }
        let shared = |content: &Registered| {
            registrations.iter().any(|other| {
                other.reference() != action.reference()
                    && other.allocation().class == Class::Action
                    && other.allocation().kind == content.allocation().kind
                    && other.allocation().schema == content.allocation().schema
                    && other.content() == content.content()
            })
        };
        let content = registrations.iter().copied().find(|candidate| {
            candidate.allocation().class == Class::Content
                && candidate.allocation().kind == action.allocation().kind
                && candidate.allocation().schema == action.allocation().schema
                && candidate.content() == action.content()
        });
        if let Some(content) =
            content.filter(|content| !rooted.contains(&content.reference()) && !shared(content))
        {
            released.insert(content.reference());
        }
        return deletion_plan(&released, &rooted, registrations).map(Some);
    }
    Ok(None)
}

fn unrooted(
    snapshot: &pins::lock::Snapshot,
    registrations: &[Registered],
) -> Result<Option<Box<[Registered]>>, Error> {
    let mut rooted = BTreeSet::new();
    rooted.extend(snapshot.protected().iter().copied());
    for root in snapshot.active() {
        rooted.extend(root.references().iter().copied());
    }
    for root in snapshot.stale() {
        rooted.extend(root.references().iter().copied());
    }

    let mut actions = registrations
        .iter()
        .copied()
        .filter(|registered| {
            registered.allocation().class == Class::Action
                && !rooted.contains(&registered.reference())
        })
        .collect::<Vec<_>>();
    actions.sort_unstable_by_key(|registered| {
        (
            registered.sequence(),
            registered.publisher(),
            registered.allocation().kind,
            registered.allocation().key,
        )
    });
    for action in actions {
        let owned_bundle = registrations
            .iter()
            .copied()
            .find(|candidate| action_owns_bundle(action, *candidate));
        if owned_bundle.is_some_and(|bundle| rooted.contains(&bundle.reference())) {
            continue;
        }
        let mut released = BTreeSet::new();
        released.insert(action.reference());
        if let Some(bundle) = owned_bundle {
            released.insert(bundle.reference());
        }
        let content = registrations.iter().copied().find(|candidate| {
            candidate.allocation().class == Class::Content
                && candidate.allocation().kind == action.allocation().kind
                && candidate.allocation().schema == action.allocation().schema
                && candidate.content() == action.content()
        });
        if let Some(content) = content.filter(|content| !rooted.contains(&content.reference())) {
            released.insert(content.reference());
        }
        return deletion_plan(&released, &rooted, registrations).map(Some);
    }

    let mut contents = registrations
        .iter()
        .copied()
        .filter(|registered| {
            registered.allocation().class == Class::Content
                && !rooted.contains(&registered.reference())
                && !registrations.iter().any(|action| {
                    action.allocation().class == Class::Action
                        && action.allocation().kind == registered.allocation().kind
                        && action.allocation().schema == registered.allocation().schema
                        && action.content() == registered.content()
                })
        })
        .collect::<Vec<_>>();
    contents.sort_unstable_by_key(|registered| {
        (
            registered.sequence(),
            registered.publisher(),
            registered.allocation().kind,
            registered.allocation().key,
        )
    });
    Ok(contents
        .into_iter()
        .next()
        .map(|content| vec![content].into_boxed_slice()))
}

fn recover_interrupted(store: &Path, budget: &mut recovery::Budget) -> Result<bool, Error> {
    let (recovered_temporary, interrupted) = lease::with_publication(store, |publication| {
        let recovered = recovery::recover_temporaries(publication, store, budget)?;
        Ok((recovered, interrupted(store, budget)?))
    })?;
    let Some(interrupted) = interrupted else {
        return Ok(recovered_temporary);
    };
    let mut evictions = Vec::new();
    for registered in interrupted
        .allocations
        .iter()
        .filter(|registered| registered.allocation().class == Class::Bundle)
    {
        if let Some(eviction) = bundle::prepare_recovery_eviction(store, registered.allocation())? {
            evictions.push((eviction, *registered));
        }
    }
    lease::with_publication(store, |publication| {
        if self::interrupted(store, budget)? != Some(interrupted.clone()) {
            return Err(Error::CleanupRequired(
                "an interrupted typed store eviction changed during recovery",
            ));
        }
        for (eviction, registered) in evictions {
            eviction
                .into_repair(registered.allocation(), budget)?
                .retire(publication, store, budget)?;
        }
        retire_allocations(publication, store, &interrupted.allocations, true, budget)?;
        accounting::finish_eviction(publication, store, budget)?;
        Ok(true)
    })
}

fn interrupted(store: &Path, budget: &mut recovery::Budget) -> Result<Option<Interrupted>, Error> {
    let pending = accounting::pending_eviction_recovery(store, budget)?;
    if pending.is_empty() {
        return Ok(None);
    }
    Ok(Some(Interrupted {
        allocations: pending,
    }))
}

#[cfg(test)]
pub(super) fn interrupt_oldest(store: &Path, requesting: Digest) -> Result<(), Error> {
    let mut budget = recovery::Budget::new();
    if recover_interrupted(store, &mut budget)? {
        return Err(Error::State(
            "the eviction interruption fixture found earlier recovery work",
        ));
    }
    let candidate = lease::with_publication(store, |publication| {
        current_candidates(publication, store, requesting, false, false, &mut budget)
    })?
    .into_iter()
    .find(|candidate| candidate.physical)
    .ok_or(Error::State(
        "the eviction interruption fixture found no physical runner root",
    ))?;
    let mut bundles = Vec::new();
    for registered in candidate
        .allocations
        .iter()
        .filter(|registered| registered.allocation().class == Class::Bundle)
    {
        bundles.push(
            bundle::prepare_eviction(store, registered.allocation())?.ok_or(Error::State(
                "the eviction interruption fixture bundle is pinned",
            ))?,
        );
    }
    lease::with_publication(store, |publication| {
        let registrations = accounting::registrations_recovery(store, &mut budget)?;
        let snapshot = pins::snapshot(publication, store, &registrations, &mut budget)?;
        if !candidates(&snapshot, &registrations, requesting, false, false)?.contains(&candidate) {
            return Err(Error::State(
                "the eviction interruption fixture selection changed",
            ));
        }
        if !pins::retire_project_runner(
            publication,
            store,
            snapshot,
            candidate.project,
            candidate.runner,
            &registrations,
            &mut budget,
        )? {
            return Err(Error::State(
                "the eviction interruption fixture changed runner reachability",
            ));
        }
        accounting::begin_eviction(publication, store, &candidate.allocations, &mut budget)
    })
}

#[cfg(test)]
pub(super) fn interrupt_after_staging_action(
    store: &Path,
    requesting: Digest,
) -> Result<PathBuf, Error> {
    interrupt_oldest(store, requesting)?;
    let mut budget = recovery::Budget::new();
    lease::with_publication(store, |publication| {
        let interrupted = interrupted(store, &mut budget)?.ok_or(Error::State(
            "the staged interruption fixture lost its eviction journal",
        ))?;
        let action = interrupted
            .allocations
            .iter()
            .find(|registered| registered.allocation().class == Class::Action)
            .ok_or(Error::State(
                "the staged interruption fixture found no action allocation",
            ))?
            .allocation();
        let target = store
            .join("actions")
            .join(action.kind.directory())
            .join(crate::node::STORAGE_DIRECTORY)
            .join(action.key.directory());
        entry::stage_retirement(publication, store, &target)
    })
}

#[cfg(test)]
pub(super) fn interrupt_after_thawing_bundle_root(
    store: &Path,
    requesting: Digest,
) -> Result<PathBuf, Error> {
    interrupt_oldest(store, requesting)?;
    let mut budget = recovery::Budget::new();
    let interrupted = interrupted(store, &mut budget)?.ok_or(Error::State(
        "the bundle-root interruption fixture lost its eviction journal",
    ))?;
    let registered = interrupted
        .allocations
        .iter()
        .find(|registered| registered.allocation().class == Class::Bundle)
        .ok_or(Error::State(
            "the bundle-root interruption fixture found no bundle allocation",
        ))?;
    let eviction = bundle::prepare_recovery_eviction(store, registered.allocation())?.ok_or(
        Error::State("the bundle-root interruption fixture bundle is pinned"),
    )?;
    lease::with_publication(store, |publication| {
        eviction.interrupt_after_root_thaw(publication, store)
    })
}

fn current_candidates(
    publication: &lease::Publication<'_>,
    store: &Path,
    requesting: Digest,
    local_only: bool,
    keep_roots: bool,
    budget: &mut recovery::Budget,
) -> Result<Vec<Candidate>, Error> {
    let registrations = accounting::registrations_recovery(store, budget)?;
    let snapshot = pins::snapshot(publication, store, &registrations, budget)?;
    candidates(
        &snapshot,
        &registrations,
        requesting,
        local_only,
        keep_roots,
    )
}

fn candidates(
    snapshot: &pins::lock::Snapshot,
    registrations: &[Registered],
    requesting: Digest,
    local_only: bool,
    keep_roots: bool,
) -> Result<Vec<Candidate>, Error> {
    let mut bundles = BTreeMap::new();
    let mut contents = BTreeMap::new();
    for registered in registrations.iter().copied().filter(|registered| {
        let allocation = registered.allocation();
        allocation.class == Class::Bundle && allocation.kind == Kind::Runner
    }) {
        if bundles
            .insert((registered.sequence(), registered.publisher()), registered)
            .is_some()
        {
            return Err(Error::CleanupRequired(
                "a registered runner root duplicated its whole-bundle unit",
            ));
        }
    }
    for registered in registrations.iter().copied() {
        let allocation = registered.allocation();
        let key = (allocation.kind, allocation.schema, registered.content());
        match allocation.class {
            Class::Action => {}
            Class::Content => {
                if contents.insert(key, registered).is_some() {
                    return Err(Error::CleanupRequired(
                        "a registered content allocation resolved ambiguously",
                    ));
                }
            }
            Class::Bundle => {}
        }
    }
    let mut stale_projects = BTreeMap::<Reference, Vec<Digest>>::new();
    for stale in snapshot.stale() {
        for reference in stale.references() {
            stale_projects
                .entry(*reference)
                .or_default()
                .push(stale.project());
        }
    }
    for projects in stale_projects.values_mut() {
        projects.sort_unstable();
        projects.dedup();
    }
    let mut active = BTreeSet::new();
    for root in snapshot.active() {
        for reference in root.references() {
            if reference.is_runner_root() {
                active.insert((root.project(), *reference));
            }
        }
    }
    // Size targets keep each project's Live and LastGood runners even while no
    // process holds them; only `rawr cache clean` releases those too.
    for stale in snapshot.stale().iter().filter(|stale| {
        keep_roots && matches!(stale.role(), pins::Role::Live | pins::Role::LastGood)
    }) {
        for reference in stale.references() {
            if reference.is_runner_root() {
                active.insert((stale.project(), *reference));
            }
        }
    }
    let mut candidates = Vec::new();
    for action in registrations.iter().copied().filter(|registered| {
        let allocation = registered.allocation();
        allocation.class == Class::Action && allocation.kind == Kind::Runner
    }) {
        let runner = action.reference();
        let bundle = *bundles
            .get(&(action.sequence(), action.publisher()))
            .ok_or(Error::CleanupRequired(
                "a registered runner root changed its whole-bundle unit",
            ))?;
        if bundle.content() != action.content()
            || bundle.allocation().key != action.content()
            || bundle.allocation().schema != action.allocation().schema
        {
            return Err(Error::CleanupRequired(
                "a registered runner root changed its bundle content",
            ));
        }
        let content_key = (
            action.allocation().kind,
            action.allocation().schema,
            action.content(),
        );
        let content = *contents.get(&content_key).ok_or(Error::CleanupRequired(
            "a registered runner root lost its content allocation",
        ))?;
        let mut projects = stale_projects.get(&runner).cloned().unwrap_or_default();
        if projects.is_empty() {
            projects.push(action.publisher());
        }
        for project in projects {
            if local_only && project != requesting {
                continue;
            }
            if active.contains(&(project, runner)) {
                continue;
            }
            let mut released = BTreeSet::new();
            let mut remaining = BTreeSet::new();
            remaining.extend(snapshot.protected().iter().copied());
            for stale in snapshot.stale() {
                if stale.project() == project && stale.references().binary_search(&runner).is_ok() {
                    released.extend(stale.references().iter().copied());
                } else {
                    remaining.extend(stale.references().iter().copied());
                }
            }
            // Size targets delete only the whole runner unit: closure members
            // may still back values this process holds without a pin.
            if released.is_empty() || keep_roots {
                released = BTreeSet::from([runner, bundle.reference(), content.reference()]);
            }
            let physical = !remaining.contains(&runner);
            if !local_only && project != requesting && !physical {
                continue;
            }
            let allocations = if physical {
                deletion_plan(&released, &remaining, registrations)?
            } else {
                Box::new([])
            };
            if physical && (!allocations.contains(&action) || !allocations.contains(&bundle)) {
                return Err(Error::CleanupRequired(
                    "a released runner root lost its whole immutable unit",
                ));
            }
            candidates.push(Candidate {
                project,
                runner,
                action,
                physical,
                allocations,
            });
        }
    }
    candidates.sort_unstable_by_key(|candidate| {
        (
            candidate.project != requesting,
            candidate.action.sequence(),
            candidate.project,
            candidate.action.allocation().key,
        )
    });
    candidates.dedup_by_key(|candidate| (candidate.project, candidate.runner));
    Ok(candidates)
}

fn deletion_plan(
    released: &BTreeSet<Reference>,
    remaining: &BTreeSet<Reference>,
    registrations: &[Registered],
) -> Result<Box<[Registered]>, Error> {
    let by_reference = registrations
        .iter()
        .copied()
        .map(|registered| (registered.reference(), registered))
        .collect::<BTreeMap<_, _>>();
    let eligible = released
        .difference(remaining)
        .copied()
        .collect::<BTreeSet<_>>();
    let mut actions = BTreeSet::new();
    let mut selected = Vec::new();
    selected
        .try_reserve_exact(eligible.len())
        .map_err(|_| Error::Allocation)?;
    for reference in &eligible {
        let registered = *by_reference.get(reference).ok_or(Error::CleanupRequired(
            "a released closure allocation left the installation registry",
        ))?;
        if registered.allocation().class == Class::Action {
            actions.insert(*reference);
            selected.push(registered);
        }
    }
    for reference in &actions {
        let action = by_reference[reference];
        if let Some(bundle) = registrations
            .iter()
            .copied()
            .find(|bundle| action_owns_bundle(action, *bundle))
        {
            if !eligible.contains(&bundle.reference()) {
                return Err(Error::CleanupRequired(
                    "a released closure split an action from its immutable bundle",
                ));
            }
            selected.push(bundle);
        }
    }
    for reference in &eligible {
        let registered = by_reference[reference];
        match registered.allocation().class {
            Class::Action => {}
            Class::Bundle => {
                if !selected.contains(&registered) {
                    return Err(Error::CleanupRequired(
                        "a released closure split an immutable bundle from its action",
                    ));
                }
            }
            Class::Content => {
                let retained_action = registrations.iter().any(|action| {
                    action.allocation().class == Class::Action
                        && action.allocation().kind == registered.allocation().kind
                        && action.allocation().schema == registered.allocation().schema
                        && action.content() == registered.content()
                        && !actions.contains(&action.reference())
                });
                if !retained_action {
                    selected.push(registered);
                }
            }
        }
    }
    selected.sort_unstable_by_key(|registered| registered.reference());
    selected.dedup();
    if selected.is_empty() || selected.len() > released.len() {
        return Err(Error::CleanupRequired(
            "a released closure produced an invalid physical deletion plan",
        ));
    }
    Ok(selected.into_boxed_slice())
}

fn action_owns_bundle(action: Registered, bundle: Registered) -> bool {
    action.allocation().class == Class::Action
        && bundle.allocation().class == Class::Bundle
        && action.allocation().kind == bundle.allocation().kind
        && action.allocation().schema == bundle.allocation().schema
        && action.sequence() == bundle.sequence()
        && action.publisher() == bundle.publisher()
        && action.content() == bundle.content()
        && bundle.allocation().key == action.content()
}

fn retire_allocations(
    publication: &lease::Publication<'_>,
    store: &Path,
    allocations: &[Registered],
    allow_missing: bool,
    budget: &mut recovery::Budget,
) -> Result<(), Error> {
    for registered in allocations
        .iter()
        .filter(|registered| registered.allocation().class == Class::Action)
    {
        retire_action(
            publication,
            store,
            registered.allocation(),
            allow_missing,
            budget,
        )?;
    }
    for registered in allocations
        .iter()
        .filter(|registered| registered.allocation().class == Class::Content)
    {
        retire_content(
            publication,
            store,
            registered.allocation(),
            allow_missing,
            budget,
        )?;
    }
    Ok(())
}

fn retire_action(
    publication: &lease::Publication<'_>,
    store: &Path,
    allocation: Allocation,
    allow_missing: bool,
    budget: &mut recovery::Budget,
) -> Result<(), Error> {
    if allocation.class != Class::Action {
        return Err(Error::State("typed store eviction requires one action"));
    }
    let relative = PathBuf::from("actions")
        .join(allocation.kind.directory())
        .join(crate::node::STORAGE_DIRECTORY)
        .join(allocation.key.directory());
    let Some(directory) = entry::existing_directory(store, &relative)? else {
        return if allow_missing {
            index::retire_action(
                publication,
                store,
                allocation.kind,
                ActionKey::from_bytes(allocation.key.as_bytes()),
            )
        } else {
            Err(Error::CleanupRequired(
                "a registered action is missing during eviction",
            ))
        };
    };
    let mut names = allocation_names(&directory, "read typed store action during eviction")?;
    names.sort_unstable();
    if names != ["allocation", "envelope"] {
        return Err(Error::CleanupRequired(
            "a typed store action changed shape during eviction",
        ));
    }
    let envelope_path = directory.join("envelope");
    let bytes = entry::read_file(&envelope_path, MAX_ENVELOPE_BYTES)?;
    let envelope = Envelope::decode(&bytes)?;
    if envelope.kind != allocation.kind
        || envelope.output_schema != allocation.schema
        || Digest::from_bytes(envelope.action.as_bytes()) != allocation.key
    {
        return Err(Error::CleanupRequired(
            "a typed store action envelope changed identity during eviction",
        ));
    }
    let record = accounting::Record::read(&directory)?;
    if record.allocation() != allocation {
        return Err(Error::CleanupRequired(
            "a typed store action allocation changed during eviction",
        ));
    }
    let metadata = fs::metadata(&envelope_path).map_err(|source| Error::Io {
        operation: "inspect typed store action during eviction",
        path: envelope_path,
        source,
    })?;
    record.validate(
        Class::Action,
        allocation.kind,
        allocation.schema,
        allocation.key,
        &bytes,
        &metadata,
    )?;
    budget.deletion(allocation.charge)?;
    let temporary = entry::stage_retirement(publication, store, &directory)?;
    fs::remove_dir_all(&temporary).map_err(|source| Error::Io {
        operation: "remove whole typed store action during eviction",
        path: temporary.clone(),
        source,
    })?;
    entry::sync_directory(temporary.parent().ok_or(Error::State(
        "typed store action has no temporary directory",
    ))?)?;
    index::retire_action(
        publication,
        store,
        allocation.kind,
        ActionKey::from_bytes(allocation.key.as_bytes()),
    )
}

fn retire_content(
    publication: &lease::Publication<'_>,
    store: &Path,
    allocation: Allocation,
    allow_missing: bool,
    budget: &mut recovery::Budget,
) -> Result<(), Error> {
    if allocation.class != Class::Content {
        return Err(Error::State(
            "typed store eviction requires one content allocation",
        ));
    }
    let relative = PathBuf::from("content")
        .join(allocation.kind.directory())
        .join(format!("{:08x}", allocation.schema))
        .join(allocation.key.directory());
    let Some(directory) = entry::existing_directory(store, &relative)? else {
        return if allow_missing {
            Ok(())
        } else {
            Err(Error::CleanupRequired(
                "a registered content allocation is missing during eviction",
            ))
        };
    };
    let mut names = allocation_names(&directory, "read typed store content during eviction")?;
    names.sort_unstable();
    if names != ["allocation", "payload"] {
        return Err(Error::CleanupRequired(
            "a typed store content allocation changed shape during eviction",
        ));
    }
    let payload = directory.join("payload");
    let metadata = fs::symlink_metadata(&payload).map_err(|source| Error::Io {
        operation: "inspect typed store content during eviction",
        path: payload.clone(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(payload));
    }
    let record = accounting::Record::read(&directory)?;
    if record.allocation() != allocation {
        return Err(Error::CleanupRequired(
            "a typed store content allocation changed identity during eviction",
        ));
    }
    budget.deletion(allocation.charge)?;
    let temporary = entry::stage_retirement(publication, store, &directory)?;
    fs::remove_dir_all(&temporary).map_err(|source| Error::Io {
        operation: "remove whole typed store content during eviction",
        path: temporary.clone(),
        source,
    })?;
    entry::sync_directory(temporary.parent().ok_or(Error::State(
        "typed store content has no temporary directory",
    ))?)
}

fn allocation_names(
    directory: &Path,
    operation: &'static str,
) -> Result<Vec<std::ffi::OsString>, Error> {
    let entries = fs::read_dir(directory).map_err(|source| Error::Io {
        operation,
        path: directory.to_path_buf(),
        source,
    })?;
    let mut names = Vec::new();
    names.try_reserve_exact(2).map_err(|_| Error::Allocation)?;
    for entry in entries {
        if names.len() == 2 {
            return Err(Error::CleanupRequired(
                "typed store eviction allocation exceeded its bounded member shape",
            ));
        }
        names.push(
            entry
                .map_err(|source| Error::Io {
                    operation,
                    path: directory.to_path_buf(),
                    source,
                })?
                .file_name(),
        );
    }
    Ok(names)
}

#[cfg(unix)]
fn hard_linked(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    metadata.nlink() != 1
}

#[cfg(not(unix))]
fn hard_linked(_metadata: &fs::Metadata) -> bool {
    false
}
