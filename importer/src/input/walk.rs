//! The directories a tree walk is inside when it follows symlinks.
//!
//! Cargo walks a `rerun-if-changed` directory following links, and stops at
//! a link back to a directory the walk is already inside (walkdir's loop
//! check). A walk without links is one segment from its root; each followed
//! directory link starts a segment at the link's canonical target.
//!
//! A walk of a package's source tree also leaves out what Cargo's file
//! listing never selects.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The segments of one walk path, innermost first.
pub(in crate::input) struct Ancestors {
    /// The canonical directory this segment starts at.
    start: PathBuf,
    /// The directory holding the link that started this segment, and the
    /// segment it ended.
    outer: Option<(PathBuf, Rc<Ancestors>)>,
}

impl Ancestors {
    /// A walk rooted at the canonical directory `root`.
    pub(in crate::input) fn root(root: PathBuf) -> Rc<Self> {
        Rc::new(Self {
            start: root,
            outer: None,
        })
    }

    /// The walk path past a link in `directory` to the canonical `target`.
    pub(in crate::input) fn follow(self: &Rc<Self>, directory: &Path, target: PathBuf) -> Rc<Self> {
        Rc::new(Self {
            start: target,
            outer: Some((directory.to_path_buf(), Rc::clone(self))),
        })
    }

    /// Whether the canonical `target` is a directory the walk is inside
    /// while it lists `directory`: within a segment, every directory from
    /// its start down to where the walk left it.
    pub(in crate::input) fn encloses(&self, directory: &Path, target: &Path) -> bool {
        let mut segment = self;
        let mut end = directory;
        loop {
            if end.starts_with(target) && target.starts_with(&segment.start) {
                return true;
            }
            match &segment.outer {
                Some((left, outer)) => {
                    end = left;
                    segment = outer;
                }
                None => return false,
            }
        }
    }
}

/// Whether a walk of a package's source tree leaves out the entry `name`
/// of a directory at `depth`. In either its Git or its walking mode,
/// Cargo's file listing selects nothing under a `.git` or a nested package,
/// and under the package's own `target` directory only what Git tracks
/// there; everything else it might select is kept, whatever its ignore and
/// include rules say, so the walk sees every file whose change makes Cargo
/// rerun a build script that declares no `rerun-if` input. `target` is left
/// out whole: every build writes there. `nested` probes for a directory's
/// `Cargo.toml` and runs only for a directory.
pub(in crate::input) fn package_omits<E>(
    name: &OsStr,
    depth: usize,
    directory: bool,
    nested: impl FnOnce() -> Result<bool, E>,
) -> Result<bool, E> {
    if name == ".git" {
        return Ok(true);
    }
    if !directory {
        return Ok(false);
    }
    if depth == 0 && name == "target" {
        return Ok(true);
    }
    nested()
}

/// Whether resolving a link failed because it names nothing: its target is
/// absent, or the links loop.
pub(in crate::input) fn dangling(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Ancestors;

    #[test]
    fn a_walk_is_inside_each_directory_from_a_segment_start_to_where_it_left() {
        let root = Ancestors::root("/r".into());
        assert!(root.encloses(Path::new("/r/a/b"), Path::new("/r/a")));
        assert!(root.encloses(Path::new("/r/a/b"), Path::new("/r")));
        assert!(!root.encloses(Path::new("/r/a/b"), Path::new("/")));
        assert!(!root.encloses(Path::new("/r/a/b"), Path::new("/r/c")));
        let linked = root.follow(Path::new("/r/a"), "/x".into());
        assert!(linked.encloses(Path::new("/x/y"), Path::new("/x")));
        assert!(linked.encloses(Path::new("/x/y"), Path::new("/r/a")));
        assert!(linked.encloses(Path::new("/x/y"), Path::new("/r")));
        assert!(
            !linked.encloses(Path::new("/x/y"), Path::new("/r/a/b")),
            "the walk left /r at /r/a"
        );
        assert!(!linked.encloses(Path::new("/x/y"), Path::new("/")));
    }
}
