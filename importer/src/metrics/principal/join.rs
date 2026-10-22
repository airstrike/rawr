//! Phase-local joins of authenticated process observations to raw parent endpoints.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::{Commitment, Consumed, Event, Evidence, Identity, Outcome, Target};
use crate::metrics::cell::project::{Manifest, ManifestMember};

#[derive(Debug, Serialize)]
pub struct Joined {
    pub processes: Box<[Process]>,
    pub principal_members_after: usize,
    pub ledger_replacements: super::ledger::join::Joined,
    pub pin_histories: super::pin::join::Joined,
    pub retirements: super::retirement::join::Joined,
    pub creations: super::creation::join::Joined,
    pub recoveries: super::recovery::join::Joined,
    pub bundles: super::bundle::join::Joined,
    pub temporaries: super::temporary::join::Joined,
}

/// One successful replacement joined to its own repair and admitted bundle owner.
#[derive(Debug, Serialize)]
pub struct Repaired {
    history: Joined,
    old: crate::metrics::pipeline::target::Target,
    new: crate::metrics::pipeline::target::Target,
    repair_owner: u64,
    retirement: u64,
    replacement_owner: u64,
    old_allocation: super::pin::Reference,
}

#[derive(Debug, Serialize)]
pub struct Process {
    pub pid: u32,
    pub attempts: usize,
    pub renamed: usize,
    pub existing: usize,
    pub admissions: usize,
    pub consumed: usize,
}

struct Attempt<'a> {
    identity: &'a Identity,
    sealed: Option<(Commitment, Option<Commitment>)>,
    renamed: bool,
    outcome: Option<Outcome>,
    renamed_at: Option<usize>,
    finished_at: Option<usize>,
}

struct Admission {
    root: [u8; 32],
    publisher: [u8; 32],
    count: u64,
    next: u64,
}

/// This is the existing two-process cold cell only. Neither child is the other's
/// consumer, and process-local order does not imply an order between processes.
pub fn cold_pair(
    processes: [(u32, &Evidence); 2],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
) -> Result<Joined, &'static str> {
    if !before.members.is_empty()
        || processes[0].0 == processes[1].0
        || processes.iter().any(|(pid, _)| *pid == 0)
    {
        return Err("principal join requires two distinct cold-cell process owners");
    }
    join(&processes, root, before, after, false)
}

/// Join one authenticated setup child without inventing a second cold owner.
pub fn setup(
    process: (u32, &Evidence),
    root: &Path,
    before: &Manifest,
    after: &Manifest,
) -> Result<Joined, &'static str> {
    if process.0 == 0 || !before.members.is_empty() || !before.directories.is_empty() {
        return Err("principal setup requires one cold-cell process owner");
    }
    join(&[process], root, before, after, false)
}

