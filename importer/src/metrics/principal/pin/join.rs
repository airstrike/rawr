//! Pair-local pin facts. These do not establish directory or control closure.

use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::{Event, Identity, Mode, Operation, Reference, Step, TransactionStep};
use crate::metrics::cell::project::Manifest;
use crate::metrics::principal::{self, Commitment, Consumed, Evidence};
use crate::metrics::publication::Role;

#[derive(Debug, Serialize)]
pub struct Joined {
    pub processes: Box<[Process]>,
    pub endpoints: Vec<Endpoint>,
    pub staged: Vec<Staged>,
}

#[derive(Debug, Serialize)]
pub struct Process {
    pub pid: u32,
    pub records: usize,
    pub attempts: usize,
    pub owners_retired: usize,
    pub transactions: usize,
    pub restore_attempts: usize,
    pub renamed: usize,
    pub removed: usize,
    pub failed_removals: usize,
    pub unfinished_retirements: usize,
}

/// The processes whose actual renames match the final regular-file bytes.
/// This deliberately does not assign a last writer across the two processes.
#[derive(Debug, Serialize)]
pub struct Endpoint {
    pub path: PathBuf,
    pub matches_initial: bool,
    pub matching_writers: Vec<u32>,
}

/// Actual retirement destinations, including sync failures and partial removals.
#[derive(Debug, Serialize)]
pub struct Staged {
    pub pid: u32,
    pub attempt: u64,
    pub relative: [u8; 32],
    pub original: PathBuf,
    pub source: Option<u64>,
    pub record: Option<Commitment>,
    pub relocated: usize,
    pub removal_started: Option<usize>,
    pub removed: Option<bool>,
    pub removal_finished: Option<usize>,
    pub remaining_regular_members: usize,
}

struct Record<'a> {
    identity: &'a Identity,
    publication: u64,
    references: Vec<&'a Reference>,
}

struct Attempt {
    publication: u64,
    position: usize,
    source: Option<u64>,
    operation: Operation,
    path: PathBuf,
    existing: bool,
    renamed: bool,
    removal_started: bool,
    removal_position: Option<usize>,
    removed: Option<bool>,
    removed_position: Option<usize>,
    absent: bool,
    finished: Option<bool>,
}

struct Owner {
    source: u64,
    mode: Mode,
    retired: bool,
}

struct Transaction {
    publication: u64,
    position: usize,
    before: principal::ledger::Prior,
    after: Commitment,
    project: [u8; 32],
    proof: [u8; 32],
    count: u64,
    roles: Vec<Role>,
    previous_count: u64,
    previous: Vec<u64>,
    acquired: Vec<u64>,
    ledger_attempted: bool,
    binding: Option<u64>,
    ledger: bool,
    validated: bool,
    callback: Option<bool>,
    restore: Option<u64>,
    restore_position: usize,
    restore_applied: bool,
    restored: bool,
    committed: bool,
    finished: bool,
}

struct Admission<'a> {
    project: [u8; 32],
    proof: [u8; 32],
    count: u64,
    entries: Vec<&'a Consumed>,
}

/// Only already-framed histories and parent regular-file manifests are read.
pub fn pair(
    processes: [(u32, &Evidence); 2],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
) -> Result<Joined, &'static str> {
    if processes[0].0 == 0 || processes[1].0 == 0 || processes[0].0 == processes[1].0 {
        return Err("pin join requires two distinct process owners");
    }
    join(&processes, root, before, after)
}

