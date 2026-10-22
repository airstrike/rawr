//! Joins reached removal occurrences to visited endpoints without global order.

use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use serde::Serialize;

use super::{Cause, Event, Target, Temporary};
use crate::metrics::cell::project::Manifest;
use crate::metrics::principal::{self, Evidence};

#[derive(Debug, Serialize)]
pub struct Joined {
    pub attempts: Vec<Attempt>,
    pub missing: Vec<Endpoint>,
    pub temporaries: Vec<Retained>,
}

#[derive(Debug, Serialize)]
pub struct Attempt {
    pub pid: u32,
    pub token: u64,
    pub publication: u64,
    pub target: Target,
    pub cause: Cause,
    pub facts: super::Facts,
    pub renamed: Option<usize>,
    pub temporary: Option<Temporary>,
    pub removal_started: Option<usize>,
    pub removal: Option<bool>,
    pub removal_finished: Option<usize>,
    pub finished: Option<bool>,
    pub finished_at: Option<usize>,
}

/// Candidate mutation occurrences explain absence, not global last-writer order.
/// A failed recursive removal can have partially removed descendants.
#[derive(Debug, Serialize)]
pub struct Endpoint {
    pub path: Box<[u8]>,
    pub directory: bool,
    pub candidates: Vec<(u32, u64)>,
}

#[derive(Debug, Serialize)]
pub struct Retained {
    pub pid: u32,
    pub attempt: u64,
    pub path: Box<[u8]>,
    pub original: Box<[u8]>,
    pub directory: bool,
    pub matches_initial: bool,
}

impl Joined {
    /// Only this same process's later proven rename/removal can retire its
    /// publication obligation. Cross-process order is deliberately unavailable.
    pub(in crate::metrics::principal) fn removed_after(
        &self,
        pid: u32,
        event: usize,
        path: &[u8],
    ) -> bool {
        let path = Path::new(std::ffi::OsStr::from_bytes(path));
        self.attempts.iter().any(|attempt| {
            attempt.pid == pid
                && attempt
                    .target
                    .relative()
                    .is_some_and(|target| path.starts_with(target))
                && (attempt.renamed.is_some_and(|index| index > event)
                    || (attempt.removal == Some(true)
                        && attempt.removal_finished.is_some_and(|index| index > event)))
        })
    }
}

