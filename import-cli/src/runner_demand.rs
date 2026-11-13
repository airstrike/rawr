//! The demand of the runner a project last built or took from the store
//! (#1743), kept beside the project's other rawr files: the Rust paths its
//! API was resolved for and its native items. The next preparation whose
//! program names no path the runner's didn't resolves the same API, and a
//! request whose native items that runner covers takes its demand
//! (`rawr_import::surface::Kept`), so it names the same runner instead of
//! one built for fewer items.

use std::path::{Path, PathBuf};

/// The last runner's demand.
pub(crate) struct Last {
    /// The Rust paths its API was resolved for, sorted.
    paths: Vec<Box<str>>,
    kept: rawr_import::surface::Kept,
}

impl Last {
    /// Whether the runner's API was resolved for each of `paths`, so a
    /// resolution for its paths names nothing more than this program does.
    pub(crate) fn resolves(&self, paths: &[Box<str>]) -> bool {
        paths
            .iter()
            .all(|path| self.paths.binary_search(path).is_ok())
    }

    /// The Rust paths the runner's API was resolved for.
    pub(crate) fn paths(&self) -> &[Box<str>] {
        &self.paths
    }

    /// The runner's native items.
    pub(crate) fn kept(&self) -> &rawr_import::surface::Kept {
        &self.kept
    }
}

/// The Rust paths a program resolves on its own: `remembered` and `seed`,
/// sorted.
pub(crate) fn own(remembered: &[Box<str>], seed: &[Box<str>]) -> Vec<Box<str>> {
    let mut own = remembered.iter().chain(seed).cloned().collect::<Vec<_>>();
    own.sort_unstable();
    own.dedup();
    own
}

/// Where the project at `project` keeps its last runner's native items,
/// and the paths beside them.
fn location(project: &Path) -> (PathBuf, PathBuf) {
    location_in(&rawr_import::location::rawr_directory_here(project))
}

/// [`location`] for the project whose Rawr directory is `directory`.
fn location_in(directory: &Path) -> (PathBuf, PathBuf) {
    (
        directory.join("runner-demand.json"),
        directory.join("runner-paths"),
    )
}

/// Keep `kept`, a request's native items whose runner was built, and
/// `paths`, the Rust paths its API was resolved for, as the project at
/// `project`'s. A file that holds them already is not written again. What
/// can't be kept leaves nothing, and the next request needs its own runner.
pub(crate) fn store(project: &Path, kept: &[u8], paths: &[Box<str>]) {
    let (items, resolved) = location(project);
    let text = paths
        .iter()
        .fold(String::new(), |text, path| text + &**path + "\n");
    let keep = |path: &Path, bytes: &[u8]| {
        std::fs::read(path).is_ok_and(|held| held == bytes) || std::fs::write(path, bytes).is_ok()
    };
    let written = items
        .parent()
        .is_some_and(|parent| std::fs::create_dir_all(parent).is_ok())
        && keep(&resolved, text.as_bytes())
        && keep(&items, kept);
    if !written {
        let _ = std::fs::remove_file(&items);
        let _ = std::fs::remove_file(&resolved);
    }
}

/// The last runner's demand the project whose Rawr directory is `rawr`
/// keeps, or none.
pub(crate) fn load_in(rawr: &Path) -> Option<Last> {
    let (items, resolved) = location_in(rawr);
    let kept = rawr_import::surface::Kept::decode(&std::fs::read(items).ok()?)?;
    let mut paths = std::fs::read_to_string(resolved)
        .ok()?
        .lines()
        .map(Box::from)
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();
    Some(Last { paths, kept })
}
