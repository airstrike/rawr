//! Canonical baseline fixture project and persistent-cache identities.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::Error;

const PATH_BYTES: usize = 16 * 1024;
const SOURCE_MEMBER_BYTES: u64 = 4 * 1024 * 1024;
const CACHE_MEMBER_BYTES: u64 = 1_207_959_552;
const CACHE_MEMBERS: usize = 200_000;
const CACHE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const SOURCE_FILES: [&str; 5] = [
    "Cargo.toml",
    "rawr.toml",
    "native/Cargo.toml",
    "native/src/lib.rs",
    "src/main.rawr",
];

/// Content byte total and versioned file/directory/permission aggregate.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct State {
    pub bytes: u64,
    pub digest: [u8; 32],
}

/// One exact member in a complete bounded cache manifest.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ManifestMember {
    pub path: Box<[u8]>,
    pub bytes: u64,
    pub digest: [u8; 32],
    pub mode: u32,
}

/// One reached directory. The empty path represents the present cache root.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Directory {
    pub path: Box<[u8]>,
    pub mode: u32,
}

/// Complete non-truncated cache state and per-member evidence.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Manifest {
    pub state: State,
    pub members: Box<[ManifestMember]>,
    /// Empty only when symlink_metadata reported the root absent.
    pub directories: Box<[Directory]>,
}

/// One exact member-level durable transition.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Change {
    pub path: Box<[u8]>,
    pub before: Option<MemberState>,
    pub after: Option<MemberState>,
}

/// Content length/digest and permission bits at one transition endpoint.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct MemberState {
    pub bytes: u64,
    pub digest: [u8; 32],
    pub mode: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct DirectoryChange {
    pub path: Box<[u8]>,
    pub before: Option<u32>,
    pub after: Option<u32>,
}

/// Complete raw cache transition owned by the baseline parent.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Transition {
    pub before: Manifest,
    pub after: Manifest,
    pub changes: Box<[Change]>,
    pub directory_changes: Box<[DirectoryChange]>,
}

/// Canonical fixture root, semantic input tree, and cache prestate.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Identity {
    pub path: Box<[u8]>,
    pub device: u64,
    pub inode: u64,
    pub source_digest: [u8; 32],
    pub cache: State,
}

struct Member {
    path: PathBuf,
    device: u64,
    inode: u64,
    bytes: u64,
    mode: u32,
}

impl Identity {
    /// Seal the exact fixed fixture inputs while excluding runtime title files.
    pub fn read(root: &Path) -> Result<Self, Error> {
        let cache = Manifest::read(&root.join(".rawr"))?;
        Self::read_with_manifest(root, &cache)
    }

    /// Bind an already-sealed complete cache manifest to the fixed source tree.
    pub fn read_with_manifest(root: &Path, cache: &Manifest) -> Result<Self, Error> {
        let metadata = fs::symlink_metadata(root).map_err(Error::Io)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::Project("root type"));
        }
        let canonical = fs::canonicalize(root).map_err(Error::Io)?;
        if canonical != root || !canonical.is_absolute() {
            return Err(Error::Project("canonical root"));
        }
        let path = canonical.as_os_str().as_bytes();
        if path.is_empty() || path.len() > PATH_BYTES {
            return Err(Error::Project("root path"));
        }
        let mut digest = Sha256::new();
        digest.update(b"rawr.phase9.cell.project-source/v1");
        for relative in SOURCE_FILES {
            let file = canonical.join(relative);
            let file_metadata = fs::symlink_metadata(&file).map_err(Error::Io)?;
            if file_metadata.file_type().is_symlink()
                || !file_metadata.is_file()
                || file_metadata.len() > SOURCE_MEMBER_BYTES
            {
                return Err(Error::Project("source member"));
            }
            let bytes = fs::read(&file).map_err(Error::Io)?;
            if bytes.len() as u64 != file_metadata.len() {
                return Err(Error::Project("source replacement"));
            }
            digest.update((relative.len() as u64).to_le_bytes());
            digest.update(relative.as_bytes());
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        }
        let identity = Self {
            path: path.into(),
            device: metadata.dev(),
            inode: metadata.ino(),
            source_digest: digest.finalize().into(),
            cache: cache.state.clone(),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.path.is_empty()
            || self.path.len() > PATH_BYTES
            || !self.path.starts_with(b"/")
            || self.device == 0
            || self.inode == 0
            || self.source_digest == [0; 32]
            || self.cache.digest == [0; 32]
            || self.cache.bytes > CACHE_BYTES
        {
            return Err(Error::Project("identity"));
        }
        Ok(())
    }

    /// Recover the validated canonical project path.
    pub fn path(&self) -> Result<PathBuf, Error> {
        use std::os::unix::ffi::OsStringExt as _;

        if self.path.contains(&0) {
            return Err(Error::Project("path bytes"));
        }
        Ok(PathBuf::from(std::ffi::OsString::from_vec(
            self.path.to_vec(),
        )))
    }
}

