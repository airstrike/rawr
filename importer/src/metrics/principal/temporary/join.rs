//! Exact temporary occurrences reuse the already-validated source histories.

use super::{Action, Event, Form, Member, Name, Outcome, Source};
use crate::metrics::cell::project::Manifest;
use crate::metrics::principal::{self, Commitment, Evidence};
use serde::Serialize;
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct Joined {
    #[serde(skip)]
    shared: bool,
    pub attempts: Vec<Attempt>,
    pub endpoints: Vec<Endpoint>,
    /// Published leaves and fixed members, including validated later moves.
    pub published: Vec<Endpoint>,
}
#[derive(Debug, Serialize)]
pub struct Attempt {
    pub pid: u32,
    pub token: u64,
    pub source: Source,
    pub form: Form,
    pub name: Name,
    pub target: PathBuf,
    pub temporary: Option<principal::retirement::Temporary>,
    pub commitments: [Option<Commitment>; 2],
    pub steps: Vec<Step>,
    pub operation: Option<Outcome>,
    pub cleanup_exists: Option<bool>,
    pub outcome: Option<Outcome>,
    pub started: usize,
    pub finished: Option<usize>,
    #[serde(skip)]
    reserved_at: Option<usize>,
    #[serde(skip)]
    active: Option<(Action, usize)>,
    #[serde(skip)]
    next: usize,
    #[serde(skip)]
    failed: bool,
    #[serde(skip)]
    source_renamed: Option<usize>,
    #[serde(skip)]
    source_finished: usize,
    #[serde(skip)]
    source_sealed: Option<usize>,
    #[serde(skip)]
    source_outcome: Option<Outcome>,
    #[serde(skip)]
    pin_succeeded: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct Step {
    pub action: Action,
    pub started: usize,
    pub finished: usize,
    pub succeeded: bool,
}
#[derive(Debug, Serialize)]
pub struct Endpoint {
    pub path: Box<[u8]>,
    pub directory: bool,
    pub present: bool,
    pub attempts: Vec<(u32, u64)>,
    /// A source seal may never be reached after an earlier completed write.
    pub unavailable_commitments: Vec<(u32, u64)>,
    /// Matching mutation in another process; no ordering or transfer is proved.
    pub unavailable_associations: Vec<(u32, u64)>,
}
struct Projection {
    target: PathBuf,
    form: Form,
    name: Name,
    commitments: [Option<Commitment>; 2],
    started: usize,
    renamed: Option<usize>,
    finished: usize,
    sealed: Option<usize>,
    outcome: Option<Outcome>,
    pin_succeeded: Option<bool>,
}

/// Source validators run first in the paired parent. This projection adds only
/// the actual entry-helper lifetime and temporary/native-step association.
pub fn pair(
    processes: [(u32, &Evidence); 2],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
    recoveries: &principal::recovery::join::Joined,
) -> Result<Joined, &'static str> {
    if processes[0].0 == 0 || processes[1].0 == 0 || processes[0].0 == processes[1].0 {
        return Err("temporary join requires distinct process owners");
    }
    join(&processes, root, before, after, recoveries)
}

