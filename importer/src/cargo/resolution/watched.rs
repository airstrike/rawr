//! Physical identities of everything one owner replay depends on.
//!
//! A replay is a pure function of the bytes it reads. Its sealed bundle, its
//! recorded input membership and its compiler namespace name every such file
//! and directory, so the stamps taken before the replay read them prove, while
//! they still match, that a later replay would read the same bytes and reach the
//! same result. Any write, rename, addition or removal changes a stamp (the
//! status-change time cannot be set by user space), and the next consumer then
//! replays in full.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::extract::plan::namespace;
use sha2::Digest as _;

use crate::input::proof::{Entry, TreeFile};

/// Device, inode, length, mode, modification and status-change time (with
/// nanoseconds) of one path, or `None` while it is absent or unreadable.
type Stamp = Option<[i64; 8]>;

#[derive(Default)]
pub(in crate::cargo) struct Watched {
    bundle: PathBuf,
    stamps: BTreeMap<PathBuf, Stamp>,
}

impl Watched {
    /// Stamp the bundle tree, every recorded input path and the namespace.
    pub(in crate::cargo) fn observe(
        bundle: &Path,
        entries: &[Entry],
        namespace: Option<&namespace::Retained>,
    ) -> Self {
        let mut exact = Vec::new();
        for entry in entries {
            match entry {
                Entry::Workspace { paths, .. } => {
                    exact.extend(paths.iter().map(|path| PathBuf::from(path.as_ref())));
                }
                Entry::File { path, .. } | Entry::Missing { path, .. } => {
                    exact.push(PathBuf::from(path.as_ref()));
                }
                Entry::Directory {
                    path: root,
                    directories,
                    files,
                    ..
                } => {
                    let root = Path::new(root.as_ref());
                    exact.extend(directories.iter().map(|item| root.join(item.path.as_ref())));
                    tree(&mut exact, root, files);
                }
                Entry::Git { root, files, .. } => tree(&mut exact, Path::new(root.as_ref()), files),
                Entry::Registry { root, .. } => {
                    let root = Path::new(root.as_ref());
                    exact.push(root.join(".cargo-ok"));
                    exact.push(root.to_path_buf());
                }
                Entry::Environment { .. }
                | Entry::EnvironmentProjection { .. }
                | Entry::Fact { .. }
                | Entry::BuildMessage { .. }
                | Entry::Directive { .. } => {}
            }
        }
        if let Some(namespace) = namespace {
            exact.extend(namespace.roots.iter().cloned());
            exact.extend(namespace.explicit.iter().cloned());
            exact.extend(namespace.members.iter().map(|member| member.path.clone()));
        }
        let mut stamps = BTreeMap::new();
        walk(bundle, &mut stamps);
        for path in exact {
            let stamp = stamp(&path);
            stamps.insert(path, stamp);
        }
        Self {
            bundle: bundle.to_path_buf(),
            stamps,
        }
    }

    /// The bundle root whose tree was stamped.
    pub(in crate::cargo) fn bundle(&self) -> &Path {
        &self.bundle
    }

    /// Whether every stamped path, and the bundle's membership, is unchanged.
    pub(in crate::cargo) fn unchanged(&self) -> bool {
        if !cfg!(unix) || self.stamps.is_empty() {
            return false;
        }
        let mut current = BTreeMap::new();
        walk(&self.bundle, &mut current);
        current.keys().all(|path| self.stamps.contains_key(path))
            && self.stamps.iter().all(|(path, expected)| {
                current
                    .get(path)
                    .map_or_else(|| stamp(path), |stamp| *stamp)
                    == *expected
            })
    }
}

/// The length and SHA-256 every file of one successful replay was recorded
/// with. A later lookup of the same bundle compares them first: a file
/// whose current length or bytes differ proves the bundle stale, so its full
/// replay is not run only to refuse it. A file that cannot be shown to differ
/// (equal bytes, an unreadable path, too many unknown identities) leaves the
/// answer to the full replay.
pub(in crate::cargo) struct Recorded {
    files: Box<[(PathBuf, u64, [u8; 32])]>,
}

/// Files whose identity has no remembered digest that one check will hash.
const UNKNOWN_FILES: usize = 64;