impl State {
    /// Seal one bounded cache tree without following links.
    pub fn read(root: &Path) -> Result<Self, Error> {
        Ok(Manifest::read(root)?.state)
    }
}

impl Manifest {
    /// Seal reached file bytes, permissions and directory presence in one walk.
    /// This is not an atomic snapshot. The existing population ceiling now
    /// includes every file and directory, including a present root.
    pub fn read(root: &Path) -> Result<Self, Error> {
        let metadata = match fs::symlink_metadata(root) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    state: State {
                        bytes: 0,
                        digest: Sha256::digest(b"rawr.phase9.absent-cache/v2").into(),
                    },
                    members: Box::default(),
                    directories: Box::default(),
                });
            }
            Err(source) => return Err(Error::Io(source)),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::Project("cache root"));
        }
        let mut files = Vec::new();
        let mut directories = Vec::new();
        directories
            .try_reserve(1)
            .map_err(|_| Error::Project("cache allocation"))?;
        directories.push(Directory {
            path: Box::default(),
            mode: metadata.mode() & 0o7777,
        });
        let mut population = 1;
        collect(root, root, &mut files, &mut directories, &mut population)?;
        files.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        directories.sort_unstable_by(|left, right| left.path.cmp(&right.path));
        if files.windows(2).any(|pair| pair[0].0 == pair[1].0)
            || directories
                .windows(2)
                .any(|pair| pair[0].path == pair[1].path)
        {
            return Err(Error::Project("cache duplicate path"));
        }
        let mut bytes = 0_u64;
        let mut digest = Sha256::new();
        digest.update(b"rawr.phase9.cell.cache/v2");
        digest.update((directories.len() as u64).to_le_bytes());
        for directory in &directories {
            digest.update(b"directory");
            digest.update((directory.path.len() as u64).to_le_bytes());
            digest.update(&directory.path);
            digest.update(directory.mode.to_le_bytes());
        }
        digest.update((files.len() as u64).to_le_bytes());
        let mut buffer = [0_u8; 64 * 1024];
        let mut members = Vec::new();
        members
            .try_reserve_exact(files.len())
            .map_err(|_| Error::Project("cache allocation"))?;
        for (relative, member) in files {
            bytes = bytes
                .checked_add(member.bytes)
                .ok_or(Error::Project("cache bytes"))?;
            if bytes > CACHE_BYTES {
                return Err(Error::Project("cache bytes"));
            }
            digest.update(b"file");
            digest.update((relative.len() as u64).to_le_bytes());
            digest.update(&relative);
            digest.update(member.mode.to_le_bytes());
            digest.update(member.bytes.to_le_bytes());
            let member_digest = member.hash(&mut digest, &mut buffer)?;
            members.push(ManifestMember {
                path: relative,
                bytes: member.bytes,
                digest: member_digest,
                mode: member.mode,
            });
        }
        Ok(Self {
            state: State {
                bytes,
                digest: digest.finalize().into(),
            },
            members: members.into_boxed_slice(),
            directories: directories.into_boxed_slice(),
        })
    }
}

