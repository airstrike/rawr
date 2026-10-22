//! Recovery occurrences joined to visited endpoints, not an inode snapshot.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use serde::Serialize;

use super::{Event, Origin};
use crate::metrics::cell::project::Manifest;
use crate::metrics::principal::{self, Commitment, Evidence};

#[derive(Debug, Serialize)]
pub struct Joined {
    pub batches: Vec<Batch>,
    pub temporaries: Vec<Temporary>,
    pub modes: Vec<Mode>,
    pub missing: Vec<Endpoint>,
    pub retained: Vec<Endpoint>,
}
#[derive(Debug, Serialize)]
pub struct Batch {
    pub pid: u32,
    pub token: u64,
    pub publication: u64,
    pub measured: Vec<Measurement>,
    pub ready: Option<u64>,
    pub started: u64,
    pub synced: Option<bool>,
    pub finished: bool,
    pub recovered: Option<bool>,
    #[serde(skip)]
    active: Option<u64>,
    #[serde(skip)]
    failed: bool,
}
#[derive(Debug, Serialize)]
pub struct Measurement {
    pub path: Box<[u8]>,
    pub directory: bool,
    pub bytes: u64,
    pub members: u64,
    pub origin: Origin,
}
#[derive(Debug, Serialize)]
pub struct Temporary {
    pub pid: u32,
    pub token: u64,
    pub batch: u64,
    pub index: u64,
    pub path: Box<[u8]>,
    pub directory: bool,
    pub members: u64,
    pub thaw_started: bool,
    pub thawed: Option<bool>,
    pub modes: u64,
    pub removal_started: bool,
    pub removed: Option<bool>,
    pub removed_at: Option<usize>,
    pub finished: Option<bool>,
    #[serde(skip)]
    active_mode: Option<u64>,
    #[serde(skip)]
    mode_failed: bool,
}
#[derive(Debug, Serialize)]
pub struct Mode {
    pub pid: u32,
    pub token: u64,
    pub attempt: u64,
    pub path: Box<[u8]>,
    pub directory: bool,
    pub before: u32,
    pub requested: u32,
    pub succeeded: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct Endpoint {
    pub path: Box<[u8]>,
    pub directory: bool,
    pub mode: u32,
    pub attempts: Vec<(u32, u64)>,
    /// Successful requests whose requested bits match this visited final mode.
    /// These are candidates, not post-write stats or cross-process last writers.
    pub matching_modes: Vec<(u32, u64)>,
}
struct PathRows {
    token: u64,
    commitment: Commitment,
    bytes: Vec<u8>,
}

pub fn pair(
    processes: [(u32, &Evidence); 2],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
    occurrences: (
        &principal::retirement::join::Joined,
        &principal::pin::join::Joined,
    ),
) -> Result<Joined, &'static str> {
    if processes[0].0 == 0 || processes[1].0 == 0 || processes[0].0 == processes[1].0 {
        return Err("recovery join requires distinct process owners");
    }
    join(&processes, root, before, after, occurrences)
}