pub(in crate::metrics::principal) fn join(
    processes: &[(u32, &Evidence)],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
) -> Result<Joined, &'static str> {
    let root = principal::path_digest(root);
    let mut results = Vec::new();
    let mut writes = Vec::new();
    let mut retirements = Vec::new();
    let mut staged = Vec::new();
    for (pid, evidence) in processes.iter().copied() {
        if evidence.fault.is_some() || !evidence.cell_transport {
            return Err("pin observation is incomplete");
        }
        let mut records = BTreeMap::<u64, Record<'_>>::new();
        let mut attempts = BTreeMap::<u64, Attempt>::new();
        let mut owners = BTreeMap::<u64, Owner>::new();
        let mut transactions = BTreeMap::<u64, Transaction>::new();
        let mut admissions = BTreeMap::<u64, Admission<'_>>::new();
        let mut result = Process {
            pid,
            records: 0,
            attempts: 0,
            owners_retired: 0,
            transactions: 0,
            restore_attempts: 0,
            renamed: 0,
            removed: 0,
            failed_removals: 0,
            unfinished_retirements: 0,
        };
        for (position, event) in evidence.events.as_slice().iter().enumerate() {
            match event {
                principal::Event::Admitted {
                    admission,
                    root: actual,
                    publisher,
                    proof,
                    entries,
                } => {
                    if *admission == 0
                        || *actual != root
                        || admissions
                            .insert(
                                *admission,
                                Admission {
                                    project: *publisher,
                                    proof: *proof,
                                    count: *entries,
                                    entries: Vec::new(),
                                },
                            )
                            .is_some()
                    {
                        return Err("pin consumer admission changed");
                    }
                }
                principal::Event::Consumed {
                    admission,
                    index,
                    entry,
                } => {
                    let state = admissions
                        .get_mut(admission)
                        .ok_or("pin consumer has no admission")?;
                    if *index != state.entries.len() as u64 || *index >= state.count {
                        return Err("pin consumer order changed");
                    }
                    state.entries.push(entry);
                }
                principal::Event::Ledger {
                    change:
                        principal::ledger::Event::Started {
                            attempt,
                            publication,
                            target: principal::ledger::Target::Project(project),
                            cause,
                            ..
                        },
                } if matches!(
                    cause,
                    principal::ledger::Cause::PinBind | principal::ledger::Cause::PinRestore
                ) =>
                {
                    let state = transactions
                        .values_mut()
                        .find(|state| {
                            state.publication == *publication
                                && state.project == *project
                                && !state.finished
                        })
                        .ok_or("pin ledger mutation has no active transaction")?;
                    if *cause == principal::ledger::Cause::PinBind {
                        if !state.ledger_attempted
                            || state.binding.is_some()
                            || state.ledger
                            || state.restore.is_some()
                        {
                            return Err("pin binding write is outside its reached interval");
                        }
                        held(state, &owners)?;
                        state.binding = Some(*attempt);
                    } else {
                        restoring(state, &owners)?;
                    }
                }
                principal::Event::Ledger {
                    change:
                        principal::ledger::Event::Renamed { attempt }
                        | principal::ledger::Event::Cleanup { attempt, .. }
                        | principal::ledger::Event::Finished { attempt, .. },
                } => {
                    if let Some(state) = transactions
                        .values_mut()
                        .find(|state| state.binding == Some(*attempt))
                    {
                        held(state, &owners)?;
                        if matches!(
                            event,
                            principal::Event::Ledger {
                                change: principal::ledger::Event::Finished { .. }
                            }
                        ) {
                            state.binding = None;
                        }
                    }
                }
                principal::Event::Pin { change } => match change {
                    Event::Record {
                        source,
                        publication,
                        identity,
                    } => {
                        if *source == 0
                            || *publication == 0
                            || identity.root != root
                            || identity.relative().is_none()
                            || identity.references > 4096
                            || identity.record.bytes != 120 + identity.references * 96
                            || records
                                .insert(
                                    *source,
                                    Record {
                                        identity,
                                        publication: *publication,
                                        references: Vec::new(),
                                    },
                                )
                                .is_some()
                        {
                            return Err("pin source identity changed");
                        }
                    }
                    Event::Reference {
                        source,
                        index,
                        reference,
                    } => {
                        let state = records
                            .get_mut(source)
                            .ok_or("pin reference has no source")?;
                        if *index != state.references.len() as u64
                            || *index >= state.identity.references
                            || !(1..=3).contains(&reference.class)
                            || crate::node::Kind::from_code(reference.kind).is_none()
                            || state
                                .references
                                .last()
                                .is_some_and(|previous| **previous >= *reference)
                        {
                            return Err("pin reference order or identity changed");
                        }
                        state.references.push(reference);
                    }
                    Event::Started {
                        attempt,
                        publication,
                        source,
                        root: actual,
                        relative,
                        operation,
                    } => {
                        let path = match operation {
                            Operation::Project {
                                project,
                                purpose,
                                cause,
                            } => {
                                if source.is_some()
                                    || !principal::ledger::compatible(
                                        principal::ledger::Target::Project(*project),
                                        1,
                                        *purpose,
                                        *cause,
                                    )
                                {
                                    return Err("project retirement purpose changed");
                                }
                                principal::ledger::Target::Project(*project).relative()
                            }
                            _ => records
                                .get(&source.ok_or("pin attempt lost its source")?)
                                .ok_or("pin attempt precedes source")?
                                .identity
                                .relative()
                                .ok_or("pin source namespace")?,
                        };
                        let restoring_project = match operation {
                            Operation::Project {
                                project,
                                cause: principal::ledger::Cause::PinRestore,
                                ..
                            } => Some((*project, true)),
                            Operation::Release => source
                                .and_then(|source| records.get(&source))
                                .map(|record| (record.identity.project, false)),
                            _ => None,
                        };
                        if let Some((project, required)) = restoring_project {
                            let state = transactions.values().find(|state| {
                                state.publication == *publication
                                    && state.project == project
                                    && !state.finished
                                    && state.restore.is_some()
                            });
                            match state {
                                Some(state) => restoring(state, &owners)?,
                                None if required => {
                                    return Err("project restoration has no active retry interval")
                                }
                                None => {}
                            }
                        }
                        if *attempt == 0
                            || *publication == 0
                            || *actual != root
                            || principal::path_digest(&path) != *relative
                            || source.is_some_and(|source| {
                                records[&source].references.len() as u64
                                    != records[&source].identity.references
                                    || (*operation != Operation::Stale
                                        && records[&source].publication != *publication)
                            })
                            || attempts
                                .insert(
                                    *attempt,
                                    Attempt {
                                        publication: *publication,
                                        position,
                                        source: *source,
                                        operation: *operation,
                                        path,
                                        existing: false,
                                        renamed: false,
                                        removal_started: false,
                                        removal_position: None,
                                        removed: None,
                                        removed_position: None,
                                        absent: false,
                                        finished: None,
                                    },
                                )
                                .is_some()
                        {
                            return Err("pin attempt identity changed");
                        }
                    }
                    Event::Relocated { attempt, temporary } => {
                        let state = attempts
                            .get_mut(attempt)
                            .ok_or("pin relocation has no attempt")?;
                        if !retiring(state.operation)
                            || state.finished.is_some()
                            || state.renamed
                            || state.absent
                            || state.source.is_some_and(|source| {
                                !owners.values().any(|owner| {
                                    owner.source == source
                                        && owner.mode == Mode::Exclusive
                                        && !owner.retired
                                })
                            })
                        {
                            return Err("pin relocation order changed");
                        }
                        state.renamed = true;
                        result.renamed += 1;
                        retirements.push(state.path.clone());
                        staged.push(Staged {
                            pid,
                            attempt: *attempt,
                            relative: *temporary,
                            original: state.path.clone(),
                            source: state.source,
                            record: state.source.map(|source| records[&source].identity.record),
                            relocated: position,
                            removal_started: None,
                            removed: None,
                            removal_finished: None,
                            remaining_regular_members: after
                                .members
                                .iter()
                                .filter(|member| {
                                    let path = Path::new(std::ffi::OsStr::from_bytes(&member.path));
                                    path.starts_with("temporaries")
                                        && (principal::path_digest(path) == *temporary
                                            || path.parent().is_some_and(|parent| {
                                                principal::path_digest(parent) == *temporary
                                            }))
                                })
                                .count(),
                        });
                    }
                    Event::Step { attempt, step } => {
                        let state = attempts.get_mut(attempt).ok_or("pin step has no attempt")?;
                        if state.finished.is_some() {
                            return Err("pin step follows terminal outcome");
                        }
                        let retirement = retiring(state.operation);
                        match step {
                            Step::Existing
                                if state.operation == Operation::Acquire
                                    && !state.existing
                                    && !state.renamed =>
                            {
                                state.existing = true
                            }
                            Step::Renamed
                                if !state.renamed
                                    && !state.existing
                                    && !state.absent
                                    && state.operation == Operation::Acquire =>
                            {
                                state.renamed = true;
                                result.renamed += 1;
                                let source = records
                                    .get(&state.source.ok_or("pin publication lost source")?)
                                    .ok_or("pin publication source")?;
                                writes.push((pid, state.path.join("root"), source.identity.record));
                            }
                            Step::RemovalStarted
                                if retirement && state.renamed && !state.removal_started =>
                            {
                                state.removal_started = true;
                                state.removal_position = Some(position);
                            }
                            Step::Removed | Step::RemovalFailed
                                if retirement
                                    && state.removal_started
                                    && state.removed.is_none() =>
                            {
                                let succeeded = *step == Step::Removed;
                                state.removed = Some(succeeded);
                                state.removed_position = Some(position);
                                result.removed += usize::from(succeeded);
                                result.failed_removals += usize::from(!succeeded);
                            }
                            Step::Absent
                                if matches!(state.operation, Operation::Project { .. })
                                    && !state.absent
                                    && !state.renamed =>
                            {
                                state.absent = true
                            }
                            _ => return Err("pin mutation order changed"),
                        }
                    }
                    Event::Finished { attempt, succeeded } => {
                        let state = attempts
                            .get_mut(attempt)
                            .ok_or("pin result has no attempt")?;
                        let complete = match state.operation {
                            Operation::Acquire => state.renamed || state.existing,
                            Operation::Retain => true,
                            _ => state.absent || state.removed == Some(true),
                        };
                        if state.finished.replace(*succeeded).is_some()
                            || (*succeeded
                                && (!complete
                                    || (matches!(
                                        state.operation,
                                        Operation::Acquire | Operation::Retain
                                    ) && !owners.values().any(|owner| {
                                        Some(owner.source) == state.source
                                            && owner.mode == Mode::Shared
                                            && !owner.retired
                                    }))))
                        {
                            return Err("pin result differs from reached mutations");
                        }
                        result.unfinished_retirements +=
                            usize::from(retiring(state.operation) && !*succeeded);
                    }
                    Event::Owner {
                        owner,
                        source,
                        mode,
                    } => {
                        let record = records.get(source).ok_or("pin owner has no source")?;
                        if *owner == 0
                            || record.references.len() as u64 != record.identity.references
                            || owners
                                .insert(
                                    *owner,
                                    Owner {
                                        source: *source,
                                        mode: *mode,
                                        retired: false,
                                    },
                                )
                                .is_some()
                        {
                            return Err("pin owner identity changed");
                        }
                    }
                    Event::Retired { owner } => {
                        if transactions.values().any(|transaction| {
                            transaction.binding.is_some() && transaction.acquired.contains(owner)
                        }) {
                            return Err(
                                "acquired pin owner retired before its binding write finished",
                            );
                        }
                        if transactions.values().any(|transaction| {
                            transaction.previous.contains(owner)
                                && !transaction.restore_applied
                                && transaction.callback != Some(true)
                        }) {
                            return Err("previous pin owner retired during protected work");
                        }
                        let state = owners
                            .get_mut(owner)
                            .ok_or("pin retirement has no opening in this process history")?;
                        if std::mem::replace(&mut state.retired, true) {
                            return Err("pin owner retired twice");
                        }
                        result.owners_retired += 1;
                    }
                    Event::Transaction {
                        transaction,
                        publication,
                        root: actual,
                        project,
                        proof,
                        roles,
                        previous,
                        before,
                        after,
                    } => {
                        if *transaction == 0
                            || *publication == 0
                            || *actual != root
                            || *roles > 4
                            || *previous > 4
                            || *before == principal::ledger::Prior::Unknown
                            || transactions
                                .insert(
                                    *transaction,
                                    Transaction {
                                        publication: *publication,
                                        position,
                                        before: *before,
                                        after: *after,
                                        project: *project,
                                        proof: *proof,
                                        count: *roles,
                                        roles: Vec::new(),
                                        previous_count: *previous,
                                        previous: Vec::new(),
                                        acquired: Vec::new(),
                                        ledger_attempted: false,
                                        binding: None,
                                        ledger: false,
                                        validated: false,
                                        callback: None,
                                        restore: None,
                                        restore_position: 0,
                                        restore_applied: false,
                                        restored: false,
                                        committed: false,
                                        finished: false,
                                    },
                                )
                                .is_some()
                        {
                            return Err("pin transaction identity changed");
                        }
                    }
                    Event::Role {
                        transaction,
                        index,
                        role,
                    } => {
                        let state = transactions
                            .get_mut(transaction)
                            .ok_or("pin role has no transaction")?;
                        if *index != state.roles.len() as u64
                            || *index >= state.count
                            || state.roles.contains(role)
                            || !state.acquired.is_empty()
                            || state.finished
                        {
                            return Err("pin role order changed");
                        }
                        state.roles.push(*role);
                    }
                    Event::TransactionStep { transaction, step } => {
                        let state = transactions
                            .get_mut(transaction)
                            .ok_or("pin transaction step has no start")?;
                        if state.finished || state.roles.len() as u64 != state.count {
                            return Err("pin transaction is incomplete or already finished");
                        }
                        match step {
                            TransactionStep::Previous { owner } => {
                                let guard = owners.get(owner).ok_or("previous pin owner absent")?;
                                if guard.retired
                                    || guard.mode != Mode::Shared
                                    || records[&guard.source].identity.project != state.project
                                    || records[&guard.source].publication != state.publication
                                    || !state.acquired.is_empty()
                                    || state.previous.contains(owner)
                                    || state.previous.len() as u64 >= state.previous_count
                                {
                                    return Err("previous pin owner changed");
                                }
                                state.previous.push(*owner);
                            }
                            TransactionStep::Acquired { index, owner } => {
                                held(state, &owners)?;
                                let owner_state =
                                    owners.get(owner).ok_or("acquired pin owner absent")?;
                                let record = records[&owner_state.source].identity;
                                if *index != state.acquired.len() as u64
                                    || *index >= state.count
                                    || owner_state.retired
                                    || owner_state.mode != Mode::Shared
                                    || record.project != state.project
                                    || record.proof != state.proof
                                    || record.role != state.roles[*index as usize]
                                    || records[&owner_state.source].publication != state.publication
                                    || state.ledger
                                    || state.restore.is_some()
                                {
                                    return Err(
                                        "acquired pin differs from ordered transaction role",
                                    );
                                }
                                state.acquired.push(*owner);
                            }
                            TransactionStep::LedgerAttempted
                                if !state.ledger_attempted
                                    && !state.ledger
                                    && state.restore.is_none()
                                    && state.acquired.len() as u64 == state.count =>
                            {
                                held(state, &owners)?;
                                if state.before == principal::ledger::Prior::Bytes(state.after) {
                                    return Err("unchanged pin ledger was not a write attempt");
                                }
                                state.ledger_attempted = true;
                            }
                            TransactionStep::Ledger { replaced }
                                if state.acquired.len() as u64 == state.count
                                    && !state.ledger
                                    && state.binding.is_none()
                                    && state.restore.is_none() =>
                            {
                                held(state, &owners)?;
                                if *replaced != state.ledger_attempted
                                    || *replaced
                                        != (state.before
                                            != principal::ledger::Prior::Bytes(state.after))
                                {
                                    return Err("pin ledger replacement decision changed");
                                }
                                if *replaced {
                                    let reached =
                                        &evidence.events.as_slice()[state.position..position];
                                    let write = reached.iter().find_map(|event| match event {
                                        principal::Event::Ledger { change: principal::ledger::Event::Started { attempt, publication, target: principal::ledger::Target::Project(project), cause: principal::ledger::Cause::PinBind, before, after, .. } } if *publication == state.publication && *project == state.project && *before == state.before && *after == state.after => Some(*attempt),
                                        _ => None,
                                    }).ok_or("pin binding lacks its actual ledger replacement")?;
                                    if !reached.iter().any(|event| matches!(event, principal::Event::Ledger { change: principal::ledger::Event::Finished { attempt, succeeded: true } } if *attempt == write)) { return Err("pin ledger step precedes replacement completion"); }
                                }
                                state.ledger = true;
                            }
                            TransactionStep::Validated
                                if state.ledger && !state.validated && state.restore.is_none() =>
                            {
                                held(state, &owners)?;
                                state.validated = true
                            }
                            TransactionStep::Callback { succeeded }
                                if state.validated
                                    && state.callback.is_none()
                                    && state.restore.is_none() =>
                            {
                                held(state, &owners)?;
                                state.callback = Some(*succeeded)
                            }
                            TransactionStep::Committed
                                if state.callback == Some(true)
                                    && state.restore.is_none()
                                    && !state.committed =>
                            {
                                require(&state.acquired, &owners, false)?;
                                require(&state.previous, &owners, true)?;
                                state.committed = true
                            }
                            TransactionStep::Restore { attempt }
                                if *attempt != 0
                                    && state.binding.is_none()
                                    && state.restore.is_none()
                                    && !state.restored
                                    && !state.committed =>
                            {
                                if state.previous.len() as u64 != state.previous_count {
                                    return Err("previous pin cohort is incomplete");
                                }
                                require(&state.previous, &owners, false)?;
                                state.restore = Some(*attempt);
                                state.restore_position = position;
                                state.restore_applied = false;
                                result.restore_attempts += 1;
                            }
                            TransactionStep::RestoreApplied { attempt }
                                if state.restore == Some(*attempt) && !state.restore_applied =>
                            {
                                require(&state.acquired, &owners, true)?;
                                require(&state.previous, &owners, false)?;
                                restored_ledger(
                                    state,
                                    &evidence.events.as_slice()[state.restore_position..position],
                                    &attempts,
                                )?;
                                state.restore_applied = true;
                            }
                            TransactionStep::Restored { attempt, succeeded }
                                if state.restore == Some(*attempt) =>
                            {
                                require(&state.acquired, &owners, true)?;
                                require(&state.previous, &owners, *succeeded)?;
                                if *succeeded != state.restore_applied {
                                    return Err("pin restoration outcome has no completed protected interval");
                                }
                                state.restore = None;
                                state.restored = *succeeded;
                            }
                            _ => return Err("pin transaction outcome order changed"),
                        }
                    }
                    Event::TransactionFinished {
                        transaction,
                        pending,
                    } => {
                        let state = transactions
                            .get_mut(transaction)
                            .ok_or("pin transaction result has no start")?;
                        if state.finished
                            || *pending
                            || state.binding.is_some()
                            || state.restore.is_some()
                            || (!state.committed && !state.restored)
                        {
                            return Err("pin transaction has an unclosed replacement");
                        }
                        state.finished = true;
                    }
                },
                _ => {}
            }
        }
        if owners.values().any(|owner| !owner.retired)
            || attempts.values().any(|attempt| attempt.finished.is_none())
            || transactions
                .values()
                .any(|transaction| !transaction.finished)
        {
            return Err("pin history has an unretired owner or unfinished operation");
        }
        for record in records.values() {
            canonical(record)?;
        }
        for attempt in attempts.values().filter(|attempt| {
            attempt.finished == Some(true)
                && matches!(attempt.operation, Operation::Acquire | Operation::Retain)
        }) {
            if !owners
                .values()
                .any(|owner| Some(owner.source) == attempt.source && owner.mode == Mode::Shared)
            {
                return Err("successful pin acquisition omitted its guard");
            }
        }
        for transaction in transactions.values().filter(|transaction| {
            transaction.committed && transaction.roles.as_slice() == [Role::Live, Role::LastGood]
        }) {
            let admission = admissions
                .values()
                .find(|admission| {
                    admission.project == transaction.project
                        && admission.proof == transaction.proof
                        && admission.entries.len() as u64 == admission.count
                })
                .ok_or("live pins lack their own process's admitted read set")?;
            for owner in &transaction.acquired {
                let record = &records[&owners[owner].source];
                if record.references.iter().any(|reference| {
                    !admission
                        .entries
                        .iter()
                        .any(|entry| matches_entry(reference, entry))
                }) || admission.entries.iter().any(|entry| {
                    [1, 2]
                        .into_iter()
                        .chain(entry.bundled.then_some(3))
                        .any(|class| {
                            !record.references.iter().any(|reference| {
                                reference.class == class && matches_entry(reference, entry)
                            })
                        })
                }) {
                    return Err("pin references differ from the admitted native read set");
                }
            }
        }
        for row in staged.iter_mut().filter(|row| row.pid == pid) {
            let attempt = &attempts[&row.attempt];
            row.removal_started = attempt.removal_position;
            row.removed = attempt.removed;
            row.removal_finished = attempt.removed_position;
        }
        result.records = records.len();
        result.attempts = attempts.len();
        result.transactions = transactions.len();
        results.push(result);
    }
    let mut endpoints = Vec::new();
    for member in after
        .members
        .iter()
        .filter(|member| member.path.starts_with(b"pins/"))
    {
        let path = Path::new(std::ffi::OsStr::from_bytes(&member.path));
        let commitment = Commitment {
            bytes: member.bytes,
            sha256: member.digest,
        };
        let matches_initial = before.members.iter().any(|initial| initial == member);
        let mut matching_writers = Vec::new();
        for (pid, target, written) in &writes {
            if target == path && *written == commitment && !matching_writers.contains(pid) {
                matching_writers.push(*pid);
            }
        }
        if processes.len() == 1
            && writes
                .iter()
                .rev()
                .find(|(_, target, _)| target == path)
                .is_some_and(|(_, _, written)| *written != commitment)
        {
            return Err("pin endpoint contradicts its last actual replacement");
        }
        if !matches_initial && matching_writers.is_empty() {
            return Err("pin endpoint has no actual matching publication");
        }
        endpoints.push(Endpoint {
            path: path.to_path_buf(),
            matches_initial,
            matching_writers,
        });
    }
    for member in before.members.iter().filter(|member| {
        member.path.starts_with(b"pins/") || member.path.starts_with(b"ledgers/projects/")
    }) {
        if after.members.iter().any(|after| after.path == member.path) {
            continue;
        }
        let path = Path::new(std::ffi::OsStr::from_bytes(&member.path));
        if !retirements.iter().any(|retired| {
            path == retired
                || (path.file_name().is_some_and(|name| name == "root")
                    && path.parent() == Some(retired.as_path()))
        }) {
            return Err("retired pin or project endpoint lacks an actual rename");
        }
    }
    Ok(Joined {
        processes: results.into_boxed_slice(),
        endpoints,
        staged,
    })
}