impl Transition {
    /// Transform two sealed manifests into one complete mutation ledger.
    pub fn transform(before: Manifest, after: Manifest) -> Self {
        let before_by_path = before
            .members
            .iter()
            .map(|member| (member.path.as_ref(), member))
            .collect::<BTreeMap<_, _>>();
        let after_by_path = after
            .members
            .iter()
            .map(|member| (member.path.as_ref(), member))
            .collect::<BTreeMap<_, _>>();
        let mut paths = before_by_path
            .keys()
            .chain(after_by_path.keys())
            .copied()
            .collect::<Vec<_>>();
        paths.sort_unstable();
        paths.dedup();
        let changes = paths
            .into_iter()
            .filter_map(|path| {
                let before_member = before_by_path.get(path).copied();
                let after_member = after_by_path.get(path).copied();
                if before_member == after_member {
                    return None;
                }
                Some(Change {
                    path: path.into(),
                    before: before_member.map(ManifestMember::state),
                    after: after_member.map(ManifestMember::state),
                })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let before_dirs = before
            .directories
            .iter()
            .map(|directory| (directory.path.as_ref(), directory.mode))
            .collect::<BTreeMap<_, _>>();
        let after_dirs = after
            .directories
            .iter()
            .map(|directory| (directory.path.as_ref(), directory.mode))
            .collect::<BTreeMap<_, _>>();
        let mut paths = before_dirs
            .keys()
            .chain(after_dirs.keys())
            .copied()
            .collect::<Vec<_>>();
        paths.sort_unstable();
        paths.dedup();
        let directory_changes = paths
            .into_iter()
            .filter_map(|path| {
                let before = before_dirs.get(path).copied();
                let after = after_dirs.get(path).copied();
                (before != after).then(|| DirectoryChange {
                    path: path.into(),
                    before,
                    after,
                })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            before,
            after,
            changes,
            directory_changes,
        }
    }
}

impl ManifestMember {
    fn state(&self) -> MemberState {
        MemberState {
            bytes: self.bytes,
            digest: self.digest,
            mode: self.mode,
        }
    }
}

fn collect(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(Box<[u8]>, Member)>,
    directories: &mut Vec<Directory>,
    population: &mut usize,
) -> Result<(), Error> {
    let prefix = directory
        .strip_prefix(root)
        .map_err(|_| Error::Project("cache relative path"))?
        .as_os_str()
        .as_bytes();
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let name = entry.file_name();
        let length = prefix
            .len()
            .checked_add(usize::from(!prefix.is_empty()))
            .and_then(|length| length.checked_add(name.as_bytes().len()))
            .ok_or(Error::Project("cache relative path"))?;
        if length == 0 || length > PATH_BYTES {
            return Err(Error::Project("cache relative path"));
        }
        if *population >= CACHE_MEMBERS {
            return Err(Error::Project("cache member count"));
        }
        *population += 1;
        entries
            .try_reserve(1)
            .map_err(|_| Error::Project("cache allocation"))?;
        entries.push((name, entry));
    }
    entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    for (_, entry) in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(Error::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(Error::Project("cache symlink"));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| Error::Project("cache relative path"))?
            .as_os_str()
            .as_bytes();
        if relative.is_empty() || relative.len() > PATH_BYTES {
            return Err(Error::Project("cache relative path"));
        }
        let mut copied = Vec::new();
        copied
            .try_reserve_exact(relative.len())
            .map_err(|_| Error::Project("cache allocation"))?;
        copied.extend_from_slice(relative);
        let relative = copied.into_boxed_slice();
        if metadata.is_dir() {
            directories
                .try_reserve(1)
                .map_err(|_| Error::Project("cache allocation"))?;
            directories.push(Directory {
                path: relative,
                mode: metadata.mode() & 0o7777,
            });
            collect(root, &path, files, directories, population)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(Error::Project("cache member type"));
        }
        if metadata.len() > CACHE_MEMBER_BYTES {
            return Err(Error::Project("cache member bytes"));
        }
        files
            .try_reserve(1)
            .map_err(|_| Error::Project("cache allocation"))?;
        files.push((
            relative,
            Member {
                path,
                device: metadata.dev(),
                inode: metadata.ino(),
                bytes: metadata.len(),
                mode: metadata.mode() & 0o7777,
            },
        ));
    }
    Ok(())
}

impl Member {
    fn hash(&self, digest: &mut Sha256, buffer: &mut [u8]) -> Result<[u8; 32], Error> {
        let mut file = File::open(&self.path).map_err(Error::Io)?;
        self.validate(&file.metadata().map_err(Error::Io)?)?;
        let mut observed = 0_u64;
        let mut member_digest = Sha256::new();
        loop {
            let read = file.read(buffer).map_err(Error::Io)?;
            if read == 0 {
                break;
            }
            observed = observed
                .checked_add(read as u64)
                .ok_or(Error::Project("cache member bytes"))?;
            if observed > self.bytes {
                return Err(Error::Project("cache replacement"));
            }
            digest.update(&buffer[..read]);
            member_digest.update(&buffer[..read]);
        }
        if observed != self.bytes {
            return Err(Error::Project("cache replacement"));
        }
        self.validate(&file.metadata().map_err(Error::Io)?)?;
        self.validate(&fs::symlink_metadata(&self.path).map_err(Error::Io)?)?;
        Ok(member_digest.finalize().into())
    }

