//! Checks creation occurrences against the existing visited parent endpoints.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::{Control, Directory, Event, Outcome, Source, Sync, Target};
use crate::metrics::cell::project::Manifest;
use crate::metrics::principal::{self, Evidence};

#[derive(Debug, Serialize)]
pub struct Joined {
    pub attempts: Vec<Attempt>,
    pub endpoints: Vec<Endpoint>,
}

#[derive(Debug, Serialize)]
pub struct Attempt {
    pub pid: u32,
    pub token: u64,
    pub publication: Option<u64>,
    pub target: Target,
    pub source: Source,
    pub creation: Option<Outcome>,
    pub created_at: Option<usize>,
    pub adopted: Option<Metadata>,
    pub file_sync: Option<bool>,
    pub parent_sync: Option<bool>,
    pub succeeded: Option<bool>,
}

/// Reached metadata validation establishes shape, length and mode, not bytes.
#[derive(Debug, Serialize)]
pub struct Metadata {
    pub bytes: Option<u64>,
    pub mode: u32,
}

/// Creation candidates explain presence and, for files, produced empty content.
/// Modes remain endpoint facts; these operations do not attest a requested mode.
#[derive(Debug, Serialize)]
pub struct Endpoint {
    pub path: Box<[u8]>,
    pub directory: bool,
    pub mode: u32,
    pub initial_mode: Option<u32>,
    pub initial_contents: bool,
    pub creations: Vec<(u32, u64)>,
    pub adoptions: Vec<(u32, u64)>,
}

pub fn pair(
    processes: [(u32, &Evidence); 2],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
    retirements: &super::super::retirement::join::Joined,
) -> Result<Joined, &'static str> {
    if processes[0].0 == 0 || processes[1].0 == 0 || processes[0].0 == processes[1].0 {
        return Err("creation join requires two distinct process owners");
    }
    join(&processes, root, before, after, retirements)
}

