//! Watcher-local invalidation history; never native freshness or live authority.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

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
    /// Each source input's identity as [`History::poll`] last saw it, and
    /// whether that sighting was a change it delivered.
    sources: Mutex<BTreeMap<PathBuf, (Option<Identity>, bool)>>,
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

    /// Stat `paths` and return those whose identity changed since the last
    /// poll. A path seen the first time is only recorded.
    pub(super) fn poll(&self, paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
        let mut sources = self
            .0
            .sources
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut changed = Vec::new();
        for path in paths {
            let now = identity(&path);
            let delivered = match sources.get(&path) {
                Some((seen, delivered)) if *seen == now => *delivered,
                Some(_) => {
                    changed.push(path.clone());
                    false
                }
                None => false,
            };
            sources.insert(path, (now, delivered));
        }
        changed
    }

    /// Mark the changes [`Self::poll`] returned as delivered.
    pub(super) fn deliver(&self, paths: &[PathBuf]) {
        let mut sources = self
            .0
            .sources
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for path in paths {
            if let Some((_, delivered)) = sources.get_mut(path) {
                *delivered = true;
            }
        }
    }

    /// Whether an event on `paths` reports only writes a poll already
    /// delivered: each path a poll knows still has the identity it delivered.
    /// A platform stream that lags behind the poll reports them again.
    pub(super) fn delivered(&self, paths: &[PathBuf]) -> bool {
        let sources = self
            .0
            .sources
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut known = paths
            .iter()
            .filter_map(|path| Some((path, sources.get(path)?)))
            .peekable();
        known.peek().is_some()
            && known.all(|(path, (seen, delivered))| *delivered && *seen == identity(path))
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

pub(super) type Identity = [i64; 8];

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
pub(super) fn identity(path: &Path) -> Option<Identity> {
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
pub(super) fn identity(_: &Path) -> Option<Identity> {
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
            ..Counters::default()
        }));
        let before = history.capture(Stamp::default());
        history.invalidate();
        let exhausted = history.capture(Stamp::default());
        history.invalidate();
        assert!(!history.unchanged(&before));
        assert!(!history.unchanged(&exhausted));
    }

    #[test]
    fn a_poll_delivers_each_write_once_and_a_late_event_for_it_is_stale() {
        let root = std::env::temp_dir().join(format!("rawr-watch-poll-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("main.rs");
        fs::write(&source, "fn main() {}\n").unwrap();
        let history = History::default();
        let paths = [source.clone()];

        assert!(
            history.poll(paths.clone()).is_empty(),
            "a first sighting is recorded"
        );
        assert!(!history.delivered(&paths), "nothing was delivered yet");

        fs::write(&source, "fn main() { 1; }\n").unwrap();
        assert_eq!(history.poll(paths.clone()), paths);
        assert!(
            !history.delivered(&paths),
            "delivery is marked after the event"
        );
        history.deliver(&paths);
        assert!(
            history.delivered(&paths),
            "the stream's late event is stale"
        );
        assert!(history.poll(paths.clone()).is_empty());
        assert!(
            !history.delivered(&[root.join("other.rs")]),
            "an unknown path is news"
        );

        fs::write(&source, "fn main() { 2; }\n").unwrap();
        assert!(
            !history.delivered(&paths),
            "a newer write is news to the stream"
        );
        fs::remove_dir_all(root).unwrap();
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
