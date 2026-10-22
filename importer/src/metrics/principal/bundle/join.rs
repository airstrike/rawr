//! Bundle-owned effects joined to visited endpoints, without traversal claims.

use super::{Action, Cleanup, Control, Event, Origin, Outcome, Progress, Stage, Target};
use crate::metrics::cell::project::Manifest;
use crate::metrics::principal::{self, Commitment, Evidence};
use serde::Serialize;
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct Joined {
    pub owners: Vec<Owner>,
    pub operations: Vec<Operation>,
    pub endpoints: Vec<Endpoint>,
}
#[derive(Debug, Serialize)]
pub struct Owner {
    pub pid: u32,
    pub token: u64,
    pub target: Target,
    pub origin: Origin,
    pub publication: Option<u64>,
    pub stages: Vec<Stage>,
    pub record: Option<Record>,
    pub ended: bool,
    #[serde(skip)]
    active: Vec<u64>,
}
#[derive(Debug, Serialize)]
pub struct Record {
    pub publication: u64,
    pub allocation: principal::pin::Reference,
    pub lease: principal::Lease,
    pub publisher: [u8; 32],
    pub geometry: Option<[u8; 32]>,
    pub consumed: bool,
}
#[derive(Debug, Serialize)]
pub struct Operation {
    pub pid: u32,
    pub token: u64,
    pub owner: u64,
    pub publication: Option<u64>,
    pub target: Target,
    pub path: Commitment,
    pub action: Action,
    pub started: usize,
    pub opened: Option<usize>,
    pub written: Option<usize>,
    pub finished: Option<usize>,
    pub outcome: Option<Outcome>,
    /// No endpoint match does not prove absence; intermediate members can vanish.
    pub matched_endpoint: bool,
}
#[derive(Debug, Serialize)]
pub struct Endpoint {
    pub path: Box<[u8]>,
    pub directory: bool,
    pub present: bool,
    pub creations: Vec<(u32, u64)>,
    pub writes: Vec<(u32, u64)>,
    pub partial_writes: Vec<(u32, u64)>,
    pub removals: Vec<(u32, u64)>,
    pub matching_modes: Vec<(u32, u64)>,
}

pub fn pair(
    processes: [(u32, &Evidence); 2],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
    retirements: &principal::retirement::join::Joined,
) -> Result<Joined, &'static str> {
    if processes[0].0 == 0 || processes[1].0 == 0 || processes[0].0 == processes[1].0 {
        return Err("bundle join requires distinct process owners");
    }
    join(&processes, root, before, after, retirements)
}

