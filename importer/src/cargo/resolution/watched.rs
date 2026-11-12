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

#[derive(Clone, Default)]
pub(in crate::cargo) struct Watched {
    /// The tree whose membership is stamped as well, when there is one.
    bundle: Option<PathBuf>,
    stamps: BTreeMap<PathBuf, Stamp>,
}

impl Watched {
    /// Stamp the bundle tree, every recorded input path and the namespace.
    pub(in crate::cargo) fn observe(
        bundle: &Path,
        entries: &[Entry],
        namespace: Option<&namespace::Retained>,
    ) -> Self {
        let mut watched = Self::exact(entries, namespace);
        walk(bundle, &mut watched.stamps);
        watched.bundle = Some(bundle.to_path_buf());
        watched
    }

    /// Stamp every path `entries` read, with no bundle: taken before those
    /// paths are read again, they prove while they match that the reading
    /// saw what it sees now.
    pub(in crate::cargo) fn observe_entries(entries: &[Entry]) -> Self {
        Self::exact(entries, None)
    }

    fn exact(entries: &[Entry], namespace: Option<&namespace::Retained>) -> Self {
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
        for path in exact {
            let stamp = stamp(&path);
            stamps.insert(path, stamp);
        }
        Self {
            bundle: None,
            stamps,
        }
    }

    /// The bundle root whose tree was stamped.
    pub(in crate::cargo) fn bundle(&self) -> Option<&Path> {
        self.bundle.as_deref()
    }

    /// Whether every stamped path, and the bundle's membership, is unchanged.
    pub(in crate::cargo) fn unchanged(&self) -> bool {
        if !cfg!(unix) || self.stamps.is_empty() {
            return false;
        }
        let mut current = BTreeMap::new();
        if let Some(bundle) = &self.bundle {
            walk(bundle, &mut current);
        }
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

/// The encoding a later process reads a [`Recorded`] back from.
const MAGIC: &[u8; 8] = b"RAWRRCD1";

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

    /// These files as bytes [`Self::decode`] reads back.
    #[cfg(unix)]
    pub(in crate::cargo) fn encode(&self) -> Vec<u8> {
        use std::os::unix::ffi::OsStrExt as _;
        let mut bytes = Vec::from(*MAGIC);
        bytes.extend_from_slice(&(self.files.len() as u64).to_le_bytes());
        for (path, length, digest) in &self.files {
            let path = path.as_os_str().as_bytes();
            bytes.extend_from_slice(&(path.len() as u64).to_le_bytes());
            bytes.extend_from_slice(path);
            bytes.extend_from_slice(&length.to_le_bytes());
            bytes.extend_from_slice(digest);
        }
        bytes
    }

    /// The files [`Self::encode`] wrote, or `None` for bytes it did not.
    #[cfg(unix)]
    pub(in crate::cargo) fn decode(bytes: &[u8]) -> Option<Self> {
        use std::os::unix::ffi::OsStrExt as _;
        let mut rest = bytes.strip_prefix(MAGIC)?;
        let mut take = |count: usize| {
            let (taken, after) = rest.split_at_checked(count)?;
            rest = after;
            Some(taken)
        };
        let scalar = |bytes: &[u8]| Some(u64::from_le_bytes(bytes.try_into().ok()?));
        let count = usize::try_from(scalar(take(8)?)?).ok()?;
        // Each file takes at least its three fixed fields.
        if count > bytes.len() / 48 {
            return None;
        }
        let mut files = Vec::with_capacity(count);
        for _ in 0..count {
            let path = usize::try_from(scalar(take(8)?)?).ok()?;
            let path = PathBuf::from(std::ffi::OsStr::from_bytes(take(path)?));
            let length = scalar(take(8)?)?;
            let digest = take(32)?.try_into().ok()?;
            files.push((path, length, digest));
        }
        rest.is_empty().then(|| Self {
            files: files.into_boxed_slice(),
        })
    }

    #[cfg(not(unix))]
    pub(in crate::cargo) fn encode(&self) -> Vec<u8> {
        Vec::new()
    }

    #[cfg(not(unix))]
    pub(in crate::cargo) fn decode(_bytes: &[u8]) -> Option<Self> {
        None
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
        // A later process reads the same record back, and the same answers.
        let decoded = Recorded::decode(&recorded.encode()).expect("a record decodes");
        assert_eq!(decoded.encode(), recorded.encode());
        std::fs::write(&path, b"pub fn dial() -> u32 { 8 }\n").unwrap();
        assert!(decoded.changed(), "a decoded record proves the change");
        let encoded = recorded.encode();
        assert!(Recorded::decode(&encoded[..encoded.len() - 1]).is_none());
        assert!(Recorded::decode(&[encoded.as_slice(), &[0]].concat()).is_none());
        std::fs::remove_file(&path).unwrap();
        assert!(recorded.changed(), "a removed file");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