/// Join one repair phase, including the exact injected and replacement targets.
/// Its PID and local tokens are independent of every earlier setup phase.
pub fn repair(
    process: (u32, &Evidence),
    root: &Path,
    before: &Manifest,
    after: &Manifest,
    targets: (
        crate::metrics::pipeline::target::Target,
        crate::metrics::pipeline::target::Target,
    ),
    setup: &Joined,
) -> Result<Repaired, &'static str> {
    if process.0 == 0 || targets.0 == targets.1 || setup.processes.len() != 1 {
        return Err("principal repair requires one owner and a distinct replacement");
    }
    let history = join(&[process], root, before, after, true)?;
    let old = targets.0.bundle_relative();
    let new = targets.1.bundle_relative();
    let mut initial = setup.bundles.owners.iter().filter(|owner| {
        owner.target.relative() == old
            && owner.pid == setup.processes[0].pid
            && matches!(owner.origin, super::bundle::Origin::Allocation { .. })
            && owner.ended
            && owner.stages.contains(&super::bundle::Stage::Retained)
    });
    let old_allocation = initial
        .next()
        .and_then(|owner| owner.record.as_ref())
        .filter(|record| record.consumed)
        .ok_or("setup omitted the consumed old bundle allocation")?
        .allocation;
    if initial.next().is_some() {
        return Err("setup repeats the selected old bundle allocation");
    }
    let mut owners = history.bundles.owners.iter().filter(|owner| {
        owner.pid == process.0
            && owner.target.relative() == old
            && owner.origin == super::bundle::Origin::Repair
    });
    let owner = owners
        .next()
        .ok_or("exact corrupt bundle repair owner absent")?;
    if owners.next().is_some() || !owner.ended {
        return Err("corrupt bundle repair owner is repeated or unfinished");
    }
    let mut retirements = history.retirements.attempts.iter().filter(|attempt| {
        attempt.pid == process.0
            && attempt.target.relative().as_ref() == Some(&old)
            && attempt.cause == super::retirement::Cause::BundleRepair
    });
    let retirement = retirements
        .next()
        .ok_or("exact corrupt bundle retirement absent")?;
    if retirements.next().is_some()
        || Some(retirement.publication) != owner.publication
        || retirement.renamed.is_none()
        || retirement.removal_started.is_none()
        || retirement.removal_finished.is_none()
        || retirement.removal != Some(true)
        || retirement.finished != Some(true)
        || retirement.facts.allocation != Some(old_allocation)
    {
        return Err("corrupt bundle retirement is repeated, incomplete or unassociated");
    }
    let temporary = retirement.temporary.ok_or("repair relocation absent")?;
    if !history.bundles.operations.iter().any(|operation| {
        operation.pid == process.0
            && operation.owner == owner.token
            && operation.publication == owner.publication
            && operation.target == super::bundle::Target::Temporary(temporary)
            && operation.action
                == (super::bundle::Action::Remove {
                    cleanup: super::bundle::Cleanup::Checked,
                })
            && operation.outcome == Some(super::bundle::Outcome::Succeeded)
    }) {
        return Err("corrupt bundle repair omitted successful owned cleanup");
    }
    let mut replacements = history.bundles.owners.iter().filter(|candidate| {
        candidate.pid == process.0
            && candidate.target.relative() == new
            && matches!(candidate.origin, super::bundle::Origin::Allocation { .. })
    });
    let replacement = replacements
        .next()
        .ok_or("exact replacement bundle owner absent")?;
    if replacements.next().is_some()
        || !replacement.ended
        || !replacement.stages.contains(&super::bundle::Stage::Retained)
        || replacement
            .record
            .as_ref()
            .is_none_or(|record| !record.consumed)
    {
        return Err("replacement bundle is repeated, unretained or unconsumed");
    }
    let (repair_owner, retirement, replacement_owner) =
        (owner.token, retirement.token, replacement.token);
    Ok(Repaired {
        history,
        old: targets.0,
        new: targets.1,
        repair_owner,
        retirement,
        replacement_owner,
        old_allocation,
    })
}