pub(in crate::metrics::principal) fn join(
    processes: &[(u32, &Evidence)],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
    retirements: &super::super::retirement::join::Joined,
) -> Result<Joined, &'static str> {
    let mut attempts = Vec::new();
    for (pid, evidence) in processes.iter().copied() {
        if evidence.fault.is_some() || !evidence.cell_transport {
            return Err("creation observation is incomplete");
        }
        let mut current = BTreeMap::<u64, Attempt>::new();
        for (event_index, event) in evidence.events.as_slice().iter().enumerate() {
            let principal::Event::Creation { change } = event else {
                continue;
            };
            match change {
                Event::Started {
                    attempt,
                    publication,
                    root: observed_root,
                    relative,
                    target,
                    source,
                } => {
                    let anchor = if target.cargo_home() {
                        let parent = root.parent().ok_or("store root has no parent")?;
                        if root.file_name() != Some(std::ffi::OsStr::new("v1"))
                            || parent.file_name() != Some(std::ffi::OsStr::new("rawr"))
                        {
                            return Err("Cargo-home creation has no matching store-root anchor");
                        }
                        parent
                            .parent()
                            .ok_or("store root has no Cargo-home anchor")?
                    } else {
                        root
                    };
                    let path = target.relative().ok_or("creation target namespace")?;
                    if *attempt == 0
                        || current.contains_key(attempt)
                        || *observed_root != principal::path_digest(anchor)
                        || *relative != principal::path_digest(&path)
                        || !super::compatible(*target, *source, *publication)
                    {
                        return Err("creation target, source or anchor differs");
                    }
                    current.insert(
                        *attempt,
                        Attempt {
                            pid,
                            token: *attempt,
                            publication: *publication,
                            target: *target,
                            source: *source,
                            creation: None,
                            created_at: None,
                            adopted: None,
                            file_sync: None,
                            parent_sync: None,
                            succeeded: None,
                        },
                    );
                }
                Event::Create { attempt, outcome } => {
                    let state = current
                        .get_mut(attempt)
                        .ok_or("creation result precedes attempt")?;
                    if state.creation.replace(*outcome).is_some()
                        || state.adopted.is_some()
                        || state.succeeded.is_some()
                        || (state.source == Source::Keyed && state.publication.is_none())
                    {
                        return Err("creation result order or publication differs");
                    }
                    if *outcome == Outcome::Created {
                        state.created_at = Some(event_index);
                    }
                }
                Event::Adopted {
                    attempt,
                    mode,
                    bytes,
                } => {
                    let state = current
                        .get_mut(attempt)
                        .ok_or("adoption precedes attempt")?;
                    if state.adopted.is_some()
                        || state.succeeded.is_some()
                        || state.file_sync.is_some()
                        || state.parent_sync.is_some()
                        || *mode > 0o7777
                        || bytes.is_none() != state.target.directory()
                        || matches!(state.creation, Some(Outcome::Created | Outcome::Failed))
                        || (state.creation == Some(Outcome::Contended)
                            && !matches!(
                                state.source,
                                Source::Bootstrap
                                    | Source::Fixed
                                    | Source::Directory
                                    | Source::CargoHome
                            ))
                        || state.source == Source::OwnershipDirectory
                    {
                        return Err("adoption shape or order differs");
                    }
                    state.adopted = Some(Metadata {
                        bytes: *bytes,
                        mode: *mode,
                    });
                }
                Event::Synced {
                    attempt,
                    scope,
                    succeeded,
                } => {
                    let state = current
                        .get_mut(attempt)
                        .ok_or("creation sync precedes attempt")?;
                    if state.creation != Some(Outcome::Created)
                        || state.adopted.is_some()
                        || state.succeeded.is_some()
                        || matches!(state.source, Source::CargoHome | Source::Directory)
                    {
                        return Err("creation sync has no matching create");
                    }
                    match scope {
                        Sync::File => {
                            if state.source != Source::Migration
                                || state.file_sync.replace(*succeeded).is_some()
                                || state.parent_sync.is_some()
                            {
                                return Err("control file sync order differs");
                            }
                        }
                        Sync::Parent => {
                            if state.parent_sync.replace(*succeeded).is_some()
                                || (state.source == Source::Migration
                                    && state.file_sync != Some(true))
                            {
                                return Err("control parent sync order differs");
                            }
                        }
                    }
                }
                Event::Finished { attempt, succeeded } => {
                    let state = current
                        .get_mut(attempt)
                        .ok_or("creation completion precedes attempt")?;
                    if state.succeeded.replace(*succeeded).is_some() {
                        return Err("creation completion repeated");
                    }
                    if *succeeded && !successful(state) {
                        return Err("creation success omits reached adoption or sync");
                    }
                }
            }
        }
        if current.values().any(|state| state.succeeded.is_none()) {
            return Err("creation attempt has no final result");
        }
        attempts.extend(current.into_values());
    }
    let mut endpoints = Vec::new();
    for row in &after.members {
        let path = raw_path(&row.path);
        let Some(control) = Control::parse(path) else {
            continue;
        };
        let target = Target::Control(control);
        let creations = candidates(&attempts, target, true, &row.path, retirements);
        let adoptions = candidates(&attempts, target, false, &row.path, retirements);
        let initial = before
            .members
            .iter()
            .find(|initial| initial.path == row.path);
        let initial_contents = initial.is_some_and(|initial| {
            initial.bytes == row.bytes
                && initial.digest == row.digest
                && (processes.len() > 1 || initial.mode == row.mode)
        });
        let empty = principal::Commitment::produced(&[]);
        if !initial_contents
            && (creations.is_empty() || row.bytes != 0 || row.digest != empty.sha256)
        {
            return Err("control bytes have no initial content or actual empty creation");
        }
        endpoints.push(Endpoint {
            path: row.path.clone(),
            directory: false,
            mode: row.mode,
            initial_mode: initial.map(|row| row.mode),
            initial_contents,
            creations,
            adoptions,
        });
    }
    for row in &after.directories {
        let target = if row.path.is_empty() {
            Target::StoreRoot
        } else if let Some(directory) = Directory::parse(raw_path(&row.path)) {
            Target::Directory(directory)
        } else {
            continue;
        };
        let creations = candidates(&attempts, target, true, &row.path, retirements);
        let adoptions = candidates(&attempts, target, false, &row.path, retirements);
        let initial = before
            .directories
            .iter()
            .find(|initial| initial.path == row.path);
        if (initial.is_none()
            || processes.len() == 1 && initial.is_some_and(|initial| initial != row))
            && creations.is_empty()
        {
            return Err("directory endpoint has no initial presence or actual creation");
        }
        endpoints.push(Endpoint {
            path: row.path.clone(),
            directory: true,
            mode: row.mode,
            initial_mode: initial.map(|row| row.mode),
            initial_contents: initial.is_some(),
            creations,
            adoptions,
        });
    }
    Ok(Joined {
        attempts,
        endpoints,
    })
}

fn successful(state: &Attempt) -> bool {
    if state.adopted.is_some() {
        return true;
    }
    if state.creation != Some(Outcome::Created) {
        return false;
    }
    match state.source {
        Source::Directory | Source::CargoHome => {
            state.file_sync.is_none() && state.parent_sync.is_none()
        }
        Source::Migration => state.file_sync == Some(true) && state.parent_sync == Some(true),
        _ => state.file_sync.is_none() && state.parent_sync == Some(true),
    }
}
fn candidates(
    attempts: &[Attempt],
    target: Target,
    created: bool,
    path: &[u8],
    retirements: &super::super::retirement::join::Joined,
) -> Vec<(u32, u64)> {
    attempts
        .iter()
        .filter(|state| {
            state.target == target
                && if created {
                    state.creation == Some(Outcome::Created)
                        && state
                            .created_at
                            .is_some_and(|index| !retirements.removed_after(state.pid, index, path))
                } else {
                    state.adopted.is_some()
                }
        })
        .map(|state| (state.pid, state.token))
        .collect()
}
fn raw_path(bytes: &[u8]) -> &Path {
    use std::os::unix::ffi::OsStrExt as _;
    Path::new(std::ffi::OsStr::from_bytes(bytes))
}