    fn validate(&self, metadata: &fs::Metadata) -> Result<(), Error> {
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
            || metadata.len() != self.bytes
            || metadata.mode() & 0o7777 != self.mode
        {
            return Err(Error::Project("cache replacement"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn cache_state_streams_members_under_the_governed_canonical_ceiling() {
        assert_eq!(super::CACHE_MEMBER_BYTES, 1_207_959_552);
        let root = std::env::temp_dir().join(format!(
            "rawr-cell-cache-state-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&root).expect("cache root creates");
        let member = root.join("member.bin");
        std::fs::write(&member, b"first-state").expect("first cache member writes");
        let first = super::State::read(&root).expect("first cache state seals");
        std::fs::write(&member, b"other-state").expect("replacement cache member writes");
        let second = super::State::read(&root).expect("second cache state seals");
        assert_eq!(first.bytes, 11);
        assert_eq!(second.bytes, 11);
        assert_ne!(first.digest, second.digest);
        std::fs::remove_dir_all(root).expect("cache fixture retires");
    }

    #[test]
    fn cache_transition_retains_every_changed_member_without_truncation() {
        let root = std::env::temp_dir().join(format!(
            "rawr-cell-cache-transition-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&root).expect("cache root creates");
        for index in 0..40 {
            std::fs::write(root.join(format!("member-{index:02}")), b"before")
                .expect("initial member writes");
        }
        let before = super::Manifest::read(&root).expect("before manifest seals");
        for index in 0..40 {
            std::fs::write(root.join(format!("member-{index:02}")), b"after-")
                .expect("changed member writes");
        }
        let after = super::Manifest::read(&root).expect("after manifest seals");
        let transition = super::Transition::transform(before.clone(), after.clone());
        assert_eq!(transition.before, before);
        assert_eq!(transition.after, after);
        assert_eq!(transition.changes.len(), 40);
        assert!(transition.directory_changes.is_empty());
        assert!(transition.changes.iter().all(|change| {
            change.before.as_ref().is_some_and(|state| state.bytes == 6)
                && change.after.as_ref().is_some_and(|state| state.bytes == 6)
        }));
        std::fs::remove_dir_all(root).expect("cache fixture retires");
    }

    #[test]
    fn cache_endpoints_distinguish_absence_directories_modes_and_type_changes() {
        let root = std::env::temp_dir().join(format!(
            "rawr-cell-cache-shape-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        let absent = super::Manifest::read(&root).unwrap();
        assert!(absent.members.is_empty() && absent.directories.is_empty());
        std::fs::create_dir(&root).unwrap();
        let present = super::Manifest::read(&root).unwrap();
        assert_eq!(present.state.bytes, absent.state.bytes);
        assert_ne!(present.state.digest, absent.state.digest);
        assert_eq!(present.directories.len(), 1);
        assert!(present.directories[0].path.is_empty());
        let appeared = super::Transition::transform(absent, present.clone());
        assert!(appeared.changes.is_empty());
        assert_eq!(appeared.directory_changes.len(), 1);
        assert_eq!(appeared.directory_changes[0].before, None);

        std::fs::create_dir(root.join("empty")).unwrap();
        let empty = super::Manifest::read(&root).unwrap();
        assert_eq!(empty.state.bytes, present.state.bytes);
        assert_ne!(empty.state.digest, present.state.digest);
        assert_eq!(empty.directories[1].path.as_ref(), b"empty");
        std::fs::write(root.join("member"), b"same").unwrap();
        std::fs::set_permissions(root.join("member"), std::fs::Permissions::from_mode(0o640))
            .unwrap();
        let before = super::Manifest::read(&root).unwrap();
        std::fs::set_permissions(root.join("member"), std::fs::Permissions::from_mode(0o740))
            .unwrap();
        std::fs::set_permissions(
            &root,
            std::fs::Permissions::from_mode(present.directories[0].mode ^ 0o1000),
        )
        .unwrap();
        let after = super::Manifest::read(&root).unwrap();
        assert_eq!(before.state.bytes, after.state.bytes);
        assert_eq!(before.members[0].digest, after.members[0].digest);
        assert_ne!(before.state.digest, after.state.digest);
        let modes = super::Transition::transform(before, after.clone());
        assert_eq!(modes.changes.len(), 1);
        assert_eq!(modes.changes[0].before.as_ref().unwrap().mode, 0o640);
        assert_eq!(modes.changes[0].after.as_ref().unwrap().mode, 0o740);
        assert_eq!(modes.directory_changes.len(), 1);
        assert!(modes.directory_changes[0].path.is_empty());

        std::fs::remove_file(root.join("member")).unwrap();
        std::fs::create_dir(root.join("member")).unwrap();
        std::fs::write(root.join("member/child"), b"same").unwrap();
        let replaced = super::Manifest::read(&root).unwrap();
        assert_eq!(replaced.state.bytes, after.state.bytes);
        let transition = super::Transition::transform(after, replaced);
        assert_eq!(transition.changes.len(), 2);
        assert_eq!(transition.directory_changes.len(), 1);
        assert_eq!(transition.directory_changes[0].path.as_ref(), b"member");
        let mut legacy = serde_json::to_value(&transition.after).unwrap();
        legacy.as_object_mut().unwrap().remove("directories");
        assert!(serde_json::from_value::<super::Manifest>(legacy).is_err());
        let mut legacy = serde_json::to_value(&transition.after).unwrap();
        legacy["members"][0].as_object_mut().unwrap().remove("mode");
        assert!(serde_json::from_value::<super::Manifest>(legacy).is_err());
        std::fs::remove_dir_all(&root).unwrap();
        symlink("absent-target", &root).unwrap();
        assert!(matches!(
            super::Manifest::read(&root),
            Err(super::Error::Project("cache root"))
        ));
        std::fs::remove_file(root).unwrap();
    }

    #[test]
    fn cache_population_refuses_before_retaining_another_entry() {
        let root = std::env::temp_dir().join(format!(
            "rawr-cell-cache-population-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("empty")).unwrap();
        let mut files = Vec::new();
        let mut directories = Vec::new();
        let mut population = super::CACHE_MEMBERS;
        assert!(matches!(
            super::collect(&root, &root, &mut files, &mut directories, &mut population),
            Err(super::Error::Project("cache member count"))
        ));
        assert!(files.is_empty() && directories.is_empty());
        assert_eq!(population, super::CACHE_MEMBERS);
        std::fs::remove_dir_all(root).unwrap();
    }
}