impl Recorded {
    /// Files under `bundle` are left to the bundle's own control hashing,
    /// which reports their drift as corruption.
    pub(in crate::cargo) fn of(entries: &[Entry], bundle: &Path) -> Self {
        let mut files = Vec::new();
        for entry in entries {
            match entry {
                Entry::File {
                    path,
                    bytes,
                    digest,
                    ..
                } => files.push((PathBuf::from(path.as_ref()), *bytes as u64, *digest)),
                Entry::Directory {
                    path: root,
                    files: tree,
                    ..
                } => recorded_tree(&mut files, Path::new(root.as_ref()), tree),
                Entry::Git {
                    root, files: tree, ..
                } => recorded_tree(&mut files, Path::new(root.as_ref()), tree),
                _ => {}
            }
        }
        files.retain(|(path, _, _)| !path.starts_with(bundle));
        Self {
            files: files.into_boxed_slice(),
        }
    }

    /// Whether some recorded file's current bytes provably differ.
    pub(in crate::cargo) fn changed(&self) -> bool {
        use crate::file_digest::{self, Domain, Identity};
        let mut unknown = Vec::new();
        for (path, bytes, digest) in &self.files {
            let metadata = match std::fs::metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
                Err(_) => return false,
            };
            if !metadata.is_file() {
                return false;
            }
            if metadata.len() != *bytes {
                return true;
            }
            match file_digest::get(Identity::of(&metadata), Domain::Sha256) {
                Some(current) if current != *digest => return true,
                Some(_) => {}
                None if unknown.len() < UNKNOWN_FILES => unknown.push((path, digest)),
                None => return false,
            }
        }
        unknown.into_iter().any(|(path, digest)| {
            std::fs::read(path)
                .is_ok_and(|bytes| <[u8; 32]>::from(sha2::Sha256::digest(&bytes)) != *digest)
        })
    }
}

fn recorded_tree(files: &mut Vec<(PathBuf, u64, [u8; 32])>, root: &Path, tree: &[TreeFile]) {
    files.extend(tree.iter().map(|file| {
        (
            root.join(file.path.as_ref()),
            file.bytes as u64,
            file.digest,
        )
    }));
}

/// A recorded tree's root, files and their directories: an added or removed
/// member changes its directory's stamp.
fn tree(exact: &mut Vec<PathBuf>, root: &Path, files: &[TreeFile]) {
    exact.push(root.to_path_buf());
    for file in files {
        let file = root.join(file.path.as_ref());
        exact.extend(file.parent().map(Path::to_path_buf));
        exact.push(file);
    }
}

fn walk(path: &Path, stamps: &mut BTreeMap<PathBuf, Stamp>) {
    let stamp = stamp(path);
    let directory = stamp.is_some_and(|stamp| stamp[3] & 0o170_000 == 0o040_000);
    stamps.insert(path.to_path_buf(), stamp);
    if !directory {
        return;
    }
    let Ok(children) = std::fs::read_dir(path) else {
        return;
    };
    for child in children.flatten() {
        walk(&child.path(), stamps);
    }
}

#[cfg(unix)]
#[allow(
    clippy::cast_possible_wrap,
    reason = "stamps compare raw identities for equality only"
)]
fn stamp(path: &Path) -> Stamp {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::symlink_metadata(path).ok()?;
    Some([
        metadata.dev() as i64,
        metadata.ino() as i64,
        metadata.size() as i64,
        i64::from(metadata.mode()),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    ])
}

#[cfg(not(unix))]
fn stamp(_path: &Path) -> Stamp {
    None
}

#[cfg(test)]
mod tests {
    use super::Recorded;
    use crate::input::proof::{Entry, Role};
    use sha2::{Digest as _, Sha256};

    #[test]
    fn recorded_bytes_prove_a_changed_file_and_leave_equal_bytes_to_the_replay() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-scratch")
            .join(format!("recorded-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("lib.rs");
        let original = b"pub fn dial() -> u32 { 7 }\n";
        std::fs::write(&path, original).unwrap();
        let recorded = Recorded::of(
            &[Entry::File {
                role: Role::Authority,
                package_id: None,
                path: path.to_str().unwrap().into(),
                bytes: original.len(),
                digest: Sha256::digest(original).into(),
            }],
            &root.join("bundle"),
        );
        assert!(
            !recorded.changed(),
            "unchanged bytes are not a proven change"
        );
        std::fs::write(&path, b"pub fn dial() -> u32 { 8 }\n").unwrap();
        assert!(recorded.changed(), "equal length, different bytes");
        std::fs::write(&path, b"pub fn dial() -> u32 { 7 }\nfn more() {}\n").unwrap();
        assert!(recorded.changed(), "a different length");
        std::fs::write(&path, original).unwrap();
        assert!(!recorded.changed(), "an undo restores the recorded bytes");
        std::fs::remove_file(&path).unwrap();
        assert!(recorded.changed(), "a removed file");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