fn retiring(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::Release | Operation::Stale | Operation::Project { .. }
    )
}

fn require(
    tokens: &[u64],
    owners: &BTreeMap<u64, Owner>,
    retired: bool,
) -> Result<(), &'static str> {
    if tokens.iter().any(|token| {
        owners
            .get(token)
            .is_none_or(|owner| owner.retired != retired)
    }) {
        return Err("pin guard lifetime differs from its protected transaction interval");
    }
    Ok(())
}

fn held(transaction: &Transaction, owners: &BTreeMap<u64, Owner>) -> Result<(), &'static str> {
    if transaction.previous.len() as u64 != transaction.previous_count {
        return Err("previous pin cohort is incomplete");
    }
    require(&transaction.previous, owners, false)?;
    require(&transaction.acquired, owners, false)
}

fn restoring(transaction: &Transaction, owners: &BTreeMap<u64, Owner>) -> Result<(), &'static str> {
    if transaction.restore.is_none() || transaction.restore_applied {
        return Err("pin restoration mutation is outside its protected interval");
    }
    require(&transaction.previous, owners, false)?;
    require(&transaction.acquired, owners, true)
}

/// Every retry must perform its own obligation. A successful later transaction
/// or an earlier retry's write cannot stand in for this reached restoration.
fn restored_ledger(
    transaction: &Transaction,
    interval: &[principal::Event],
    attempts: &BTreeMap<u64, Attempt>,
) -> Result<(), &'static str> {
    if !transaction.ledger_attempted {
        return Ok(());
    }
    match transaction.before {
        principal::ledger::Prior::Bytes(expected) => {
            let attempt = interval.iter().find_map(|event| match event {
                principal::Event::Ledger { change: principal::ledger::Event::Started { attempt, publication, target: principal::ledger::Target::Project(project), cause: principal::ledger::Cause::PinRestore, after, .. } }
                    if *publication == transaction.publication && *project == transaction.project && *after == expected => Some(*attempt),
                _ => None,
            }).ok_or("successful restoration omitted its captured prior ledger write")?;
            if !interval.iter().any(|event| matches!(event, principal::Event::Ledger { change: principal::ledger::Event::Renamed { attempt: actual } } if *actual == attempt))
                || !interval.iter().any(|event| matches!(event, principal::Event::Ledger { change: principal::ledger::Event::Finished { attempt: actual, succeeded: true } } if *actual == attempt)) {
                return Err("restoration ledger write did not finish inside this retry");
            }
        }
        principal::ledger::Prior::Absent => {
            if !attempts.values().any(|attempt| attempt.publication == transaction.publication
                && attempt.position > transaction.restore_position && attempt.finished == Some(true)
                && (attempt.absent || attempt.removed == Some(true))
                && matches!(attempt.operation, Operation::Project { project, purpose: crate::metrics::publication::Replacement::PinRestore, cause: principal::ledger::Cause::PinRestore } if project == transaction.project)) {
                return Err("successful restoration omitted its prior-absence retirement outcome");
            }
        }
        principal::ledger::Prior::Unknown => return Err("pin restoration has no captured predecessor"),
    }
    Ok(())
}