pub(in crate::metrics::principal) fn join(
    processes: &[(u32, &Evidence)],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
    occurrences: (
        &principal::retirement::join::Joined,
        &principal::pin::join::Joined,
    ),
) -> Result<Joined, &'static str> {
    let root = principal::path_digest(root);
    let mut joined = Joined {
        batches: Vec::new(),
        temporaries: Vec::new(),
        modes: Vec::new(),
        missing: Vec::new(),
        retained: Vec::new(),
    };
    for (pid, evidence) in processes.iter().copied() {
        if evidence.fault.is_some() || !evidence.cell_transport {
            return Err("recovery observation is incomplete");
        }
        let mut batches = BTreeMap::<u64, Batch>::new();
        let mut temporaries = BTreeMap::<u64, Temporary>::new();
        let mut modes = BTreeMap::<u64, Mode>::new();
        let mut path_tokens = BTreeSet::new();
        let mut pending = None::<PathRows>;
        for (position, event) in evidence.events.as_slice().iter().enumerate() {
            let principal::Event::Recovery { change } = event else {
                continue;
            };
            if pending.is_some()
                && !matches!(
                    change,
                    Event::Bytes { .. } | Event::Measured { .. } | Event::Mode { .. }
                )
            {
                return Err("recovery path rows are unbound or interrupted");
            }
            match change {
                Event::Batch {
                    batch,
                    publication,
                    root: actual_root,
                } => {
                    if *batch == 0
                        || *publication == 0
                        || *actual_root != root
                        || batches.contains_key(batch)
                        || batches.values().any(|value| !value.finished)
                    {
                        return Err("recovery batch ownership or order differs");
                    }
                    batches.insert(
                        *batch,
                        Batch {
                            pid,
                            token: *batch,
                            publication: *publication,
                            measured: Vec::new(),
                            ready: None,
                            started: 0,
                            synced: None,
                            finished: false,
                            recovered: None,
                            active: None,
                            failed: false,
                        },
                    );
                }
                Event::Path { path, commitment } => {
                    let length =
                        usize::try_from(commitment.bytes).map_err(|_| "recovery path length")?;
                    if *path == 0
                        || length == 0
                        || length > crate::metrics::cell::REPORT_BYTES
                        || !path_tokens.insert(*path)
                    {
                        return Err("recovery path frame or identity differs");
                    }
                    let mut bytes = Vec::new();
                    bytes
                        .try_reserve_exact(length)
                        .map_err(|_| "recovery path allocation")?;
                    pending = Some(PathRows {
                        token: *path,
                        commitment: *commitment,
                        bytes,
                    });
                }
                Event::Bytes {
                    path,
                    index,
                    used,
                    bytes,
                } => {
                    let state = pending
                        .as_mut()
                        .ok_or("recovery path bytes have no header")?;
                    let remaining = (state.commitment.bytes as usize)
                        .checked_sub(state.bytes.len())
                        .ok_or("recovery path overrun")?;
                    if *path != state.token
                        || *index != (state.bytes.len() / 32) as u64
                        || remaining == 0
                        || *used as usize != remaining.min(32)
                        || bytes[*used as usize..].iter().any(|byte| *byte != 0)
                    {
                        return Err("recovery path bytes reordered, repeated or truncated");
                    }
                    state.bytes.extend_from_slice(&bytes[..*used as usize]);
                }
                Event::Measured {
                    batch,
                    index,
                    path,
                    directory,
                    bytes,
                    members,
                    origin,
                } => {
                    let state = batches
                        .get_mut(batch)
                        .ok_or("recovery measurement has no batch")?;
                    let path = take_path(&mut pending, *path)?;
                    if state.finished
                        || state.ready.is_some()
                        || *index != state.measured.len() as u64
                        || *members == 0
                        || (!*directory && *members != 1)
                        || !temporary_path(&path, true)
                        || state
                            .measured
                            .last()
                            .is_some_and(|prior| prior.path >= path)
                    {
                        return Err("recovery measurement identity, geometry or order differs");
                    }
                    state.measured.push(Measurement {
                        path,
                        directory: *directory,
                        bytes: *bytes,
                        members: *members,
                        origin: *origin,
                    });
                }
                Event::Ready { batch, count } => {
                    let state = batches
                        .get_mut(batch)
                        .ok_or("recovery ready has no batch")?;
                    if state.finished
                        || state.ready.replace(*count).is_some()
                        || *count != state.measured.len() as u64
                    {
                        return Err("recovery ready count differs");
                    }
                }
                Event::Started {
                    batch,
                    attempt,
                    index,
                } => {
                    let state = batches
                        .get_mut(batch)
                        .ok_or("temporary recovery has no batch")?;
                    if *attempt == 0
                        || temporaries.contains_key(attempt)
                        || state.finished
                        || state.failed
                        || state.active.is_some()
                        || state.ready.is_none()
                        || *index != state.started
                    {
                        return Err("temporary recovery attempt order differs");
                    }
                    let measured = state
                        .measured
                        .get(*index as usize)
                        .ok_or("temporary was not measured")?;
                    temporaries.insert(
                        *attempt,
                        Temporary {
                            pid,
                            token: *attempt,
                            batch: *batch,
                            index: *index,
                            path: measured.path.clone(),
                            directory: measured.directory,
                            members: measured.members,
                            thaw_started: false,
                            thawed: None,
                            modes: 0,
                            removal_started: false,
                            removed: None,
                            removed_at: None,
                            finished: None,
                            active_mode: None,
                            mode_failed: false,
                        },
                    );
                    state.started += 1;
                    state.active = Some(*attempt);
                }
                Event::Thaw { attempt } => {
                    let state = temporaries
                        .get_mut(attempt)
                        .ok_or("thaw has no temporary")?;
                    if !state.directory
                        || state.thaw_started
                        || state.removal_started
                        || state.finished.is_some()
                    {
                        return Err("recovery thaw order differs");
                    }
                    state.thaw_started = true;
                }
                Event::Mode {
                    attempt,
                    change,
                    directory,
                    before,
                    requested,
                } => {
                    let state = temporaries
                        .get_mut(attempt)
                        .ok_or("recovery mode has no temporary")?;
                    let path = take_path(&mut pending, *change)?;
                    if modes.contains_key(change)
                        || !state.thaw_started
                        || state.thawed.is_some()
                        || state.active_mode.is_some()
                        || state.mode_failed
                        || state.finished.is_some()
                        || !raw_path(&path).starts_with(raw_path(&state.path))
                        || (path == state.path && !*directory)
                        || *requested != (*before | if *directory { 0o700 } else { 0o600 })
                        || modes
                            .values()
                            .any(|mode| mode.attempt == *attempt && mode.path == path)
                    {
                        return Err("recovery mode target, request or order differs");
                    }
                    state.modes += 1;
                    if state.modes > state.members {
                        return Err("recovery thaw exceeds measured members");
                    }
                    state.active_mode = Some(*change);
                    modes.insert(
                        *change,
                        Mode {
                            pid,
                            token: *change,
                            attempt: *attempt,
                            path,
                            directory: *directory,
                            before: *before,
                            requested: *requested,
                            succeeded: None,
                        },
                    );
                }
                Event::ModeResult { change, succeeded } => {
                    let mode = modes
                        .get_mut(change)
                        .ok_or("recovery mode result has no request")?;
                    let state = temporaries
                        .get_mut(&mode.attempt)
                        .ok_or("recovery mode lost its temporary")?;
                    if mode.succeeded.replace(*succeeded).is_some()
                        || state.active_mode.take() != Some(*change)
                        || state.finished.is_some()
                    {
                        return Err("recovery mode result order differs");
                    }
                    state.mode_failed = !*succeeded;
                }
                Event::Thawed { attempt, succeeded } => {
                    let state = temporaries
                        .get_mut(attempt)
                        .ok_or("recovery thaw result has no attempt")?;
                    if !state.thaw_started
                        || state.thawed.replace(*succeeded).is_some()
                        || state.active_mode.is_some()
                        || state.finished.is_some()
                        || (*succeeded && (state.mode_failed || state.modes != state.members))
                    {
                        return Err("recovery thaw result omits reached work");
                    }
                }
                Event::RemovalStarted { attempt } => {
                    let state = temporaries
                        .get_mut(attempt)
                        .ok_or("recovery removal has no attempt")?;
                    if state.removal_started
                        || state.finished.is_some()
                        || (state.directory && state.thawed != Some(true))
                    {
                        return Err("recovery removal precedes complete thaw");
                    }
                    state.removal_started = true;
                }
                Event::Removed { attempt, succeeded } => {
                    let state = temporaries
                        .get_mut(attempt)
                        .ok_or("recovery removal result has no attempt")?;
                    if !state.removal_started
                        || state.removed.replace(*succeeded).is_some()
                        || state.finished.is_some()
                    {
                        return Err("recovery removal result order differs");
                    }
                    state.removed_at = Some(position);
                }
                Event::Finished { attempt, succeeded } => {
                    let state = temporaries
                        .get_mut(attempt)
                        .ok_or("temporary recovery completion has no attempt")?;
                    if state.finished.replace(*succeeded).is_some()
                        || state.active_mode.is_some()
                        || (state.thaw_started && state.thawed.is_none())
                        || (state.removal_started && state.removed.is_none())
                        || (*succeeded && state.removed != Some(true))
                    {
                        return Err("temporary recovery completion differs");
                    }
                    let batch = batches
                        .get_mut(&state.batch)
                        .ok_or("temporary recovery lost batch")?;
                    if batch.active.take() != Some(*attempt) {
                        return Err("temporary recovery active interval differs");
                    }
                    batch.failed = !*succeeded;
                }
                Event::Synced { batch, succeeded } => {
                    let state = batches.get_mut(batch).ok_or("recovery sync has no batch")?;
                    if state.finished
                        || state.failed
                        || state.active.is_some()
                        || state.synced.replace(*succeeded).is_some()
                        || state.ready == Some(0)
                        || state.ready != Some(state.started)
                    {
                        return Err("recovery sync precedes terminal successful removals");
                    }
                }
                Event::BatchFinished { batch, recovered } => {
                    let state = batches
                        .get_mut(batch)
                        .ok_or("recovery completion has no batch")?;
                    if state.finished || state.active.is_some() {
                        return Err("recovery batch completion order differs");
                    }
                    if let Some(recovered) = recovered {
                        if state.failed
                            || state.ready != Some(state.started)
                            || *recovered != (state.started != 0)
                            || (*recovered && state.synced != Some(true))
                            || (!*recovered && state.synced.is_some())
                        {
                            return Err("recovery batch success omits removal or sync");
                        }
                    }
                    state.finished = true;
                    state.recovered = *recovered;
                }
            }
        }
        if pending.is_some()
            || batches.values().any(|batch| !batch.finished)
            || temporaries.values().any(|value| value.finished.is_none())
            || modes.values().any(|mode| mode.succeeded.is_none())
        {
            return Err("recovery history is unfinished");
        }
        joined.batches.extend(batches.into_values());
        joined.temporaries.extend(temporaries.into_values());
        joined.modes.extend(modes.into_values());
    }
    for (path, directory, mode) in rows(before) {
        if !temporary_path(path, false)
            || rows(after).any(|(other, kind, _)| other == path && kind == directory)
        {
            continue;
        }
        let attempts = joined
            .temporaries
            .iter()
            .filter(|attempt| covers(attempt, path) && attempt.removal_started)
            .map(|attempt| (attempt.pid, attempt.token))
            .collect::<Vec<_>>();
        if attempts.is_empty() {
            return Err("missing temporary endpoint has no recovery removal attempt");
        }
        joined.missing.push(Endpoint {
            path: path.into(),
            directory,
            mode,
            attempts,
            matching_modes: Vec::new(),
        });
    }
    for (path, directory, mode) in rows(after) {
        let matching = joined
            .temporaries
            .iter()
            .filter(|attempt| covers(attempt, path))
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        for attempt in &matching {
            if attempt.removed == Some(true) && !later_relocation(processes, attempt, occurrences) {
                return Err("successfully recovered temporary has retained endpoints without a later relocation");
            }
        }
        let attempts = matching
            .iter()
            .filter(|attempt| attempt.removed != Some(true))
            .map(|attempt| (attempt.pid, attempt.token))
            .collect();
        let matching_modes = joined
            .modes
            .iter()
            .filter(|change| {
                change.path.as_ref() == path
                    && change.directory == directory
                    && change.succeeded == Some(true)
                    && change.requested & 0o7777 == mode
            })
            .map(|change| (change.pid, change.token))
            .collect();
        joined.retained.push(Endpoint {
            path: path.into(),
            directory,
            mode,
            attempts,
            matching_modes,
        });
    }
    Ok(joined)
}