pub fn pair(
    processes: [(u32, &Evidence); 2],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
) -> Result<Joined, &'static str> {
    if processes[0].0 == 0 || processes[1].0 == 0 || processes[0].0 == processes[1].0 {
        return Err("retirement join requires two distinct process owners");
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
    let mut attempts = Vec::new();
    let mut temporary_owners = BTreeMap::new();
    for (pid, evidence) in processes.iter().copied() {
        if evidence.fault.is_some() || !evidence.cell_transport {
            return Err("retirement observation is incomplete");
        }
        let mut current = BTreeMap::<u64, Attempt>::new();
        for (index, event) in evidence.events.as_slice().iter().enumerate() {
            let principal::Event::Retirement { change } = event else {
                continue;
            };
            match change {
                Event::Started {
                    attempt,
                    publication,
                    root: actual_root,
                    relative,
                    target,
                    cause,
                    facts,
                } => {
                    let path = target.relative().ok_or("retirement target namespace")?;
                    if *attempt == 0
                        || *publication == 0
                        || *actual_root != root
                        || principal::path_digest(&path) != *relative
                        || !super::compatible(*target, *cause, *facts)
                        || current.contains_key(attempt)
                    {
                        return Err("retirement target, cause or ownership facts changed");
                    }
                    current.insert(
                        *attempt,
                        Attempt {
                            pid,
                            token: *attempt,
                            publication: *publication,
                            target: *target,
                            cause: *cause,
                            facts: *facts,
                            renamed: None,
                            temporary: None,
                            removal_started: None,
                            removal: None,
                            removal_finished: None,
                            finished: None,
                            finished_at: None,
                        },
                    );
                }
                Event::Relocated {
                    attempt,
                    temporary,
                    relative,
                } => {
                    let state = current
                        .get_mut(attempt)
                        .ok_or("retirement rename precedes attempt")?;
                    if direct(state.target)
                        || state.renamed.is_some()
                        || state.removal_started.is_some()
                        || state.finished.is_some()
                        || temporary.process != pid
                        || temporary.sequence == 0
                        || principal::path_digest(&temporary.relative()) != *relative
                        || temporary_owners
                            .insert((temporary.process, temporary.sequence), (pid, *attempt))
                            .is_some()
                    {
                        return Err("retirement rename association or order changed");
                    }
                    state.renamed = Some(index);
                    state.temporary = Some(*temporary);
                }
                Event::RemovalStarted { attempt } => {
                    let state = current
                        .get_mut(attempt)
                        .ok_or("removal precedes retirement attempt")?;
                    if state.removal_started.replace(index).is_some()
                        || state.finished.is_some()
                        || (!direct(state.target) && state.renamed.is_none())
                    {
                        return Err("retirement removal start order changed");
                    }
                }
                Event::RemovalFinished { attempt, succeeded } => {
                    let state = current
                        .get_mut(attempt)
                        .ok_or("removal result precedes retirement attempt")?;
                    if state.removal_started.is_none()
                        || state.removal.replace(*succeeded).is_some()
                        || state.finished.is_some()
                    {
                        return Err("retirement removal result order changed");
                    }
                    state.removal_finished = Some(index);
                }
                Event::Finished { attempt, succeeded } => {
                    let state = current
                        .get_mut(attempt)
                        .ok_or("retirement outcome precedes attempt")?;
                    if state.finished.replace(*succeeded).is_some()
                        || (state.removal_started.is_some() && state.removal.is_none())
                        || (*succeeded && state.removal != Some(true))
                    {
                        return Err("retirement outcome differs from reached operations");
                    }
                    state.finished_at = Some(index);
                }
            }
        }
        if current.values().any(|attempt| attempt.finished.is_none()) {
            return Err("retirement attempt has no final outcome");
        }
        attempts.extend(current.into_values());
    }
    let mut missing = Vec::new();
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
        let present = if directory {
            after
                .directories
                .iter()
                .any(|row| row.path.as_ref() == path)
        } else {
            after.members.iter().any(|row| row.path.as_ref() == path)
        };
        if present || !covered(path) {
            continue;
        }
        let path_value = Path::new(std::ffi::OsStr::from_bytes(path));
        let candidates = attempts
            .iter()
            .filter(|attempt| {
                (attempt.renamed.is_some() || attempt.removal_started.is_some())
                    && attempt
                        .target
                        .relative()
                        .is_some_and(|target| path_value.starts_with(target))
            })
            .map(|attempt| (attempt.pid, attempt.token))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err("removed endpoint has no observed non-pin mutation");
        }
        missing.push(Endpoint {
            path: path.into(),
            directory,
            candidates,
        });
    }
    let mut temporaries = Vec::new();
    for attempt in &attempts {
        let Some(temporary) = attempt.temporary else {
            continue;
        };
        let prefix = temporary.relative();
        let original = attempt
            .target
            .relative()
            .ok_or("retirement target namespace")?;
        for row in &after.members {
            let path = Path::new(std::ffi::OsStr::from_bytes(&row.path));
            let Ok(suffix) = path.strip_prefix(&prefix) else {
                continue;
            };
            if attempt.removal == Some(true) {
                return Err("successfully removed temporary still has a file endpoint");
            }
            let source = if suffix.as_os_str().is_empty() {
                original.clone()
            } else {
                original.join(suffix)
            };
            let matches_initial = before.members.iter().any(|initial| {
                initial.path.as_ref() == source.as_os_str().as_bytes()
                    && initial.bytes == row.bytes
                    && initial.digest == row.digest
                    && initial.mode == row.mode
            });
            temporaries.push(Retained {
                pid: attempt.pid,
                attempt: attempt.token,
                path: row.path.clone(),
                original: source.as_os_str().as_bytes().into(),
                directory: false,
                matches_initial,
            });
        }
        for row in &after.directories {
            let path = Path::new(std::ffi::OsStr::from_bytes(&row.path));
            let Ok(suffix) = path.strip_prefix(&prefix) else {
                continue;
            };
            if attempt.removal == Some(true) {
                return Err("successfully removed temporary still has a directory endpoint");
            }
            let source = if suffix.as_os_str().is_empty() {
                original.clone()
            } else {
                original.join(suffix)
            };
            let matches_initial = before.directories.iter().any(|initial| {
                initial.path.as_ref() == source.as_os_str().as_bytes() && initial.mode == row.mode
            });
            temporaries.push(Retained {
                pid: attempt.pid,
                attempt: attempt.token,
                path: row.path.clone(),
                original: source.as_os_str().as_bytes().into(),
                directory: true,
                matches_initial,
            });
        }
    }
    Ok(Joined {
        attempts,
        missing,
        temporaries,
    })
}

fn direct(target: Target) -> bool {
    matches!(target, Target::IndexBucket { .. } | Target::IndexRow { .. })
}

fn covered(path: &[u8]) -> bool {
    let path = Path::new(std::ffi::OsStr::from_bytes(path));
    path.ancestors()
        .any(|ancestor| Target::parse(ancestor).is_some())
}
