//! Watcher-local invalidation history; never native freshness or live authority.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One watcher's native invalidation watermark captured before full validation.
/// A successfully admitted owner may retain it; capture alone grants no reuse.
#[derive(Debug)]
pub struct Baseline {
    history: Arc<Origin>,
    revision: u64,
    rescans: u64,
    registrations: u64,
    stamp: Stamp,
}

/// The watcher's word, at one moment, that no native input changed since
/// a baseline: a new process of the runner checked against it needs no
/// walk of its read set (#1755). Only [`crate::Adapter::unchanged`] makes
/// one.
#[derive(Debug)]
pub struct Unchanged(pub(super) ());

/// Why a native baseline no longer stands, so a save can't take the
/// resident route against the running native build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    /// The baseline belongs to another watcher, or its history ran out.
    Watcher,
    /// The watcher stopped admitting saves: its backend or a registration
    /// failed.
    Suspended,
    /// A native input changed: the path of the latest such event, when it
    /// named one, and whether the watcher registered new inputs since.
    Event {
        path: Option<PathBuf>,
        registered: bool,
    },
    /// The backend dropped events and a native input's filesystem stamp
    /// differs: the first that does, and whether the watcher registered new
    /// inputs since.
    Dropped {
        path: Option<PathBuf>,
        registered: bool,
    },
}

impl fmt::Display for Change {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let registered = match self {
            Self::Watcher => {
                return formatter.write_str(
                    "the file watcher's history no longer reaches the running native build",
                );
            }
            Self::Suspended => return formatter.write_str("the file watcher stopped"),
            Self::Event { path, registered } => {
                match path {
                    Some(path) => write!(formatter, "`{}` changed", path.display())?,
                    None => formatter.write_str("a native input changed")?,
                }
                *registered
            }
            Self::Dropped { path, registered } => {
                formatter.write_str("the file watcher dropped events and ")?;
                match path {
                    Some(path) => write!(formatter, "`{}` differs", path.display())?,
                    None => formatter.write_str("a native input differs")?,
                }
                *registered
            }
        };
        formatter.write_str(" since the running native build was checked")?;
        if registered {
            formatter.write_str(", after the file watcher registered new inputs")?;
        }
        Ok(())
    }
}

/// One watcher's identity, which a baseline names.
#[derive(Debug, Default)]
struct Origin;

/// What the watcher's owner learned from its events, read on the owner's
/// own thread.
#[derive(Default)]
pub(super) struct History {
    origin: Arc<Origin>,
    revision: u64,
    rescans: u64,
    /// How many times the watcher replaced its registration.
    registrations: u64,
    /// Whether an event since the watcher last derived its roots may have
    /// created or removed a directory they derive from: any event but a
    /// write to a Rawr source with nothing watched beneath it.
    roots_moved: bool,
    /// The path of the latest native event, when it named one.
    changed: Option<PathBuf>,
}

impl History {
    pub(super) fn capture(&self, stamp: Stamp) -> Baseline {
        Baseline {
            history: Arc::clone(&self.origin),
            revision: self.revision,
            rescans: self.rescans,
            registrations: self.registrations,
            stamp,
        }
    }

    /// No native event since the baseline (see [`Self::change`]).
    #[cfg(test)]
    pub(super) fn unchanged(&self, baseline: &Baseline) -> bool {
        self.change(baseline).is_none()
    }

    /// What changed since the baseline, `None` when nothing did. Dropped
    /// events (a rescan) only count when a native input's filesystem stamp
    /// actually differs.
    pub(super) fn change(&self, baseline: &Baseline) -> Option<Change> {
        if !Arc::ptr_eq(&self.origin, &baseline.history) || baseline.revision == u64::MAX {
            return Some(Change::Watcher);
        }
        let registered = self.registrations != baseline.registrations;
        if self.revision != baseline.revision {
            return Some(Change::Event {
                path: self.changed.clone(),
                registered,
            });
        }
        if self.rescans == baseline.rescans {
            return None;
        }
        match baseline.stamp.compare() {
            Stamped::Current => None,
            Stamped::Differs(path) => Some(Change::Dropped { path, registered }),
        }
    }

    /// A native input at `path`, when the event named one, changed.
    pub(super) fn invalidate(&mut self, path: Option<&Path>) {
        // Saturation permanently disables eligibility rather than wrapping to
        // an earlier owner.
        self.revision = self.revision.saturating_add(1);
        self.changed = path.map(Path::to_path_buf);
    }

    /// The watcher replaced its registration with a successor's.
    pub(super) const fn registered(&mut self) {
        self.registrations = self.registrations.wrapping_add(1);
    }

    /// An event may have created or removed a directory the watcher's roots
    /// derive from.
    pub(super) const fn roots_moved(&mut self) {
        self.roots_moved = true;
    }

    /// Whether an event since the last call may have moved the watcher's
    /// roots, which the caller derives again.
    pub(super) const fn take_roots_moved(&mut self) -> bool {
        std::mem::replace(&mut self.roots_moved, false)
    }

    /// The backend dropped events: anything may have changed, so eligibility
    /// now rests on re-stamping the native inputs instead of on events.
    pub(super) const fn rescan(&mut self) {
        self.rescans = self.rescans.wrapping_add(1);
    }
}

/// Filesystem identity of every native input at baseline capture. Final
/// validation after the capture proves contents; an equal stamp later proves
/// nothing was written since, even when the event stream lost the writes.
#[derive(Debug, Default)]
pub(super) struct Stamp {
    paths: Box<[PathBuf]>,
    trees: Box<[PathBuf]>,
    entries: Vec<(PathBuf, Option<Identity>)>,
}

