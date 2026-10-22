//! Watcher-local invalidation history; never native freshness or live authority.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// One watcher's native invalidation watermark captured before full validation.
/// A successfully admitted owner may retain it; capture alone grants no reuse.
#[derive(Debug)]
pub struct Baseline {
    history: Arc<Counters>,
    revision: u64,
    rescans: u64,
    stamp: Stamp,
}

#[derive(Debug, Default)]
struct Counters {
    revision: AtomicU64,
    rescans: AtomicU64,
}

#[derive(Clone, Default)]
pub(super) struct History(Arc<Counters>);

impl History {
    pub(super) fn capture(&self, stamp: Stamp) -> Baseline {
        Baseline {
            history: Arc::clone(&self.0),
            revision: self.0.revision.load(Ordering::Acquire),
            rescans: self.0.rescans.load(Ordering::Acquire),
            stamp,
        }
    }

    /// No native event since the baseline. Dropped events (a rescan) only
    /// count when a native input's filesystem stamp actually differs.
    pub(super) fn unchanged(&self, baseline: &Baseline) -> bool {
        Arc::ptr_eq(&self.0, &baseline.history)
            && baseline.revision != u64::MAX
            && self.0.revision.load(Ordering::Acquire) == baseline.revision
            && (self.0.rescans.load(Ordering::Acquire) == baseline.rescans
                || baseline.stamp.is_current())
    }

    pub(super) fn invalidate(&self) {
        // Saturation permanently disables eligibility rather than wrapping to
        // an earlier owner. Updates occur inside the admission gate's lock.
        let revision = &self.0.revision;
        revision.store(
            revision.load(Ordering::Acquire).saturating_add(1),
            Ordering::Release,
        );
    }

    /// The backend dropped events: anything may have changed, so eligibility
    /// now rests on re-stamping the native inputs instead of on events.
    pub(super) fn rescan(&self) {
        self.0.rescans.fetch_add(1, Ordering::AcqRel);
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

    fn is_current(&self) -> bool {
        cfg!(unix) && entries(&self.paths, &self.trees) == self.entries
    }
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
        let history = History(Arc::new(Counters {
            revision: AtomicU64::new(u64::MAX - 1),
            rescans: AtomicU64::new(0),
        }));
        let before = history.capture(Stamp::default());
        history.invalidate();
        let exhausted = history.capture(Stamp::default());
        history.invalidate();
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
        let history = History::default();
        let capture = || {
            history.capture(Stamp::capture(
                Box::from([file.clone(), root.join("absent")]),
                Box::from([tree.clone()]),
            ))
        };

        let baseline = capture();
        history.rescan();
        assert!(history.unchanged(&baseline), "nothing was written");

        fs::write(tree.join("new.rs"), "fn b() {}\n").unwrap();
        assert!(!history.unchanged(&baseline), "a file appeared in a tree");

        let baseline = capture();
        history.rescan();
        fs::write(&file, "[package]\nname = \"x\"\n").unwrap();
        assert!(
            !history.unchanged(&baseline),
            "an exact input was rewritten"
        );

        let baseline = capture();
        history.invalidate();
        assert!(
            !history.unchanged(&baseline),
            "a concrete event never re-stamps"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