fn join(
    processes: &[(u32, &Evidence)],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
    retain_initial: bool,
) -> Result<Joined, &'static str> {
    let ledger_replacements = super::ledger::join::join(processes, root, before, after)?;
    let pin_histories = super::pin::join::join(processes, root, before, after)?;
    let retirements = super::retirement::join::join(processes, root, before, after)?;
    let recoveries = super::recovery::join::join(
        processes,
        root,
        before,
        after,
        (&retirements, &pin_histories),
    )?;
    let creations = super::creation::join::join(processes, root, before, after, &retirements)?;
    let bundles = super::bundle::join::join(processes, root, before, after, &retirements)?;
    let root_path = root;
    let root = super::path_digest(root);
    let mut written = BTreeMap::new();
    let mut results = Vec::new();
    for (pid, evidence) in processes.iter().copied() {
        if evidence.fault.is_some() || !evidence.cell_transport {
            return Err("principal observation is incomplete");
        }
        let mut attempts = BTreeMap::<u64, Attempt<'_>>::new();
        let mut admissions = BTreeMap::<u64, Admission>::new();
        let mut consumed = Vec::<([u8; 32], &Consumed)>::new();
        for (event_index, event) in evidence.events.as_slice().iter().enumerate() {
            match event {
                Event::Temporary { .. }
                | Event::Bundle { .. }
                | Event::Ledger { .. }
                | Event::Pin { .. }
                | Event::Retirement { .. }
                | Event::Creation { .. }
                | Event::Recovery { .. } => {}
                Event::Started { attempt, identity } => {
                    if *attempt == 0
                        || identity.root != root
                        || identity.relative().is_none()
                        || attempts
                            .insert(
                                *attempt,
                                Attempt {
                                    identity,
                                    sealed: None,
                                    renamed: false,
                                    outcome: None,
                                    renamed_at: None,
                                    finished_at: None,
                                },
                            )
                            .is_some()
                    {
                        return Err("principal attempt identity or actual target differs");
                    }
                }
                Event::Sealed {
                    attempt,
                    member,
                    allocation,
                } => {
                    let state = attempts
                        .get_mut(attempt)
                        .ok_or("principal seal without attempt")?;
                    if state.sealed.is_some()
                        || state.renamed
                        || state.outcome.is_some()
                        || allocation.is_some()
                            == matches!(state.identity.target, Target::Index { .. })
                    {
                        return Err("principal seal order or allocation shape differs");
                    }
                    state.sealed = Some((*member, *allocation));
                }
                Event::Renamed { attempt } => {
                    let state = attempts
                        .get_mut(attempt)
                        .ok_or("principal rename without attempt")?;
                    if state.sealed.is_none() || state.renamed || state.outcome.is_some() {
                        return Err("principal rename order differs");
                    }
                    state.renamed = true;
                    state.renamed_at = Some(event_index);
                }
                Event::Finished { attempt, outcome } => {
                    let state = attempts
                        .get_mut(attempt)
                        .ok_or("principal result without attempt")?;
                    if state.outcome.replace(*outcome).is_some()
                        || (*outcome == Outcome::Published && !state.renamed)
                        || (*outcome == Outcome::Existing && state.renamed)
                    {
                        return Err("principal terminal result differs from rename history");
                    }
                    state.finished_at = Some(event_index);
                }
                Event::Admitted {
                    admission,
                    root: observed_root,
                    publisher,
                    entries,
                    ..
                } => {
                    if *admission == 0
                        || *observed_root != root
                        || admissions
                            .insert(
                                *admission,
                                Admission {
                                    root: *observed_root,
                                    publisher: *publisher,
                                    count: *entries,
                                    next: 0,
                                },
                            )
                            .is_some()
                    {
                        return Err("consumer admission identity differs");
                    }
                }
                Event::Consumed {
                    admission,
                    index,
                    entry,
                } => {
                    let state = admissions
                        .get_mut(admission)
                        .ok_or("consumed node without its own admission")?;
                    if state.root != root || state.next != *index || state.next >= state.count {
                        return Err("consumer read set is reordered or incomplete");
                    }
                    state.next += 1;
                    consumed.push((state.publisher, entry));
                }
            }
        }
        if admissions.is_empty() || admissions.values().any(|state| state.next != state.count) {
            return Err("successful own-process consumer read set is absent or incomplete");
        }
        let mut result = Process {
            pid,
            attempts: attempts.len(),
            renamed: 0,
            existing: 0,
            admissions: admissions.len(),
            consumed: consumed.len(),
        };
        for attempt in attempts.values() {
            let identity = attempt.identity;
            let Some(outcome) = attempt.outcome else {
                return Err("principal attempt has no terminal result");
            };
            if !consumed.iter().any(|(publisher, entry)| {
                *publisher == identity.publisher
                    && entry.kind == identity.kind
                    && entry.schema == identity.schema
                    && entry.action == identity.action
                    && match identity.target {
                        Target::Payload { content, .. } | Target::Envelope { content, .. } => {
                            entry.content == content
                        }
                        Target::Index { .. } => true,
                    }
            }) {
                return Err("principal target was not consumed by its own publishing process");
            }
            let target = identity.relative().ok_or("principal target namespace")?;
            if attempt.renamed {
                // A later sync/cleanup error cannot erase this actual durable rename.
                result.renamed += 1;
                let (member, allocation) = attempt
                    .sealed
                    .ok_or("renamed principal omitted commitments")?;
                for (name, commitment) in [
                    (identity.member(), Some(member)),
                    ("allocation", allocation),
                ] {
                    let Some(commitment) = commitment else {
                        continue;
                    };
                    let path = target.join(name).as_os_str().as_encoded_bytes().to_vec();
                    if retirements.removed_after(
                        pid,
                        attempt
                            .renamed_at
                            .ok_or("principal rename position absent")?,
                        &path,
                    ) {
                        continue;
                    }
                    if !after
                        .members
                        .iter()
                        .any(|member| member.path.as_ref() == path.as_slice())
                        && (removal_attempted_after(
                            &retirements,
                            pid,
                            attempt
                                .renamed_at
                                .ok_or("principal rename position absent")?,
                            &path,
                        ) || super::temporary::join::other_mutation(
                            pid,
                            &target,
                            &retirements,
                            &pin_histories,
                        ))
                    {
                        continue;
                    }
                    require_member(after, &path, commitment)?;
                    if written.insert(path, pid).is_some() {
                        return Err("two principal renames claim the same cold target");
                    }
                }
            } else if outcome == Outcome::Existing {
                result.existing += 1;
                // Sealed allocation bytes belong to the losing candidate. They
                // are deliberately not claimed as the existing winner's record.
                let (member, _) = attempt
                    .sealed
                    .ok_or("existing principal omitted its member commitment")?;
                let path = target.join(identity.member());
                if !retirements.removed_after(
                    pid,
                    attempt
                        .finished_at
                        .ok_or("principal outcome position absent")?,
                    path.as_os_str().as_encoded_bytes(),
                ) {
                    let path_bytes = path.as_os_str().as_encoded_bytes();
                    if after
                        .members
                        .iter()
                        .any(|member| member.path.as_ref() == path_bytes)
                        || !(removal_attempted_after(
                            &retirements,
                            pid,
                            attempt
                                .finished_at
                                .ok_or("principal outcome position absent")?,
                            path_bytes,
                        ) || super::temporary::join::other_mutation(
                            pid,
                            &target,
                            &retirements,
                            &pin_histories,
                        ))
                    {
                        require_member(after, path_bytes, member)?;
                    }
                }
            }
        }
        results.push(result);
    }
    if bundles.owners.iter().any(|owner| {
        owner.stages.contains(&super::bundle::Stage::Retained)
            && owner.record.as_ref().is_none_or(|record| !record.consumed)
    }) {
        return Err("retained bundle was not consumed by its own publishing process");
    }
    let mut members = 0;
    for member in after
        .members
        .iter()
        .filter(|member| principal(&member.path))
    {
        members += 1;
        if !written.contains_key(member.path.as_ref())
            && !(retain_initial && before.members.iter().any(|initial| initial == member))
        {
            return Err(
                "principal member has neither unchanged initial facts nor an observed publication",
            );
        }
    }
    let mut temporaries =
        super::temporary::join::join(processes, root_path, before, after, &recoveries)?;
    temporaries.compose(before, after, &retirements, &pin_histories, &recoveries)?;
    let processes = results.into_boxed_slice();
    Ok(Joined {
        processes,
        principal_members_after: members,
        ledger_replacements,
        pin_histories,
        retirements,
        creations,
        recoveries,
        bundles,
        temporaries,
    })
}

fn principal(path: &[u8]) -> bool {
    [b"actions/".as_slice(), b"content/", b"indexes/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn require_member(
    after: &Manifest,
    path: &[u8],
    commitment: Commitment,
) -> Result<(), &'static str> {
    let member: &ManifestMember = after
        .members
        .iter()
        .find(|member| member.path.as_ref() == path)
        .ok_or("principal member absent from parent after-manifest")?;
    if member.bytes != commitment.bytes || member.digest != commitment.sha256 {
        return Err("principal bytes differ from the actual publication commitment");
    }
    Ok(())
}

// Unlike removed_after, a reached recursive removal attempt explains missing
// descendants only; it grants no byte identity or successful-removal claim.
fn removal_attempted_after(
    retirements: &super::retirement::join::Joined,
    pid: u32,
    event: usize,
    path: &[u8],
) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    let path = Path::new(std::ffi::OsStr::from_bytes(path));
    retirements.attempts.iter().any(|attempt| {
        attempt.pid == pid
            && attempt.removal_started.is_some_and(|at| at > event)
            && attempt
                .target
                .relative()
                .is_some_and(|target| path.starts_with(target))
    })
}