type Identity = [i64; 8];

impl Stamp {
    pub(super) fn capture(paths: Box<[PathBuf]>, trees: Box<[PathBuf]>) -> Self {
        let entries = entries(&paths, &trees);
        Self {
            paths,
            trees,
            entries,
        }
    }

    /// Whether every native input still has its captured identity, or the
    /// first that doesn't.
    fn compare(&self) -> Stamped {
        if !cfg!(unix) {
            return Stamped::Differs(None);
        }
        let now = entries(&self.paths, &self.trees);
        if now == self.entries {
            return Stamped::Current;
        }
        let differs = now
            .iter()
            .zip(&self.entries)
            .find(|(now, then)| now != then)
            .map(|(now, _)| now.0.clone())
            .or_else(|| {
                // One list is the other's prefix: the first entry past it.
                now.get(self.entries.len())
                    .or_else(|| self.entries.get(now.len()))
                    .map(|(path, _)| path.clone())
            });
        Stamped::Differs(differs)
    }
}

/// A [`Stamp`] compared with the filesystem now.
enum Stamped {
    Current,
    /// The first path whose identity differs, when the platform stamps any.
    Differs(Option<PathBuf>),
}

fn entries(paths: &[PathBuf], trees: &[PathBuf]) -> Vec<(PathBuf, Option<Identity>)> {
    let mut entries = Vec::with_capacity(paths.len());
    for path in paths {
        entries.push((path.clone(), identity(path)));
    }
    for tree in trees {
        walk(tree, &mut entries);
    }
    entries
}

#[cfg(unix)]
fn identity(path: &Path) -> Option<Identity> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = fs::symlink_metadata(path).ok()?;
    #[allow(clippy::cast_possible_wrap)]
    Some([
        metadata.dev() as i64,
        metadata.ino() as i64,
        i64::from(metadata.mode()),
        metadata.len() as i64,
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    ])
}

#[cfg(not(unix))]
fn identity(_: &Path) -> Option<Identity> {
    None
}

fn walk(root: &Path, entries: &mut Vec<(PathBuf, Option<Identity>)>) {
    entries.push((root.to_path_buf(), identity(root)));
    let Ok(children) = fs::read_dir(root) else {
        return;
    };
    let mut children = children
        .filter_map(|child| child.ok().map(|child| child.path()))
        .collect::<Vec<_>>();
    children.sort();
    for child in children {
        if fs::symlink_metadata(&child).is_ok_and(|metadata| metadata.is_dir()) {
            walk(&child, entries);
        } else {
            entries.push((child.clone(), identity(&child)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_history_never_reenables_resident_eligibility() {
        let mut history = History {
            revision: u64::MAX - 1,
            ..History::default()
        };
        let before = history.capture(Stamp::default());
        history.invalidate(None);
        let exhausted = history.capture(Stamp::default());
        history.invalidate(None);
        assert!(!history.unchanged(&before));
        assert!(!history.unchanged(&exhausted));
    }

    #[test]
    fn dropped_events_keep_eligibility_only_while_native_inputs_are_untouched() {
        let root = std::env::temp_dir().join(format!("rawr-watch-stamp-{}", std::process::id()));
        let tree = root.join("tree");
        fs::create_dir_all(&tree).unwrap();
        let file = root.join("Cargo.toml");
        fs::write(&file, "[package]\n").unwrap();
        fs::write(tree.join("lib.rs"), "fn a() {}\n").unwrap();
        let mut history = History::default();
        let capture = |history: &History| {
            history.capture(Stamp::capture(
                Box::from([file.clone(), root.join("absent")]),
                Box::from([tree.clone()]),
            ))
        };

        let baseline = capture(&history);
        history.rescan();
        assert!(history.unchanged(&baseline), "nothing was written");

        fs::write(tree.join("new.rs"), "fn b() {}\n").unwrap();
        assert!(!history.unchanged(&baseline), "a file appeared in a tree");

        let baseline = capture(&history);
        history.rescan();
        fs::write(&file, "[package]\nname = \"x\"\n").unwrap();
        assert!(
            !history.unchanged(&baseline),
            "an exact input was rewritten"
        );

        let baseline = capture(&history);
        history.invalidate(None);
        assert!(
            !history.unchanged(&baseline),
            "a concrete event never re-stamps"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// What changed names its path, and says when the watcher registered
    /// new inputs since the baseline.
    #[test]
    fn a_change_names_its_path_and_a_registration_since_the_baseline() {
        let root = std::env::temp_dir().join(format!("rawr-watch-change-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("Cargo.toml");
        fs::write(&file, "[package]\n").unwrap();
        let mut history = History::default();
        let capture = |history: &History| {
            history.capture(Stamp::capture(Box::from([file.clone()]), Box::default()))
        };

        let baseline = capture(&history);
        history.rescan();
        fs::write(&file, "[package]\nname = \"x\"\n").unwrap();
        assert_eq!(
            history.change(&baseline),
            Some(Change::Dropped {
                path: Some(file.clone()),
                registered: false,
            })
        );

        let baseline = capture(&history);
        history.registered();
        history.invalidate(Some(&file));
        let change = history.change(&baseline).expect("an event changed it");
        assert_eq!(
            change.to_string(),
            format!(
                "`{}` changed since the running native build was checked, \
                 after the file watcher registered new inputs",
                file.display()
            )
        );
        assert_eq!(
            History::default().change(&baseline),
            Some(Change::Watcher),
            "another watcher's baseline never stands"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