fn take_path(pending: &mut Option<PathRows>, token: u64) -> Result<Box<[u8]>, &'static str> {
    let value = pending
        .take()
        .ok_or("recovery operation omits its raw path")?;
    if value.token != token
        || value.commitment.bytes != value.bytes.len() as u64
        || value.commitment != Commitment::produced(&value.bytes)
        || !temporary_path(&value.bytes, false)
    {
        return Err("recovery raw path binding differs");
    }
    Ok(value.bytes.into_boxed_slice())
}
fn temporary_path(bytes: &[u8], exact: bool) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    let mut parts = bytes.split(|byte| *byte == b'/');
    if parts.next() != Some(b"temporaries".as_slice()) {
        return false;
    }
    let Some(name) = parts.next() else {
        return false;
    };
    if !crate::store::valid_temporary_name(std::ffi::OsStr::from_bytes(name)) {
        return false;
    }
    if exact {
        return parts.next().is_none();
    }
    parts.all(|part| !part.is_empty() && part != b"." && part != b"..")
}
fn raw_path(bytes: &[u8]) -> &Path {
    Path::new(std::ffi::OsStr::from_bytes(bytes))
}
fn covers(attempt: &Temporary, path: &[u8]) -> bool {
    if attempt.directory {
        raw_path(path).starts_with(raw_path(&attempt.path))
    } else {
        path == attempt.path.as_ref()
    }
}
fn rows(manifest: &Manifest) -> impl Iterator<Item = (&[u8], bool, u32)> {
    manifest
        .members
        .iter()
        .map(|row| (row.path.as_ref(), false, row.mode))
        .chain(
            manifest
                .directories
                .iter()
                .map(|row| (row.path.as_ref(), true, row.mode)),
        )
}
fn later_relocation(
    processes: &[(u32, &Evidence)],
    attempt: &Temporary,
    occurrences: (
        &principal::retirement::join::Joined,
        &principal::pin::join::Joined,
    ),
) -> bool {
    let Some(removed_at) = attempt.removed_at else {
        return false;
    };
    if occurrences.0.attempts.iter().any(|later| {
        later.pid == attempt.pid
            && later.removal != Some(true)
            && later.renamed.is_some_and(|position| position > removed_at)
            && later.temporary.is_some_and(|temporary| {
                temporary.relative().as_os_str().as_encoded_bytes() == attempt.path.as_ref()
            })
    }) {
        return true;
    }
    let digest = principal::path_digest(raw_path(&attempt.path));
    processes.iter().copied().filter(|(pid, _)| *pid == attempt.pid).any(|(_, history)| {
        let events = history.events.as_slice();
        events.iter().enumerate().skip(removed_at + 1).any(|(position, event)| match event {
            principal::Event::Pin { change: principal::pin::Event::Relocated { attempt: token, temporary } } => {
                *temporary == digest
                    && occurrences.1.staged.iter().any(|staged| staged.pid == attempt.pid && staged.attempt == *token && staged.relative == digest)
                    && !events.iter().skip(position + 1).any(|later| matches!(later,
                        principal::Event::Pin { change: principal::pin::Event::Step { attempt: actual, step: principal::pin::Step::Removed } } if actual == token))
            }
            _ => false,
        })
    })
}