pub(in crate::metrics::principal) fn join(
    processes: &[(u32, &Evidence)],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
    retirements: &principal::retirement::join::Joined,
) -> Result<Joined, &'static str> {
    let mut joined = Joined {
        owners: Vec::new(),
        operations: Vec::new(),
        endpoints: Vec::new(),
    };
    for (pid, evidence) in processes.iter().copied() {
        if evidence.fault.is_some() || !evidence.cell_transport {
            return Err("bundle observation incomplete");
        }
        let mut owners = BTreeMap::<u64, usize>::new();
        let mut operations = BTreeMap::<u64, usize>::new();
        for (index, event) in evidence.events.as_slice().iter().enumerate() {
            let principal::Event::Bundle { change } = event else {
                continue;
            };
            match *change {
                Event::Owner {
                    owner,
                    root: observed,
                    target,
                    origin,
                    publication,
                } => {
                    if owner == 0
                        || owners.contains_key(&owner)
                        || observed != principal::path_digest(root)
                        || !matches!(target, Target::Bundle { .. })
                        || matches!(origin, Origin::Allocation { .. }) != publication.is_none()
                        || publication == Some(0)
                        || matches!(origin, Origin::Allocation { kind } if crate::node::Kind::from_code(kind).is_none())
                    {
                        return Err("bundle owner identity differs");
                    }
                    owners.insert(owner, joined.owners.len());
                    joined.owners.push(Owner {
                        pid,
                        token: owner,
                        target,
                        origin,
                        publication,
                        stages: Vec::new(),
                        record: None,
                        ended: false,
                        active: Vec::new(),
                    });
                }
                Event::Record {
                    owner,
                    publication,
                    allocation,
                    lease,
                    publisher,
                    geometry,
                } => {
                    let state = owner_mut(&mut joined, &owners, owner)?;
                    let Target::Bundle { execution, .. } = state.target else {
                        return Err("bundle record target");
                    };
                    if !state.active.is_empty()
                        || state.record.is_some()
                        || !(matches!(state.origin, Origin::Allocation { kind } if kind == allocation.kind)
                            && state.stages.last() == Some(&Stage::Sealed)
                            || state.origin == Origin::Planned
                                && state.stages.is_empty()
                                && state.publication == Some(publication))
                        || publication == 0
                        || allocation.class != 3
                        || allocation.allocations != 1
                        || lease != principal::Lease::Execution(execution)
                    {
                        return Err("bundle record identity or order differs");
                    }
                    let consumed = consumed(evidence, allocation, publisher);
                    state.record = Some(Record {
                        publication,
                        allocation,
                        lease,
                        publisher,
                        geometry,
                        consumed,
                    });
                }
                Event::Stage { owner, stage } => {
                    let owner_index = *owners.get(&owner).ok_or("bundle stage has no owner")?;
                    let state = &joined.owners[owner_index];
                    if state.ended
                        || !state.active.is_empty()
                        || !matches!(state.origin, Origin::Allocation { .. })
                        || state.stages.len()
                            != match stage {
                                Stage::Initialized => 0,
                                Stage::Sealed => 1,
                                Stage::Published => 2,
                                Stage::Retained => 3,
                            }
                    {
                        return Err("bundle stage order differs");
                    }
                    let own = joined
                        .operations
                        .iter()
                        .filter(|op| op.pid == pid && op.owner == owner)
                        .collect::<Vec<_>>();
                    if own.iter().any(|op| op.outcome != Some(Outcome::Succeeded)) {
                        return Err("successful bundle stage follows an aborting native operation");
                    }
                    match stage {
                        Stage::Initialized => initialized(state.target, &own)?,
                        Stage::Sealed
                            if !own.iter().any(|op| {
                                matches!(
                                    op.action,
                                    Action::Control {
                                        control: Control::Seal,
                                        ..
                                    }
                                ) && op.outcome == Some(Outcome::Succeeded)
                            }) =>
                        {
                            return Err("sealed bundle omitted seal write")
                        }
                        Stage::Published
                            if state.record.is_none()
                                || !own.iter().any(|op| {
                                    op.action == Action::RenameAllocation
                                        && op.outcome == Some(Outcome::Succeeded)
                                })
                                || !own.iter().any(|op| {
                                    matches!(
                                        op.action,
                                        Action::Freeze {
                                            directory: true,
                                            ..
                                        }
                                    ) && op.path == commitment(&state.target.relative())
                                        && op.outcome == Some(Outcome::Succeeded)
                                })
                                || !own.last().is_some_and(|op| {
                                    op.action == Action::Sync
                                        && op.path == commitment(&state.target.relative())
                                        && op.outcome == Some(Outcome::Succeeded)
                                }) =>
                        {
                            return Err("published bundle omitted allocation rename")
                        }
                        Stage::Retained
                            if own
                                .iter()
                                .any(|op| matches!(op.action, Action::Remove { .. })) =>
                        {
                            return Err("retained bundle was cleaned up")
                        }
                        _ => {}
                    }
                    joined.owners[owner_index].stages.push(stage);
                }
                Event::End { owner } => {
                    let position = *owners.get(&owner).ok_or("bundle retirement has no owner")?;
                    let state = &joined.owners[position];
                    if state.ended {
                        return Err("bundle owner retired twice");
                    }
                    let own = joined
                        .operations
                        .iter()
                        .filter(|op| op.pid == pid && op.owner == owner)
                        .collect::<Vec<_>>();
                    if matches!(state.origin, Origin::Allocation { .. })
                        && !state.stages.contains(&Stage::Retained)
                        && own.iter().any(|op| {
                            op.action == Action::Directory
                                && op.path == commitment(&state.target.relative())
                                && op.outcome == Some(Outcome::Succeeded)
                        })
                        && !own
                            .iter()
                            .any(|op| matches!(op.action, Action::Remove { .. }))
                    {
                        return Err("armed bundle owner omitted cleanup attempt");
                    }
                    let state = owner_mut(&mut joined, &owners, owner)?;
                    if !state.active.is_empty() {
                        return Err("bundle owner ended with active native operation");
                    }
                    state.ended = true;
                }
                Event::Started {
                    owner,
                    operation,
                    publication,
                    target,
                    path,
                    action,
                } => {
                    if operation == 0 || operations.contains_key(&operation) {
                        return Err("bundle operation token repeated");
                    }
                    let owner_index =
                        *owners.get(&owner).ok_or("bundle operation without owner")?;
                    let state = &joined.owners[owner_index];
                    if state.ended
                        || state.stages.last() == Some(&Stage::Retained)
                        || publication == Some(0)
                    {
                        return Err("bundle operation outside owner lifetime");
                    }
                    let reached = Operation {
                        pid,
                        token: operation,
                        owner,
                        publication,
                        target,
                        path,
                        action,
                        started: index,
                        opened: None,
                        written: None,
                        finished: None,
                        outcome: None,
                        matched_endpoint: false,
                    };
                    if target != state.target
                        && !relocated(retirements, state, &reached, &joined.operations)
                    {
                        return Err("bundle cleanup target lacks exact retirement association");
                    }
                    if state.publication.is_some() && state.publication != publication {
                        return Err("maintenance publication differs");
                    }
                    if let Some(active) = state.active.last() {
                        let prior = &joined.operations
                            [*operations.get(active).ok_or("bundle nested operation")?];
                        if prior.action != Action::ThawTree
                            || !matches!(action, Action::Thaw { .. })
                        {
                            return Err("bundle native operations overlap");
                        }
                    }
                    validate_action(state, target, path, action, publication, &joined.operations)?;
                    operations.insert(operation, joined.operations.len());
                    joined.operations.push(reached);
                    joined.owners[owner_index].active.push(operation);
                }
                Event::Progress {
                    operation,
                    progress,
                } => {
                    let op = &mut joined.operations[*operations
                        .get(&operation)
                        .ok_or("bundle write progress without operation")?];
                    if !matches!(op.action, Action::Control { .. }) || op.outcome.is_some() {
                        return Err("bundle write progress outside write");
                    }
                    match progress {
                        Progress::Opened if op.opened.is_none() => op.opened = Some(index),
                        Progress::Written if op.opened.is_some() && op.written.is_none() => {
                            op.written = Some(index)
                        }
                        _ => return Err("bundle write progress reordered"),
                    }
                }
                Event::Finished { operation, outcome } => {
                    let position = *operations
                        .get(&operation)
                        .ok_or("bundle outcome without operation")?;
                    let op = &mut joined.operations[position];
                    if op.outcome.replace(outcome).is_some()
                        || (matches!(op.action, Action::Control { .. })
                            && outcome == Outcome::Succeeded
                            && op.written.is_none())
                        || (outcome == Outcome::Collision
                            && (op.action != Action::Directory
                                || op.path != commitment(&op.target.relative())))
                    {
                        return Err("bundle native outcome differs");
                    }
                    op.finished = Some(index);
                    let state =
                        &mut joined.owners[*owners.get(&op.owner).ok_or("bundle outcome owner")?];
                    if state.active.pop() != Some(operation) {
                        return Err("bundle native operations finish out of order");
                    }
                    let op = &joined.operations[position];
                    if op.target != state.target
                        && !relocated(retirements, state, op, &joined.operations)
                    {
                        return Err("bundle cleanup completes outside its retirement interval");
                    }
                }
            }
        }
        if owners.values().any(|index| !joined.owners[*index].ended) {
            return Err("bundle owner omitted retirement");
        }
    }
    endpoints(
        &mut joined,
        before,
        after,
        retirements,
        processes.len() == 1,
    )?;
    Ok(joined)
}
fn owner_mut<'a>(
    joined: &'a mut Joined,
    owners: &BTreeMap<u64, usize>,
    token: u64,
) -> Result<&'a mut Owner, &'static str> {
    let owner = &mut joined.owners[*owners.get(&token).ok_or("bundle owner absent")?];
    if owner.ended {
        return Err("bundle event follows owner retirement");
    }
    Ok(owner)
}
fn consumed(
    evidence: &Evidence,
    allocation: principal::pin::Reference,
    publisher: [u8; 32],
) -> bool {
    evidence.events.as_slice().iter().any(|row| {
        let principal::Event::Admitted { admission, publisher: actual, entries, .. } = row else { return false; };
        *actual == publisher && *entries > 0 && evidence.events.as_slice().iter().any(|row| matches!(row,
            principal::Event::Consumed { admission: source, entry, .. } if source == admission && entry.bundled && entry.kind == allocation.kind && entry.schema == allocation.schema && entry.content == allocation.key))
    })
}
fn commitment(path: &Path) -> Commitment {
    Commitment {
        bytes: path.as_os_str().as_encoded_bytes().len() as u64,
        sha256: principal::path_digest(path),
    }
}
fn relocated(
    retirements: &principal::retirement::join::Joined,
    owner: &Owner,
    operation: &Operation,
    operations: &[Operation],
) -> bool {
    let Target::Temporary(temporary) = operation.target else {
        return false;
    };
    temporary.process == owner.pid
        && retirements.attempts.iter().any(|attempt| {
            attempt.pid == owner.pid
                && Some(attempt.publication) == operation.publication
                && attempt.temporary == Some(temporary)
                && attempt.target.relative() == Some(owner.target.relative())
                && attempt
                    .renamed
                    .is_some_and(|index| index < operation.started)
                && attempt
                    .finished_at
                    .is_some_and(|index| operation.finished.unwrap_or(operation.started) < index)
                && match attempt
                    .removal_finished
                    .filter(|index| *index < operation.started && attempt.removal == Some(true))
                {
                    None => true,
                    Some(removed) => {
                        // The deleted temporary remains the owner's context for
                        // its final parent sync, never for more work on that root.
                        operation.action == Action::Sync
                            && temporary
                                .relative()
                                .parent()
                                .is_some_and(|parent| operation.path == commitment(parent))
                            && operations
                                .iter()
                                .filter(|prior| {
                                    prior.pid == owner.pid
                                        && prior.owner == owner.token
                                        && prior.finished.is_some_and(|at| at < operation.started)
                                })
                                .max_by_key(|prior| prior.finished)
                                .is_some_and(|prior| {
                                    prior.target == operation.target
                                        && prior.publication == operation.publication
                                        && prior.action
                                            == (Action::Remove {
                                                cleanup: Cleanup::Checked,
                                            })
                                        && prior.outcome == Some(Outcome::Succeeded)
                                        && prior.finished.is_some_and(|at| {
                                            at > removed && at < operation.started
                                        })
                                })
                    }
                }
        })
}
fn initialized(target: Target, operations: &[&Operation]) -> Result<(), &'static str> {
    let root = target.relative();
    let directories = [
        root.clone(),
        root.join("generated"),
        root.join("build"),
        root.join("target"),
        root.join("declared"),
    ];
    if operations.len() != 8
        || directories.iter().zip(operations).any(|(path, op)| {
            op.action != Action::Directory
                || op.path != commitment(path)
                || op.outcome != Some(Outcome::Succeeded)
        })
        || !matches!(
            operations[5].action,
            Action::Control {
                control: Control::Lease,
                ..
            }
        )
        || operations[5].outcome != Some(Outcome::Succeeded)
        || operations[6].action != Action::Sync
        || operations[6].path != commitment(&root)
        || operations[6].outcome != Some(Outcome::Succeeded)
        || operations[7].action != Action::Sync
        || operations[7].path != commitment(root.parent().ok_or("bundle parent")?)
        || operations[7].outcome != Some(Outcome::Succeeded)
    {
        return Err("bundle initialization omitted or reordered native operations");
    }
    Ok(())
}
fn validate_action(
    owner: &Owner,
    target: Target,
    path: Commitment,
    action: Action,
    publication: Option<u64>,
    operations: &[Operation],
) -> Result<(), &'static str> {
    let root = target.relative();
    let prior = operations
        .iter()
        .filter(|op| op.pid == owner.pid && op.owner == owner.token && op.finished.is_some())
        .max_by_key(|op| op.finished);
    match action {
        Action::Directory => {
            if !matches!(owner.origin, Origin::Allocation { .. })
                || !owner.stages.is_empty()
                || publication.is_some()
                || ![
                    root.clone(),
                    root.join("generated"),
                    root.join("build"),
                    root.join("target"),
                    root.join("declared"),
                ]
                .iter()
                .any(|candidate| commitment(candidate) == path)
            {
                return Err("bundle directory creation target or lifetime");
            }
        }
        Action::Control { control, bytes } => {
            let (name, stage) = match control {
                Control::Lease => ("lease", None),
                Control::Seal => ("seal", Some(Stage::Initialized)),
                Control::Allocation => ("allocation.pending", Some(Stage::Sealed)),
            };
            if !matches!(owner.origin, Origin::Allocation { .. })
                || owner.stages.last().copied() != stage
                || path != commitment(&root.join(name))
            {
                return Err("bundle control path or lifetime");
            }
            if control == Control::Allocation {
                let record = owner
                    .record
                    .as_ref()
                    .ok_or("bundle allocation write lacks its actual record")?;
                let seal = operations
                    .iter()
                    .find_map(|op| match op.action {
                        Action::Control {
                            control: Control::Seal,
                            bytes,
                        } if op.pid == owner.pid
                            && op.owner == owner.token
                            && op.outcome == Some(Outcome::Succeeded) =>
                        {
                            Some(bytes)
                        }
                        _ => None,
                    })
                    .ok_or("bundle allocation omitted sealed bytes")?;
                if Some(record.publication) != publication
                    || !crate::store::matches_observed_bundle(
                        record.allocation,
                        record.lease,
                        record.publisher,
                        seal,
                        bytes,
                        record.geometry,
                    )
                {
                    return Err(
                        "bundle allocation write differs from its canonical record/publication",
                    );
                }
            } else if publication.is_some() {
                return Err("early bundle control claims a publication");
            }
        }
        Action::RenameAllocation => {
            if path != commitment(&root.join("allocation"))
                || owner
                    .record
                    .as_ref()
                    .is_none_or(|record| Some(record.publication) != publication)
                || !prior.is_some_and(|op| {
                    op.action == Action::Sync
                        && op.path == commitment(&root)
                        && op.outcome == Some(Outcome::Succeeded)
                })
                || !operations.iter().any(|op| {
                    op.pid == owner.pid
                        && op.owner == owner.token
                        && matches!(
                            op.action,
                            Action::Control {
                                control: Control::Allocation,
                                ..
                            }
                        )
                        && op.outcome == Some(Outcome::Succeeded)
                })
            {
                return Err("bundle allocation rename lacks completed write/sync");
            }
        }
        Action::Freeze {
            directory,
            before,
            requested,
        } => {
            if requested
                != if directory || before & 0o111 != 0 {
                    0o555
                } else {
                    0o444
                }
                || owner
                    .record
                    .as_ref()
                    .is_none_or(|record| Some(record.publication) != publication)
                || !operations.iter().any(|op| {
                    op.pid == owner.pid
                        && op.owner == owner.token
                        && op.action == Action::RenameAllocation
                        && op.outcome == Some(Outcome::Succeeded)
                })
            {
                return Err("bundle freeze request differs");
            }
        }
        Action::Thaw {
            directory,
            before,
            requested,
        } => {
            if requested != if directory { 0o755 } else { before | 0o200 } {
                return Err("bundle thaw request differs");
            }
        }
        Action::ThawTree | Action::Remove { .. } if path != commitment(&root) => {
            return Err("bundle cleanup escaped root")
        }
        Action::Remove { cleanup } => {
            let thawed = prior.is_some_and(|op| {
                op.action == Action::ThawTree
                    && (cleanup != Cleanup::Checked || op.outcome == Some(Outcome::Succeeded))
            });
            if (cleanup == Cleanup::Initialization
                && (publication.is_some() || !owner.stages.is_empty()))
                || (cleanup != Cleanup::Initialization && !thawed)
                || (cleanup == Cleanup::Drop && publication.is_some())
            {
                return Err("bundle cleanup policy or thaw order differs");
            }
        }
        Action::Sync => {
            let fixed = path == commitment(&root)
                || root
                    .parent()
                    .is_some_and(|parent| path == commitment(parent));
            if !fixed
                && !prior.is_some_and(|op| {
                    matches!(
                        op.action,
                        Action::Freeze {
                            directory: true,
                            ..
                        }
                    ) && op.path == path
                        && op.outcome == Some(Outcome::Succeeded)
                })
            {
                return Err("bundle directory sync lacks reached directory");
            }
        }
        _ => {}
    }
    Ok(())
}

