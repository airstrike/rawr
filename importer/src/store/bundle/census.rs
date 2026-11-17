//! What a census drift observed in a sealed bundle.
//!
//! The record keeps only the bundle's totals, so the drift itself names the
//! first total that differs. The member is found by a second walk that runs
//! only after the drift, in canonical order, from the members' modification
//! times and allocations (see `suspect`). It is evidence for reading, never
//! part of the verdict.

use std::fmt;
use std::fs;
use std::path::Path;

use super::super::accounting::Charge;
use super::Drift;
use crate::node::Kind;

/// Top-level controls Rawr itself writes or locks after the seal.
const CONTROLS: [&str; 3] = ["seal", "lease", "allocation"];

#[derive(Clone, Debug)]
pub(super) struct Census {
    kind: Kind,
    total: Total,
    recorded: u64,
    measured: u64,
    suspect: Option<Suspect>,
}

#[derive(Clone, Copy, Debug)]
enum Total {
    Allocations,
    Members,
    Bytes,
}

#[derive(Clone, Debug)]
struct Suspect {
    path: Box<Path>,
    length: u64,
    allocated: u64,
    cause: Cause,
}

#[derive(Clone, Copy, Debug)]
enum Cause {
    /// Its modification time is later than the seal's.
    Written,
    /// Its allocated blocks exceed its rounded logical length.
    Allocated,
}

impl Census {
    pub(super) fn observe(
        root: &Path,
        kind: Kind,
        drift: Drift,
        recorded: Charge,
        measured: Charge,
    ) -> Self {
        let (total, recorded, measured) = match drift {
            Drift::Count if recorded.allocations != measured.allocations => (
                Total::Allocations,
                recorded.allocations,
                measured.allocations,
            ),
            Drift::Count => (Total::Members, recorded.members, measured.members),
            // Geometry drift is never a census; `Fact::drift` routes it.
            Drift::Growth | Drift::Geometry => (Total::Bytes, recorded.bytes, measured.bytes),
        };
        Self {
            kind,
            total,
            recorded,
            measured,
            suspect: suspect(root),
        }
    }
}

impl fmt::Display for Census {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total = match self.total {
            Total::Allocations => "allocations",
            Total::Members => "members",
            Total::Bytes => "charged bytes",
        };
        write!(
            f,
            "{} bundle {total}: sealed {}, measured {}",
            self.kind.directory(),
            self.recorded,
            self.measured
        )?;
        if let Some(suspect) = &self.suspect {
            let cause = match suspect.cause {
                Cause::Written => "written after the seal",
                Cause::Allocated => "allocated beyond its length",
            };
            write!(
                f,
                "; `{}` {cause} (length {}, allocated {})",
                suspect.path.display(),
                suspect.length,
                suspect.allocated
            )?;
        }
        Ok(())
    }
}

/// A file written after the seal names itself. A directory whose entries
/// changed after the seal is named when no file was written, since a removed
/// member leaves only its directory's time. The widest allocation is the
/// last resort, for growth with every time unchanged.
fn suspect(root: &Path) -> Option<Suspect> {
    let sealed = fs::symlink_metadata(root.join("seal"))
        .and_then(|metadata| metadata.modified())
        .ok()?;
    let mut directory: Option<Suspect> = None;
    let mut widest: Option<(u64, Suspect)> = None;
    let mut pending = vec![root.to_path_buf()];
    let mut visited = 0_u64;
    while let Some(path) = pending.pop() {
        visited += 1;
        if visited > super::MAX_MEMBERS {
            break;
        }
        if crate::input::split_debuginfo(&path) {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        let relative = path.strip_prefix(root).ok()?;
        // The root's own time moves when publication writes its controls.
        if CONTROLS
            .iter()
            .any(|control| relative == Path::new(control))
        {
            continue;
        }
        let length = metadata.len();
        let allocated = super::allocated_bytes(&metadata).unwrap_or(0);
        let member = |cause| Suspect {
            path: relative.into(),
            length,
            allocated,
            cause,
        };
        let written = metadata.modified().is_ok_and(|modified| modified > sealed);
        if metadata.is_dir() {
            if written && directory.is_none() && relative != Path::new("") {
                directory = Some(member(Cause::Written));
            }
            if let Ok(entries) = super::read_sorted(&path) {
                pending.extend(entries.into_iter().rev().map(|entry| entry.path()));
            }
            continue;
        }
        if written {
            return Some(member(Cause::Written));
        }
        let excess = allocated.saturating_sub(super::round_block(length).unwrap_or(u64::MAX));
        if excess > widest.as_ref().map_or(0, |(widest, _)| *widest) {
            widest = Some((excess, member(Cause::Allocated)));
        }
    }
    directory.or(widest.map(|(_, suspect)| suspect))
}