pub(in crate::metrics::principal) fn join(
    processes: &[(u32, &Evidence)],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
    recoveries: &principal::recovery::join::Joined,
) -> Result<Joined, &'static str> {
    let mut joined = Joined {
        shared: processes.len() > 1,
        attempts: Vec::new(),
        endpoints: Vec::new(),
        published: Vec::new(),
    };
    let mut reserved = BTreeMap::new();
    for (pid, evidence) in processes.iter().copied() {
        if evidence.fault.is_some() || !evidence.cell_transport {
            return Err("temporary observation incomplete");
        }
        let mut tokens = BTreeMap::new();
        let mut sources = Vec::new();
        for (at, row) in evidence.events.as_slice().iter().enumerate() {
            let principal::Event::Temporary { change } = row else {
                continue;
            };
            if let Event::Started {
                attempt,
                source,
                form,
                name,
            } = *change
            {
                let projection = source_projection(evidence, source, root)?;
                if attempt == 0
                    || tokens.contains_key(&attempt)
                    || sources.contains(&source)
                    || form != projection.form
                    || name != projection.name
                    || projection.started >= at
                    || projection.finished <= at
                {
                    return Err("temporary source, form or lifetime differs");
                }
                sources.push(source);
                tokens.insert(attempt, joined.attempts.len());
                joined.attempts.push(Attempt {
                    pid,
                    token: attempt,
                    source,
                    form,
                    name,
                    target: projection.target,
                    temporary: None,
                    commitments: projection.commitments,
                    steps: Vec::new(),
                    operation: None,
                    cleanup_exists: None,
                    outcome: None,
                    started: at,
                    finished: None,
                    reserved_at: None,
                    active: None,
                    next: 0,
                    failed: false,
                    source_renamed: projection.renamed,
                    source_finished: projection.finished,
                    source_sealed: projection.sealed,
                    source_outcome: projection.outcome,
                    pin_succeeded: projection.pin_succeeded,
                });
                continue;
            }
            let token = match *change {
                Event::Started { .. } => unreachable!(),
                Event::Reserved { attempt, .. }
                | Event::Step { attempt, .. }
                | Event::Result { attempt, .. }
                | Event::Operation { attempt, .. }
                | Event::CleanupChecked { attempt, .. }
                | Event::Finished { attempt, .. } => attempt,
            };
            let state =
                &mut joined.attempts[*tokens.get(&token).ok_or("temporary event without start")?];
            if state.outcome.is_some() || at >= state.source_finished {
                return Err("temporary event outside source lifetime");
            }
            match *change {
                Event::Reserved { temporary, .. } => {
                    if state.temporary.replace(temporary).is_some()
                        || !state.steps.is_empty()
                        || state.active.is_some()
                        || state.operation.is_some()
                        || temporary.process != pid
                        || reserved
                            .insert((temporary.process, temporary.sequence), (pid, token))
                            .is_some()
                    {
                        return Err("temporary reservation association differs");
                    }
                    state.reserved_at = Some(at);
                }
                Event::Step { action, .. } => {
                    if state.temporary.is_none() || state.active.is_some() {
                        return Err("temporary native operation outside reservation");
                    }
                    if action == Action::Cleanup {
                        if state.cleanup_exists != Some(true)
                            || state
                                .steps
                                .iter()
                                .any(|step| step.action == Action::Cleanup)
                        {
                            return Err("temporary cleanup without reached existence check");
                        }
                    } else {
                        if state.operation.is_some()
                            || state.failed
                            || state.cleanup_exists.is_some()
                        {
                            return Err("temporary native operation after abort/result");
                        }
                        let expected = sequence(state.form).get(state.next).copied();
                        if expected != Some(action) {
                            return Err("temporary native operations reordered");
                        }
                    }
                    state.active = Some((action, at));
                }
                Event::Result { succeeded, .. } => {
                    let (action, started) = state
                        .active
                        .take()
                        .ok_or("temporary result without native attempt")?;
                    state.steps.push(Step {
                        action,
                        started,
                        finished: at,
                        succeeded,
                    });
                    if action != Action::Cleanup {
                        state.next += 1;
                        state.failed |= !succeeded;
                    }
                }
                Event::Operation { outcome, .. } => {
                    if state.active.is_some()
                        || state.operation.replace(outcome).is_some()
                        || state.cleanup_exists.is_some()
                    {
                        return Err("temporary operation result reordered");
                    }
                    validate_operation(state, outcome)?;
                }
                Event::CleanupChecked { exists, .. } => {
                    if state.operation.is_none()
                        || state.temporary.is_none()
                        || state.cleanup_exists.replace(exists).is_some()
                        || state.active.is_some()
                    {
                        return Err("temporary existence check reordered");
                    }
                }
                Event::Finished { outcome, .. } => {
                    if state.active.is_some() {
                        return Err("temporary omitted native result");
                    }
                    let created_directory = success(state, Action::CreateDirectory);
                    if state.temporary.is_some()
                        && (state.form == Form::Replacement || created_directory)
                        && (state.operation.is_none() || state.cleanup_exists.is_none())
                    {
                        return Err("temporary omitted operation/cleanup check");
                    }
                    if state.cleanup_exists == Some(true)
                        && !state
                            .steps
                            .iter()
                            .any(|step| step.action == Action::Cleanup)
                    {
                        return Err("existing temporary omitted cleanup attempt");
                    }
                    let expected = if state
                        .steps
                        .iter()
                        .any(|step| step.action == Action::Cleanup && !step.succeeded)
                    {
                        Outcome::Failed
                    } else {
                        state.operation.unwrap_or(Outcome::Failed)
                    };
                    if outcome != expected
                        || state.source_outcome.is_some_and(|source| source != outcome)
                        || outcome == Outcome::Failed && state.pin_succeeded == Some(true)
                    {
                        return Err("temporary final outcome changed cleanup error precedence");
                    }
                    let renamed = state
                        .steps
                        .iter()
                        .find(|step| step.action == Action::Rename && step.succeeded);
                    if renamed.is_some() != state.source_renamed.is_some()
                        || renamed.is_some_and(|step| {
                            state
                                .source_renamed
                                .is_none_or(|source| source <= step.finished || source >= at)
                        })
                    {
                        return Err("temporary rename differs from source history");
                    }
                    if state.form == Form::Sealed {
                        if let Some(sealed) = state.source_sealed {
                            if !state.steps.iter().any(|step| {
                                step.action == Action::SyncFile(Member::Primary)
                                    && step.succeeded
                                    && step.finished < sealed
                            }) || sealed >= at
                                || state
                                    .steps
                                    .iter()
                                    .find(|step| step.action == Action::Open(Member::Allocation))
                                    .is_some_and(|step| sealed >= step.started)
                            {
                                return Err(
                                    "source seal differs from native payload/allocation order",
                                );
                            }
                        } else if state
                            .steps
                            .iter()
                            .any(|step| step.action == Action::Open(Member::Allocation))
                        {
                            return Err("allocation write omitted source seal");
                        }
                    }
                    state.outcome = Some(outcome);
                    state.finished = Some(at);
                }
                Event::Started { .. } => unreachable!(),
            }
        }
        if tokens
            .values()
            .any(|index| joined.attempts[*index].outcome.is_none())
        {
            return Err("temporary omitted final outcome");
        }
    }
    endpoints(&mut joined, before, after, recoveries)?;
    Ok(joined)
}
fn sequence(form: Form) -> &'static [Action] {
    use Action::{CreateDirectory, Open, Rename, SyncFile, SyncParent, SyncTemporary, Write};
    match form {
        Form::Directory => &[
            CreateDirectory,
            Open(Member::Primary),
            Write(Member::Primary),
            SyncFile(Member::Primary),
            SyncTemporary,
            Rename,
            SyncParent,
        ],
        Form::Sealed => &[
            CreateDirectory,
            Open(Member::Primary),
            Write(Member::Primary),
            SyncFile(Member::Primary),
            Open(Member::Allocation),
            Write(Member::Allocation),
            SyncFile(Member::Allocation),
            SyncTemporary,
            Rename,
            SyncParent,
        ],
        Form::Replacement => &[
            Open(Member::Primary),
            Write(Member::Primary),
            SyncFile(Member::Primary),
            Rename,
            SyncParent,
        ],
    }
}
fn success(state: &Attempt, action: Action) -> bool {
    state
        .steps
        .iter()
        .any(|step| step.action == action && step.succeeded)
}
fn validate_operation(state: &Attempt, outcome: Outcome) -> Result<(), &'static str> {
    match outcome {
        Outcome::Published
            if !success(state, Action::Rename)
                || !success(state, Action::SyncParent)
                || state.failed =>
        {
            Err("published temporary omitted successful rename/parent sync")
        }
        Outcome::Failed
            if !state.failed
                && !(state.form == Form::Sealed
                    && state.next == 4
                    && state.source_sealed.is_none()) =>
        {
            Err("failed temporary omitted reached native or metadata/seal failure")
        }
        Outcome::Existing => {
            if state.form == Form::Replacement || success(state, Action::Rename) {
                return Err("replacement or renamed temporary reported existing");
            }
            if state.temporary.is_none() {
                if state.form != Form::Directory || !state.steps.is_empty() {
                    return Err("early Existing reserved a temporary");
                }
            } else if !success(state, Action::SyncTemporary)
                || state
                    .steps
                    .iter()
                    .any(|step| !step.succeeded && step.action != Action::Rename)
            {
                return Err("losing temporary omitted completed candidate");
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn source_projection(
    evidence: &Evidence,
    source: Source,
    root: &Path,
) -> Result<Projection, &'static str> {
    let mut target = None;
    let mut form = None;
    let mut name = None;
    let mut commitments = [None, None];
    let mut started = None;
    let mut renamed = None;
    let mut finished = None;
    let mut sealed = None;
    let mut outcome = None;
    let mut pin_succeeded = None;
    for (at, row) in evidence.events.as_slice().iter().enumerate() {
        match (source, row) {
            (Source::Principal(token), principal::Event::Started { attempt, identity })
                if token == *attempt =>
            {
                if identity.root != principal::path_digest(root) || started.replace(at).is_some() {
                    return Err("temporary principal source identity differs");
                }
                target = identity.relative();
                (form, name) = match identity.target {
                    principal::Target::Payload { .. } => (Some(Form::Sealed), Some(Name::Payload)),
                    principal::Target::Envelope { .. } => {
                        (Some(Form::Sealed), Some(Name::Envelope))
                    }
                    principal::Target::Index { .. } => (Some(Form::Directory), Some(Name::Row)),
                };
            }
            (
                Source::Principal(token),
                principal::Event::Sealed {
                    attempt,
                    member,
                    allocation,
                },
            ) if token == *attempt => {
                commitments = [Some(*member), *allocation];
                if sealed.replace(at).is_some() {
                    return Err("duplicate source seal");
                }
            }
            (Source::Principal(token), principal::Event::Renamed { attempt })
                if token == *attempt =>
            {
                if renamed.replace(at).is_some() {
                    return Err("duplicate principal rename");
                }
            }
            (
                Source::Principal(token),
                principal::Event::Finished {
                    attempt,
                    outcome: result,
                },
            ) if token == *attempt => {
                if finished.replace(at).is_some() {
                    return Err("duplicate principal result");
                }
                outcome = Some(match result {
                    principal::Outcome::Published => Outcome::Published,
                    principal::Outcome::Existing => Outcome::Existing,
                    principal::Outcome::Failed => Outcome::Failed,
                });
            }
            (
                Source::Pin(token),
                principal::Event::Pin {
                    change:
                        principal::pin::Event::Started {
                            attempt,
                            source: record,
                            root: observed,
                            operation,
                            ..
                        },
                },
            ) if token == *attempt => {
                if *observed != principal::path_digest(root)
                    || *operation != principal::pin::Operation::Acquire
                    || started.replace(at).is_some()
                {
                    return Err("temporary pin acquisition source differs");
                }
                let record = record.ok_or("temporary pin acquisition omitted source record")?;
                for row in &evidence.events.as_slice()[..at] {
                    if let principal::Event::Pin {
                        change:
                            principal::pin::Event::Record {
                                source, identity, ..
                            },
                    } = row
                    {
                        if *source == record {
                            if target.is_some() || identity.root != *observed {
                                return Err("temporary pin record ambiguous");
                            }
                            target = identity.relative();
                            commitments[0] = Some(identity.record);
                        }
                    }
                }
                form = Some(Form::Directory);
                name = Some(Name::Root);
            }
            (
                Source::Pin(token),
                principal::Event::Pin {
                    change:
                        principal::pin::Event::Step {
                            attempt,
                            step: principal::pin::Step::Renamed,
                        },
                },
            ) if token == *attempt => {
                if renamed.replace(at).is_some() {
                    return Err("duplicate pin rename");
                }
            }
            (
                Source::Pin(token),
                principal::Event::Pin {
                    change: principal::pin::Event::Finished { attempt, succeeded },
                },
            ) if token == *attempt => {
                if finished.replace(at).is_some() {
                    return Err("duplicate pin result");
                }
                pin_succeeded = Some(*succeeded);
            }
            (
                Source::Ledger(token),
                principal::Event::Ledger {
                    change:
                        principal::ledger::Event::Started {
                            attempt,
                            root: observed,
                            target: ledger,
                            after,
                            ..
                        },
                },
            ) if token == *attempt => {
                if *observed != principal::path_digest(root) || started.replace(at).is_some() {
                    return Err("temporary ledger source differs");
                }
                target = Some(ledger.relative());
                form = Some(Form::Replacement);
                name = Some(Name::Replacement);
                commitments[0] = Some(*after);
            }
            (
                Source::Ledger(token),
                principal::Event::Ledger {
                    change: principal::ledger::Event::Renamed { attempt },
                },
            ) if token == *attempt => {
                if renamed.replace(at).is_some() {
                    return Err("duplicate ledger rename");
                }
            }
            (
                Source::Ledger(token),
                principal::Event::Ledger {
                    change: principal::ledger::Event::Finished { attempt, succeeded },
                },
            ) if token == *attempt => {
                if finished.replace(at).is_some() {
                    return Err("duplicate ledger result");
                }
                outcome = Some(if *succeeded {
                    Outcome::Published
                } else {
                    Outcome::Failed
                });
            }
            _ => {}
        }
    }
    Ok(Projection {
        target: target.ok_or("temporary source target unavailable")?,
        form: form.ok_or("temporary source form unavailable")?,
        name: name.ok_or("temporary source member unavailable")?,
        commitments,
        started: started.ok_or("temporary source start unavailable")?,
        renamed,
        finished: finished.ok_or("temporary source end unavailable")?,
        sealed,
        outcome,
        pin_succeeded,
    })
}
fn endpoints(
    joined: &mut Joined,
    before: &Manifest,
    after: &Manifest,
    recoveries: &principal::recovery::join::Joined,
) -> Result<(), &'static str> {
    for state in &joined.attempts {
        let Some(temporary) = state.temporary else {
            continue;
        };
        let prefix = temporary.relative();
        let occurred = state
            .steps
            .iter()
            .filter(|step| {
                step.action == Action::CreateDirectory
                    || step.action == Action::Open(Member::Primary)
            })
            .map(|step| step.finished)
            .max()
            .or(state.reserved_at)
            .ok_or("temporary reservation lost its occurrence")?;
        let displaced = success(state, Action::Rename);
        let cleaned = success(state, Action::Cleanup);
        let later_recovery = recoveries.temporaries.iter().any(|recovered| {
            recovered.pid == state.pid
                && recovered.path.as_ref() == prefix.as_os_str().as_encoded_bytes()
                && recovered.removed == Some(true)
                && recovered.removed_at.is_some_and(|at| at > occurred)
        });
        for row in &after.directories {
            let path = Path::new(std::ffi::OsStr::from_bytes(&row.path));
            if !path.starts_with(&prefix) {
                continue;
            }
            if displaced || cleaned || later_recovery {
                return Err("retired publication temporary retains directory endpoint");
            }
            let initial = before
                .directories
                .iter()
                .any(|prior| prior.path == row.path);
            if path != prefix
                || state.form == Form::Replacement
                || !initial && !success(state, Action::CreateDirectory)
            {
                return Err("publication temporary directory lacks actual creation");
            }
            joined.endpoints.push(Endpoint {
                path: row.path.clone(),
                directory: true,
                present: true,
                attempts: vec![(state.pid, state.token)],
                unavailable_commitments: Vec::new(),
                unavailable_associations: Vec::new(),
            });
        }
        for row in &after.members {
            let path = Path::new(std::ffi::OsStr::from_bytes(&row.path));
            if !path.starts_with(&prefix) {
                continue;
            }
            if displaced || cleaned || later_recovery {
                return Err("retired publication temporary retains file endpoint");
            }
            let member = if state.form == Form::Replacement && path == prefix {
                Some(Member::Primary)
            } else if state
                .name
                .file()
                .is_some_and(|name| path == prefix.join(name))
            {
                Some(Member::Primary)
            } else if state.form == Form::Sealed && path == prefix.join("allocation") {
                Some(Member::Allocation)
            } else {
                None
            };
            let initial = before.members.iter().any(|prior| prior == row);
            let Some(member) = member else {
                if initial {
                    continue;
                }
                return Err("publication temporary has unobserved member");
            };
            if !success(state, Action::Open(member)) {
                if initial {
                    continue;
                }
                return Err("publication temporary member lacks exclusive-open success");
            }
            let written = success(state, Action::Write(member));
            let commitment = state.commitments[usize::from(member == Member::Allocation)];
            if written
                && commitment.is_some_and(|commitment| {
                    commitment.bytes != row.bytes || commitment.sha256 != row.digest
                })
            {
                return Err("retained temporary differs from completed write");
            }
            let mut unavailable = Vec::new();
            if !written || commitment.is_none() {
                unavailable.push((state.pid, state.token));
            }
            joined.endpoints.push(Endpoint {
                path: row.path.clone(),
                directory: false,
                present: true,
                attempts: vec![(state.pid, state.token)],
                unavailable_commitments: unavailable,
                unavailable_associations: Vec::new(),
            });
        }
        // The helper's exists result is historical, not an assertion about the
        // later endpoint. Presence obligations begin only at successful create.
        let created = if state.form == Form::Replacement {
            success(state, Action::Open(Member::Primary))
        } else {
            success(state, Action::CreateDirectory)
        };
        let removal_attempt = state
            .steps
            .iter()
            .any(|step| step.action == Action::Cleanup)
            || recoveries.temporaries.iter().any(|recovered| {
                recovered.pid == state.pid
                    && recovered.path.as_ref() == prefix.as_os_str().as_encoded_bytes()
                    && recovered.removal_started
                    && recovered.removed_at.is_some_and(|at| at > occurred)
            });
        let present = if state.form == Form::Replacement {
            after
                .members
                .iter()
                .any(|row| row.path.as_ref() == prefix.as_os_str().as_encoded_bytes())
        } else {
            after
                .directories
                .iter()
                .any(|row| row.path.as_ref() == prefix.as_os_str().as_encoded_bytes())
        };
        if created && !present && !displaced && !removal_attempt {
            return Err("created publication temporary disappeared without rename/removal");
        }
        if !displaced && !removal_attempt {
            for member in [Member::Primary, Member::Allocation] {
                if !success(state, Action::Open(member)) {
                    continue;
                }
                let path = match member {
                    Member::Primary if state.form == Form::Replacement => prefix.clone(),
                    Member::Primary => prefix.join(
                        state
                            .name
                            .file()
                            .ok_or("temporary member filename unavailable")?,
                    ),
                    Member::Allocation => prefix.join("allocation"),
                };
                if !after
                    .members
                    .iter()
                    .any(|row| row.path.as_ref() == path.as_os_str().as_encoded_bytes())
                {
                    return Err("opened temporary member disappeared without rename/removal");
                }
            }
        }
        for (path, directory) in before
            .members
            .iter()
            .map(|row| (row.path.as_ref(), false))
            .chain(
                before
                    .directories
                    .iter()
                    .map(|row| (row.path.as_ref(), true)),
            )
        {
            let native = Path::new(std::ffi::OsStr::from_bytes(path));
            if !native.starts_with(&prefix) {
                continue;
            }
            let present = if directory {
                after
                    .directories
                    .iter()
                    .any(|row| row.path.as_ref() == path)
            } else {
                after.members.iter().any(|row| row.path.as_ref() == path)
            };
            if present {
                continue;
            }
            if !displaced && !removal_attempt {
                return Err("initial publication-temporary endpoint disappeared without mutation");
            }
            joined.endpoints.push(Endpoint {
                path: path.into(),
                directory,
                present: false,
                attempts: vec![(state.pid, state.token)],
                unavailable_commitments: Vec::new(),
                unavailable_associations: Vec::new(),
            });
        }
    }
    Ok(())
}

impl Joined {
    /// Compose the actual helper rename with validated same-process mutations.
    pub fn compose(
        &mut self,
        before: &Manifest,
        after: &Manifest,
        retirements: &principal::retirement::join::Joined,
        pins: &principal::pin::join::Joined,
        recoveries: &principal::recovery::join::Joined,
    ) -> Result<(), &'static str> {
        self.published.clear();
        for state in &self.attempts {
            let Some(renamed) = rename(state) else {
                continue;
            };
            let next = self
                .attempts
                .iter()
                .filter(|next| next.pid == state.pid && next.target == state.target)
                .filter_map(rename)
                .filter(|at| *at > renamed)
                .min();
            let retired = retirements
                .attempts
                .iter()
                .filter(|retired| retired.pid == state.pid)
                .filter(|retired| {
                    retired
                        .target
                        .relative()
                        .is_some_and(|path| state.target.starts_with(path))
                })
                .filter_map(|retired| {
                    retired
                        .renamed
                        .or(retired.removal_started)
                        .filter(|at| *at > renamed)
                        .map(|at| (at, retired))
                })
                .min_by_key(|(at, _)| *at);
            let pin = pins
                .staged
                .iter()
                .filter(|pin| {
                    pin.pid == state.pid
                        && state.target.starts_with(&pin.original)
                        && pin.relocated > renamed
                })
                .min_by_key(|pin| pin.relocated);
            let retired_at = retired
                .map(|(at, _)| at)
                .into_iter()
                .chain(pin.map(|pin| pin.relocated))
                .min();
            if next.is_some_and(|next| retired_at.is_none_or(|retired| next < retired)) {
                continue;
            }
            let mut path = Some(state.target.clone());
            let mut event = renamed;
            let mut commitments = state.commitments;
            let mut removal = None;
            let mut removed = None;
            let mut removed_at = None;
            let mut relocated = false;
            if let Some((at, retired)) =
                retired.filter(|(at, _)| pin.is_none_or(|pin| *at < pin.relocated))
            {
                event = at;
                removal = retired.removal_started;
                removed = retired.removal;
                removed_at = retired.removal_finished;
                if let Some(temporary) = retired.temporary {
                    let original = retired
                        .target
                        .relative()
                        .ok_or("retirement lost its typed target")?;
                    let suffix = state
                        .target
                        .strip_prefix(&original)
                        .map_err(|_| "retirement containment changed")?;
                    let destination = temporary.relative();
                    path = Some(if suffix.as_os_str().is_empty() {
                        destination
                    } else {
                        destination.join(suffix)
                    });
                    relocated = true;
                    let allocation_matches = state.commitments[1].is_some_and(|known| {
                        retired
                            .facts
                            .allocation
                            .is_some_and(|actual| actual.record == known.sha256)
                    });
                    if allocation_matches
                        && retired
                            .facts
                            .member
                            .zip(state.commitments[0])
                            .is_some_and(|(actual, known)| actual != known)
                    {
                        return Err("retirement member contradicts its matching allocation record");
                    }
                    // Source publication binds these exact encoded allocation
                    // bytes to its member, without a second payload read here.
                    commitments[0] = state.commitments[0].filter(|known| {
                        allocation_matches
                            || state.form != Form::Sealed && retired.facts.member == Some(*known)
                    });
                    commitments[1] = state.commitments[1].filter(|_| allocation_matches);
                } else {
                    // Direct index retirement has no destination or moved-byte proof.
                    commitments = [None, None];
                }
            } else if let Some(pin) = pin {
                event = pin.relocated;
                removal = pin.removal_started;
                removed = pin.removed;
                removed_at = pin.removal_finished;
                path = pin_destination(pin, before, after, recoveries)?;
                relocated = true;
                commitments = [
                    state.commitments[0].filter(|known| pin.record == Some(*known)),
                    None,
                ];
            }
            let Some(path) = path else {
                if removal.is_none() {
                    return Err("unremoved pin destination absent from raw endpoints");
                }
                self.published.push(Endpoint {
                    path: state.target.as_os_str().as_encoded_bytes().into(),
                    directory: state.form != Form::Replacement,
                    present: false,
                    attempts: vec![(state.pid, state.token)],
                    unavailable_commitments: vec![(state.pid, state.token)],
                    unavailable_associations: vec![(state.pid, state.token)],
                });
                continue;
            };
            let replaced = self
                .attempts
                .iter()
                .filter(|next| next.pid == state.pid && next.target == path)
                .filter_map(rename)
                .filter(|at| *at > event)
                .min();
            for later in retirements.attempts.iter().filter(|later| {
                later.pid == state.pid
                    && later.renamed.is_none()
                    && later
                        .target
                        .relative()
                        .is_some_and(|target| path.starts_with(target))
                    && later
                        .removal_started
                        .is_some_and(|at| at > event && replaced.is_none_or(|stop| at < stop))
            }) {
                removal = later.removal_started;
                if later.removal == Some(true) {
                    removed = Some(true);
                    removed_at = later.removal_finished;
                }
            }
            for recovered in recoveries.temporaries.iter().filter(|recovered| {
                recovered.pid == state.pid
                    && recovered.path.as_ref() == path.as_os_str().as_encoded_bytes()
                    && recovered.removed_at.is_some_and(|at| at > event)
            }) {
                if recovered.removal_started {
                    removal = recovered.removed_at;
                }
                if recovered.removed == Some(true) {
                    removed = Some(true);
                    removed_at = recovered.removed_at;
                }
            }
            let superseded = removed_at.is_some_and(|at| {
                recreated(state.pid, at, &path, &self.attempts, retirements, pins)
            });
            if superseded {
                continue;
            }
            let uncertain = !relocated
                && removal.is_none()
                && other_mutation(state.pid, &state.target, retirements, pins);
            let recreation = !relocated
                && other_publication(state.pid, &path, &self.attempts, retirements, pins);
            let mut members = Vec::new();
            if state.form != Form::Replacement {
                published_endpoint(
                    &mut self.published,
                    state,
                    &path,
                    true,
                    None,
                    removal.is_some(),
                    removed == Some(true),
                    false,
                    uncertain,
                    recreation,
                    after,
                )?;
                members.push((
                    path.join(state.name.file().ok_or("published member name missing")?),
                    commitments[0],
                ));
                if state.form == Form::Sealed {
                    members.push((path.join("allocation"), commitments[1]));
                }
            } else {
                members.push((path.clone(), commitments[0]));
            }
            for (member, commitment) in members {
                // Shared ledgers have candidate versions, not a per-process
                // last-writer order. Their existing join checks final bytes.
                let shared_ledger = self.shared && state.form == Form::Replacement && !relocated;
                published_endpoint(
                    &mut self.published,
                    state,
                    &member,
                    false,
                    commitment,
                    removal.is_some(),
                    removed == Some(true),
                    shared_ledger,
                    uncertain,
                    recreation,
                    after,
                )?;
            }
        }
        for directory in &after.directories {
            let path = Path::new(std::ffi::OsStr::from_bytes(&directory.path));
            if !leaf(path)
                || before.directories.iter().any(|prior| {
                    if self.shared {
                        prior.path == directory.path
                    } else {
                        prior == directory
                    }
                })
            {
                continue;
            }
            if !self.published.iter().any(|endpoint| {
                endpoint.directory && endpoint.present && endpoint.path == directory.path
            }) {
                return Err("final publication leaf has no surviving actual rename");
            }
        }
        Ok(())
    }
}