fn endpoints(
    joined: &mut Joined,
    before: &Manifest,
    after: &Manifest,
    retirements: &principal::retirement::join::Joined,
    single: bool,
) -> Result<(), &'static str> {
    // Fixed controls/directories have reconstructible names. Mode commitments
    // are matched only to visited rows; they cannot enumerate missing members.
    for op in &joined.operations {
        let produced = match op.action {
            Action::Directory if op.outcome == Some(Outcome::Succeeded) => known_path(op),
            Action::Control { .. } if op.opened.is_some() => control_path(op, &joined.operations),
            _ => None,
        };
        let Some(path) = produced else {
            continue;
        };
        let bytes = path.as_os_str().as_encoded_bytes();
        let at = op
            .written
            .or(op.opened)
            .or(op.finished)
            .unwrap_or(op.started);
        let retired = retirements.removed_after(op.pid, at, bytes);
        let cleanup = joined.operations.iter().any(|later| {
            later.pid == op.pid
                && later.started > at
                && matches!(later.action, Action::Remove { .. })
                && path.starts_with(later.target.relative())
        });
        if let Action::Control {
            bytes: supplied, ..
        } = op.action
        {
            if op.written.is_some() {
                for member in &after.members {
                    let actual = Path::new(std::ffi::OsStr::from_bytes(&member.path));
                    let direct = actual == path
                        && !retired
                        && !removed_after(&joined.operations, op.pid, at, actual);
                    let relocated = relocated_path(op, actual, retirements, &joined.operations)
                        .is_some_and(|original| original == path);
                    if (direct || relocated)
                        && (member.bytes != supplied.bytes || member.digest != supplied.sha256)
                    {
                        return Err("surviving bundle control differs from completed write");
                    }
                }
            }
        }
        if retired || cleanup {
            continue;
        }
        match op.action {
            Action::Directory
                if !after
                    .directories
                    .iter()
                    .any(|row| row.path.as_ref() == bytes) =>
            {
                return Err("created bundle directory vanished without later cleanup")
            }
            Action::Control {
                bytes: supplied, ..
            } => {
                let member = after
                    .members
                    .iter()
                    .find(|row| row.path.as_ref() == bytes)
                    .ok_or("created bundle control vanished without later rename/cleanup")?;
                if op.written.is_some()
                    && (member.bytes != supplied.bytes || member.digest != supplied.sha256)
                {
                    return Err("completed bundle write differs from endpoint");
                }
            }
            _ => {}
        }
    }
    for (path, directory, mode, contents, present) in after
        .members
        .iter()
        .map(|row| {
            (
                row.path.as_ref(),
                false,
                row.mode,
                Some(Commitment {
                    bytes: row.bytes,
                    sha256: row.digest,
                }),
                true,
            )
        })
        .chain(
            after
                .directories
                .iter()
                .map(|row| (row.path.as_ref(), true, row.mode, None, true)),
        )
        .chain(
            before
                .members
                .iter()
                .filter(|row| !after.members.iter().any(|next| next.path == row.path))
                .map(|row| {
                    (
                        row.path.as_ref(),
                        false,
                        row.mode,
                        Some(Commitment {
                            bytes: row.bytes,
                            sha256: row.digest,
                        }),
                        false,
                    )
                }),
        )
        .chain(
            before
                .directories
                .iter()
                .filter(|row| !after.directories.iter().any(|next| next.path == row.path))
                .map(|row| (row.path.as_ref(), true, row.mode, None, false)),
        )
    {
        let native = Path::new(std::ffi::OsStr::from_bytes(path));
        let mut endpoint = Endpoint {
            path: path.into(),
            directory,
            present,
            creations: Vec::new(),
            writes: Vec::new(),
            partial_writes: Vec::new(),
            removals: Vec::new(),
            matching_modes: Vec::new(),
        };
        let mut covered = false;
        let mut last_mode = None;
        for n in 0..joined.operations.len() {
            let op = &joined.operations[n];
            if !native.starts_with(op.target.relative())
                && !relocated_path(op, native, retirements, &joined.operations).is_some()
            {
                continue;
            }
            covered = true;
            let exact = path_matches(op, native, retirements, &joined.operations);
            let survives = !retirements.removed_after(op.pid, op.started, path)
                && !removed_after(&joined.operations, op.pid, op.started, native);
            if single && exact && survives && op.outcome == Some(Outcome::Succeeded) {
                if let Action::Freeze {
                    directory: actual,
                    requested,
                    ..
                }
                | Action::Thaw {
                    directory: actual,
                    requested,
                    ..
                } = op.action
                {
                    let at = op.finished.ok_or("bundle mode omitted terminal position")?;
                    if actual == directory && last_mode.is_none_or(|(previous, _)| at > previous) {
                        last_mode = Some((at, requested & 0o7777));
                    }
                }
            }
            let mut matched = exact;
            match op.action {
                Action::Directory
                    if directory && exact && op.outcome == Some(Outcome::Succeeded) && survives =>
                {
                    endpoint.creations.push((op.pid, op.token))
                }
                Action::Control { bytes, .. } if !directory => {
                    if control_path(op, &joined.operations).is_some_and(|value| {
                        value == native
                            || relocated_path(op, native, retirements, &joined.operations)
                                .is_some_and(|original| original == value)
                    }) && survives
                    {
                        matched = true;
                        if op.written.is_some() && contents == Some(bytes) {
                            endpoint.writes.push((op.pid, op.token));
                        } else if op.opened.is_some() && op.written.is_none() {
                            endpoint.partial_writes.push((op.pid, op.token));
                        }
                    }
                }
                Action::Freeze {
                    directory: actual,
                    requested,
                    ..
                }
                | Action::Thaw {
                    directory: actual,
                    requested,
                    ..
                } if exact
                    && actual == directory
                    && requested & 0o7777 == mode
                    && op.outcome == Some(Outcome::Succeeded)
                    && survives =>
                {
                    endpoint.matching_modes.push((op.pid, op.token))
                }
                Action::Remove { .. } if native.starts_with(op.target.relative()) => {
                    endpoint.removals.push((op.pid, op.token));
                    matched = true;
                    if present
                        && op.outcome == Some(Outcome::Succeeded)
                        && !recreated_after(&joined.operations, op, native, retirements)
                    {
                        return Err("removed bundle retains endpoints without later creation");
                    }
                }
                _ => {}
            }
            joined.operations[n].matched_endpoint |= matched;
        }
        if !covered {
            continue;
        }
        if present && last_mode.is_some_and(|(_, requested)| requested != mode) {
            return Err("bundle endpoint contradicts its last surviving mode effect");
        }
        let initial_contents = !directory
            && before.members.iter().any(|row| {
                row.path.as_ref() == path
                    && contents
                        == Some(Commitment {
                            bytes: row.bytes,
                            sha256: row.digest,
                        })
            });
        let fixed_control = native.file_name().is_some_and(|name| {
            matches!(
                name.as_encoded_bytes(),
                b"lease" | b"seal" | b"allocation" | b"allocation.pending"
            )
        }) && joined
            .owners
            .iter()
            .any(|owner| native.parent() == Some(owner.target.relative().as_path()));
        if present
            && fixed_control
            && !directory
            && !initial_contents
            && endpoint.writes.is_empty()
            && (single || endpoint.partial_writes.is_empty())
        {
            return Err("new bundle control lacks surviving actual write");
        }
        let fixed_directory = directory
            && joined.owners.iter().any(|owner| {
                let root = owner.target.relative();
                native == root
                    || native.parent() == Some(root.as_path())
                        && native.file_name().is_some_and(|name| {
                            matches!(
                                name.as_encoded_bytes(),
                                b"generated" | b"build" | b"target" | b"declared"
                            )
                        })
            });
        if present
            && fixed_directory
            && !before
                .directories
                .iter()
                .any(|row| row.path.as_ref() == path)
            && endpoint.creations.is_empty()
        {
            return Err("new fixed bundle directory lacks surviving actual creation");
        }
        if single
            && present
            && (fixed_control || fixed_directory)
            && endpoint.matching_modes.is_empty()
            && !before
                .members
                .iter()
                .any(|row| !directory && row.path.as_ref() == path && row.mode == mode)
            && !before
                .directories
                .iter()
                .any(|row| directory && row.path.as_ref() == path && row.mode == mode)
            && endpoint.creations.is_empty()
        {
            return Err("covered bundle mode has no unchanged initial fact or surviving effect");
        }
        if !present
            && endpoint.removals.is_empty()
            && !retirements.attempts.iter().any(|attempt| {
                attempt
                    .target
                    .relative()
                    .is_some_and(|target| native.starts_with(target))
                    && attempt.renamed.is_some()
            })
        {
            return Err("initial bundle endpoint disappeared without removal/retirement");
        }
        joined.endpoints.push(endpoint);
    }
    Ok(())
}
fn known_path(op: &Operation) -> Option<PathBuf> {
    let root = op.target.relative();
    [
        root.clone(),
        root.join("generated"),
        root.join("build"),
        root.join("target"),
        root.join("declared"),
    ]
    .into_iter()
    .find(|path| commitment(path) == op.path)
}
fn control_path(op: &Operation, operations: &[Operation]) -> Option<PathBuf> {
    let Action::Control { control, .. } = op.action else {
        return None;
    };
    let name = match control {
        Control::Lease => "lease",
        Control::Seal => "seal",
        Control::Allocation
            if operations.iter().any(|later| {
                later.pid == op.pid
                    && later.owner == op.owner
                    && later.started > op.started
                    && later.action == Action::RenameAllocation
                    && later.outcome == Some(Outcome::Succeeded)
            }) =>
        {
            "allocation"
        }
        Control::Allocation => "allocation.pending",
    };
    Some(op.target.relative().join(name))
}
fn removed_after(operations: &[Operation], pid: u32, at: usize, path: &Path) -> bool {
    operations.iter().any(|later| {
        later.pid == pid
            && later.started > at
            && matches!(later.action, Action::Remove { .. })
            && later.outcome == Some(Outcome::Succeeded)
            && path.starts_with(later.target.relative())
    })
}
fn recreated_after(
    operations: &[Operation],
    removed: &Operation,
    path: &Path,
    retirements: &principal::retirement::join::Joined,
) -> bool {
    operations.iter().any(|later| {
        later.pid == removed.pid
            && later.started > removed.started
            && later.action == Action::Directory
            && later.outcome == Some(Outcome::Succeeded)
            && later.path == commitment(&later.target.relative())
            && path.starts_with(later.target.relative())
            && !removed_after(operations, later.pid, later.started, path)
            && !retirements.removed_after(
                later.pid,
                later.started,
                path.as_os_str().as_encoded_bytes(),
            )
    })
}
fn path_matches(
    op: &Operation,
    path: &Path,
    retirements: &principal::retirement::join::Joined,
    operations: &[Operation],
) -> bool {
    commitment(path) == op.path
        || relocated_path(op, path, retirements, operations)
            .is_some_and(|original| commitment(&original) == op.path)
}
fn relocated_path(
    op: &Operation,
    path: &Path,
    retirements: &principal::retirement::join::Joined,
    operations: &[Operation],
) -> Option<PathBuf> {
    retirements.attempts.iter().find_map(|attempt| {
        if attempt.pid != op.pid
            || !attempt.renamed.is_some_and(|at| at > op.started)
            || attempt.removal == Some(true)
        {
            return None;
        }
        let original = attempt.target.relative()?;
        if original != op.target.relative() {
            return None;
        }
        let temporary = attempt.temporary?.relative();
        let suffix = path.strip_prefix(temporary).ok()?;
        let source = if suffix.as_os_str().is_empty() {
            original
        } else {
            original.join(suffix)
        };
        if operations.iter().any(|later| {
            later.pid == op.pid
                && later.started > op.started
                && matches!(later.action, Action::Remove { .. })
                && later.outcome == Some(Outcome::Succeeded)
                && (path.starts_with(later.target.relative())
                    || attempt
                        .renamed
                        .is_some_and(|renamed| later.started < renamed)
                        && source.starts_with(later.target.relative()))
        }) {
            return None;
        }
        Some(source)
    })
}