fn matches_entry(reference: &Reference, entry: &Consumed) -> bool {
    reference.kind == entry.kind
        && reference.schema == entry.schema
        && match reference.class {
            1 => reference.key == entry.action,
            2 => reference.key == entry.content,
            3 => reference.key == entry.content && entry.bundled,
            _ => false,
        }
}

/// Stream the already-produced typed rows into both existing canonical domains.
/// No evidence payload copy or filesystem reread is needed.
fn canonical(record: &Record<'_>) -> Result<(), &'static str> {
    let identity = record.identity;
    if record.references.len() as u64 != identity.references {
        return Err("pin references are incomplete");
    }
    let role = match identity.role {
        Role::Live => 1,
        Role::LastGood => 2,
        Role::Prepared => 3,
        Role::InFlight => 4,
    };
    let mut bytes = Sha256::new();
    bytes.update(b"RAWRPINR");
    bytes.update(1u16.to_le_bytes());
    bytes.update([role]);
    bytes.update([0; 5]);
    bytes.update(identity.project);
    bytes.update(identity.owner);
    bytes.update(identity.proof);
    bytes.update((identity.references as u32).to_le_bytes());
    bytes.update([0; 4]);
    let mut owner = Sha256::new();
    owner.update(b"rawr.store.pin-owner/v1");
    owner.update(identity.project);
    owner.update([role]);
    owner.update(identity.proof);
    owner.update(identity.references.to_le_bytes());
    for reference in &record.references {
        let mut row = [0; 96];
        row[0] = reference.class;
        row[1] = reference.kind;
        row[4..8].copy_from_slice(&reference.schema.to_le_bytes());
        row[8..40].copy_from_slice(&reference.key);
        row[40..48].copy_from_slice(&reference.bytes.to_le_bytes());
        row[48..56].copy_from_slice(&reference.allocations.to_le_bytes());
        row[56..64].copy_from_slice(&reference.members.to_le_bytes());
        row[64..].copy_from_slice(&reference.record);
        bytes.update(row);
        owner.update(row);
    }
    if <[u8; 32]>::from(bytes.finalize()) != identity.record.sha256
        || <[u8; 32]>::from(owner.finalize()) != identity.owner
    {
        return Err("typed pin references do not commit to the actual record");
    }
    Ok(())
}