fn rename(state: &Attempt) -> Option<usize> {
    state
        .steps
        .iter()
        .find(|step| step.action == Action::Rename && step.succeeded)
        .map(|step| step.finished)
}
fn published_endpoint(
    endpoints: &mut Vec<Endpoint>,
    state: &Attempt,
    path: &Path,
    directory: bool,
    commitment: Option<Commitment>,
    removal: bool,
    removed: bool,
    shared_ledger: bool,
    uncertain: bool,
    recreation: bool,
    after: &Manifest,
) -> Result<(), &'static str> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let member = after.members.iter().find(|row| row.path.as_ref() == bytes);
    let present = if directory {
        after
            .directories
            .iter()
            .any(|row| row.path.as_ref() == bytes)
    } else {
        member.is_some()
    };
    if !present && !removal && !uncertain {
        return Err("published leaf or member disappeared without later mutation");
    }
    if present && removed && !recreation {
        return Err("successfully removed publication retains an endpoint");
    }
    let mut unavailable = Vec::new();
    if !directory && present && !(removed && recreation) {
        if let Some(known) = commitment {
            let matches =
                member.is_some_and(|row| row.bytes == known.bytes && row.digest == known.sha256);
            if !matches && !shared_ledger {
                return Err("published or relocated member differs from known bytes");
            }
            if !matches {
                unavailable.push((state.pid, state.token));
            }
        } else {
            unavailable.push((state.pid, state.token));
        }
    }
    endpoints.push(Endpoint {
        path: bytes.into(),
        directory,
        present,
        attempts: vec![(state.pid, state.token)],
        unavailable_commitments: unavailable,
        unavailable_associations: if !present && uncertain || present && removed && recreation {
            vec![(state.pid, state.token)]
        } else {
            Vec::new()
        },
    });
    Ok(())
}
fn pin_destination(
    pin: &principal::pin::join::Staged,
    before: &Manifest,
    after: &Manifest,
    recoveries: &principal::recovery::join::Joined,
) -> Result<Option<PathBuf>, &'static str> {
    let mut result = None;
    let recovered = recoveries
        .temporaries
        .iter()
        .filter(|recovered| {
            recovered.pid == pin.pid && recovered.removed_at.is_some_and(|at| at > pin.relocated)
        })
        .map(|recovered| recovered.path.as_ref());
    for bytes in before
        .members
        .iter()
        .map(|row| row.path.as_ref())
        .chain(after.members.iter().map(|row| row.path.as_ref()))
        .chain(before.directories.iter().map(|row| row.path.as_ref()))
        .chain(after.directories.iter().map(|row| row.path.as_ref()))
        .chain(recovered)
    {
        let path = Path::new(std::ffi::OsStr::from_bytes(bytes));
        let mut components = path.components();
        if components.next()
            != Some(std::path::Component::Normal(std::ffi::OsStr::new(
                "temporaries",
            )))
        {
            continue;
        }
        let Some(std::path::Component::Normal(name)) = components.next() else {
            continue;
        };
        let candidate = Path::new("temporaries").join(name);
        if principal::path_digest(&candidate) != pin.relative {
            continue;
        }
        if result.as_ref().is_some_and(|prior| prior != &candidate) {
            return Err("pin destination commitment is ambiguous");
        }
        result = Some(candidate);
    }
    Ok(result)
}
fn recreated(
    pid: u32,
    at: usize,
    path: &Path,
    attempts: &[Attempt],
    retirements: &principal::retirement::join::Joined,
    pins: &principal::pin::join::Joined,
) -> bool {
    attempts.iter().any(|next| {
        next.pid == pid && next.target == path && rename(next).is_some_and(|next| next > at)
    }) || retirements.attempts.iter().any(|next| {
        next.pid == pid
            && next.renamed.is_some_and(|next| next > at)
            && next
                .temporary
                .is_some_and(|temporary| temporary.relative() == path)
    }) || pins.staged.iter().any(|next| {
        next.pid == pid && next.relocated > at && next.relative == principal::path_digest(path)
    })
}
fn leaf(path: &Path) -> bool {
    if matches!(
        principal::retirement::Target::parse(path),
        Some(
            principal::retirement::Target::Action { .. }
                | principal::retirement::Target::Content { .. }
                | principal::retirement::Target::IndexRow { .. }
        )
    ) {
        return true;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    [
        "pins/live",
        "pins/last-good",
        "pins/prepared",
        "pins/in-flight",
    ]
    .iter()
    .any(|role| parent == Path::new(role))
        && path
            .file_name()
            .is_some_and(|name| principal::retirement::digest(name.as_encoded_bytes()).is_some())
}

pub(in crate::metrics::principal) fn other_mutation(
    pid: u32,
    path: &Path,
    retirements: &principal::retirement::join::Joined,
    pins: &principal::pin::join::Joined,
) -> bool {
    retirements.attempts.iter().any(|attempt| {
        attempt.pid != pid
            && (attempt.renamed.is_some() || attempt.removal_started.is_some())
            && attempt
                .target
                .relative()
                .is_some_and(|target| path.starts_with(target))
    }) || pins
        .staged
        .iter()
        .any(|pin| pin.pid != pid && path.starts_with(&pin.original))
}

fn other_publication(
    pid: u32,
    path: &Path,
    attempts: &[Attempt],
    retirements: &principal::retirement::join::Joined,
    pins: &principal::pin::join::Joined,
) -> bool {
    attempts.iter().any(|candidate| {
        candidate.pid != pid
            && candidate.target == path
            && rename(candidate).is_some_and(|at| {
                !retirements.attempts.iter().any(|retired| {
                    retired.pid == candidate.pid
                        && retired
                            .target
                            .relative()
                            .is_some_and(|target| path.starts_with(target))
                        && (retired.renamed.is_some_and(|event| event > at)
                            || retired.removal == Some(true)
                                && retired.removal_finished.is_some_and(|event| event > at))
                }) && !pins.staged.iter().any(|pin| {
                    pin.pid == candidate.pid
                        && pin.relocated > at
                        && path.starts_with(&pin.original)
                })
            })
    })
}
