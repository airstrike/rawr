//! Joins replacement histories without assigning an order between processes.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::{Event, Prior, Target};
use crate::metrics::cell::project::Manifest;
use crate::metrics::principal::{self, Commitment, Evidence};

#[derive(Debug, Serialize)]
pub struct Joined {
    pub processes: Box<[Process]>,
    pub endpoints: Vec<Endpoint>,
}

#[derive(Debug, Serialize)]
pub struct Process {
    pub pid: u32,
    pub attempts: usize,
    pub renamed: usize,
    pub failed: usize,
}

/// Matching writers are candidates, not a fabricated cross-process last writer.
#[derive(Debug, Serialize)]
pub struct Endpoint {
    pub target: Target,
    pub matches_initial: bool,
    pub matching_writers: Vec<u32>,
}

struct Attempt {
    target: Target,
    after: Commitment,
    renamed: bool,
    cleanup: Option<bool>,
    finished: Option<bool>,
}

/// This covers replacements only. Retirement, controls and pin records remain
/// separate joins even when all current endpoint bytes have a matching writer.
pub fn pair(
    processes: [(u32, &Evidence); 2],
    root: &Path,
    before: &Manifest,
    after: &Manifest,
) -> Result<Joined, &'static str> {
    if processes[0].0 == 0 || processes[1].0 == 0 || processes[0].0 == processes[1].0 {
        return Err("ledger join requires two distinct process owners");
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
    for (pid, evidence) in processes.iter().copied() {
        if evidence.fault.is_some() || !evidence.cell_transport {
            return Err("ledger observation is incomplete");
        }
        let mut attempts = BTreeMap::<u64, Attempt>::new();
        let mut result = Process {
            pid,
            attempts: 0,
            renamed: 0,
            failed: 0,
        };
        for event in evidence.events.as_slice() {
            let principal::Event::Ledger { change } = event else {
                continue;
            };
            match change {
                Event::Started {
                    attempt,
                    publication,
                    root: actual_root,
                    relative,
                    target,
                    schema,
                    purpose,
                    cause,
                    before,
                    after,
                } => {
                    let target_path = target.relative();
                    if *attempt == 0
                        || *publication == 0
                        || *actual_root != root
                        || principal::path_digest(&target_path) != *relative
                        || !super::compatible(*target, *schema, *purpose, *cause)
                        || after.bytes < 10
                        || matches!(before, Prior::Bytes(value) if value.bytes < 10)
                        || attempts
                            .insert(
                                *attempt,
                                Attempt {
                                    target: *target,
                                    after: *after,
                                    renamed: false,
                                    cleanup: None,
                                    finished: None,
                                },
                            )
                            .is_some()
                    {
                        return Err("ledger replacement identity or namespace changed");
                    }
                    result.attempts += 1;
                }
                Event::Renamed { attempt } => {
                    let state = attempts
                        .get_mut(attempt)
                        .ok_or("ledger rename precedes attempt")?;
                    if state.renamed || state.finished.is_some() || state.cleanup.is_some() {
                        return Err("ledger rename order changed");
                    }
                    state.renamed = true;
                    result.renamed += 1;
                    writes.push((pid, state.target, state.after));
                }
                Event::Cleanup { attempt, succeeded } => {
                    let state = attempts
                        .get_mut(attempt)
                        .ok_or("ledger cleanup precedes attempt")?;
                    if state.cleanup.replace(*succeeded).is_some() || state.finished.is_some() {
                        return Err("ledger cleanup order changed");
                    }
                }
                Event::Finished { attempt, succeeded } => {
                    let state = attempts
                        .get_mut(attempt)
                        .ok_or("ledger result precedes attempt")?;
                    if state.finished.replace(*succeeded).is_some()
                        || (*succeeded && (!state.renamed || state.cleanup == Some(false)))
                    {
                        return Err("ledger result differs from reached operations");
                    }
                    result.failed += usize::from(!*succeeded);
                }
            }
        }
        if attempts.values().any(|attempt| attempt.finished.is_none()) {
            return Err("ledger attempt has no terminal outcome");
        }
        results.push(result);
    }
    let mut endpoints = Vec::new();
    for member in after.members.iter() {
        let Some(target) = target(&member.path) else {
            if member.path.starts_with(b"ledgers/projects/")
                || member.path.as_ref() == b"ledgers/installation"
            {
                return Err("ledger endpoint namespace changed");
            }
            continue;
        };
        let commitment = Commitment {
            bytes: member.bytes,
            sha256: member.digest,
        };
        let matches_initial = before.members.iter().any(|initial| initial == member);
        let mut matching_writers = Vec::new();
        for (pid, written_target, written) in &writes {
            if *written_target == target
                && *written == commitment
                && !matching_writers.contains(pid)
            {
                matching_writers.push(*pid);
            }
        }
        if processes.len() == 1
            && writes
                .iter()
                .rev()
                .find(|(_, written_target, _)| *written_target == target)
                .is_some_and(|(_, _, written)| *written != commitment)
        {
            return Err("ledger endpoint contradicts its last actual replacement");
        }
        if !matches_initial && matching_writers.is_empty() {
            return Err("ledger endpoint has no actual matching replacement");
        }
        endpoints.push(Endpoint {
            target,
            matches_initial,
            matching_writers,
        });
    }
    Ok(Joined {
        processes: results.into_boxed_slice(),
        endpoints,
    })
}

fn target(path: &[u8]) -> Option<Target> {
    if path == b"ledgers/installation" {
        return Some(Target::Installation);
    }
    let name = path.strip_prefix(b"ledgers/projects/")?;
    if name.len() != 64 {
        return None;
    }
    let digit = |value| match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    };
    let mut project = [0; 32];
    for (value, pair) in project.iter_mut().zip(name.chunks_exact(2)) {
        *value = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Some(Target::Project(project))
}
