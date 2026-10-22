//! Explicit Cargo registry/git inputs copied into a newly owned fixture home.

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Component, Path, PathBuf};

use rawr_import::metrics::cell::project;
use sha2::{Digest as _, Sha256};

use super::isolation;

const BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MEMBERS: usize = 200_000;

/// Only these explicit cache inputs can be copied. No Cargo-home sweep occurs.
pub(in crate::import::baseline) struct Inputs {
    pub registry: PathBuf,
    pub git: PathBuf,
}

#[derive(Debug, serde::Serialize)]
pub(in crate::import::baseline) struct Evidence {
    registry: Copied,
    git: Copied,
}

#[derive(Debug, serde::Serialize)]
struct Copied {
    source: PathBuf,
    source_identity: [u64; 2],
    destination: PathBuf,
    raw: project::Transition,
}

#[derive(Debug, thiserror::Error)]
pub(in crate::import::baseline) enum Error {
    #[error(transparent)]
    Home(#[from] isolation::Error),
    #[error(transparent)]
    Manifest(#[from] rawr_import::metrics::cell::Error),
    #[error("explicit Cargo cache seed failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Cargo cache seed shape or identity changed at {0}")]
    Changed(PathBuf),
    #[error("combined Cargo cache seed exceeds the bounded copy envelope")]
    Limit,
}

impl Inputs {
    pub(in crate::import::baseline) fn copy(
        &self,
        home: &isolation::Home,
    ) -> Result<Evidence, Error> {
        home.verify()?;
        let registry = Source::capture(&self.registry)?;
        let git = Source::capture(&self.git)?;
        let bytes = registry
            .manifest
            .state
            .bytes
            .checked_add(git.manifest.state.bytes);
        let members = registry
            .manifest
            .members
            .len()
            .checked_add(registry.manifest.directories.len())
            .and_then(|count| count.checked_add(git.manifest.members.len()))
            .and_then(|count| count.checked_add(git.manifest.directories.len()));
        if bytes.is_none_or(|bytes| bytes > BYTES)
            || members.is_none_or(|members| members > MEMBERS)
        {
            return Err(Error::Limit);
        }
        let registry = registry.copy(home, "registry")?;
        let git = git.copy(home, "git")?;
        home.verify()?;
        Ok(Evidence { registry, git })
    }
}

struct Source {
    root: PathBuf,
    identity: [u64; 2],
    held: File,
    manifest: project::Manifest,
}

impl Source {
    fn capture(root: &Path) -> Result<Self, Error> {
        let identity = directory(root)?;
        let held = File::open(root).map_err(|source| io(root, source))?;
        if association(&held.metadata().map_err(|source| io(root, source))?) != identity {
            return Err(Error::Changed(root.into()));
        }
        let manifest = project::Manifest::read(root)?;
        if directory(root)? != identity {
            return Err(Error::Changed(root.into()));
        }
        Ok(Self {
            root: root.into(),
            identity,
            held,
            manifest,
        })
    }

    fn verify(&self) -> Result<(), Error> {
        let held = self
            .held
            .metadata()
            .map_err(|source| io(&self.root, source))?;
        if !held.is_dir()
            || association(&held) != self.identity
            || directory(&self.root)? != self.identity
        {
            return Err(Error::Changed(self.root.clone()));
        }
        Ok(())
    }

    fn copy(self, home: &isolation::Home, name: &str) -> Result<Copied, Error> {
        home.verify()?;
        self.verify()?;
        let destination = home.root().join(name);
        fs::create_dir(&destination).map_err(|source| io(&destination, source))?;
        let destination_identity = prepare_directory(&destination)?;
        if self
            .manifest
            .directories
            .first()
            .is_none_or(|row| !row.path.is_empty())
        {
            return Err(Error::Changed(self.root));
        }
        // The manifest's sorted prefix order creates every captured ancestor
        // before its descendants, including directories containing no files.
        let mut directories = Vec::new();
        directories
            .try_reserve_exact(self.manifest.directories.len())
            .map_err(|_| Error::Limit)?;
        directories.push((0, destination_identity));
        for (index, row) in self.manifest.directories.iter().enumerate().skip(1) {
            home.verify()?;
            self.verify()?;
            if directory(&destination)? != destination_identity {
                return Err(Error::Changed(destination.clone()));
            }
            let relative = relative(&row.path)?;
            parents(&destination, &relative)?;
            let target = destination.join(relative);
            fs::create_dir(&target).map_err(|source| io(&target, source))?;
            directories.push((index, prepare_directory(&target)?));
        }
        let mut buffer = [0_u8; 64 * 1024];
        for member in &self.manifest.members {
            home.verify()?;
            self.verify()?;
            if directory(&destination)? != destination_identity {
                return Err(Error::Changed(destination.clone()));
            }
            let relative = relative(&member.path)?;
            let source = self.root.join(&relative);
            let target = destination.join(&relative);
            parents(&destination, &relative)?;
            let metadata = regular(&source)?;
            if metadata.len() != member.bytes || metadata.mode() & 0o7777 != member.mode {
                return Err(Error::Changed(source));
            }
            let mut input = File::open(&source).map_err(|error| io(&source, error))?;
            let opened = input.metadata().map_err(|error| io(&source, error))?;
            if !same(&metadata, &opened) {
                return Err(Error::Changed(source));
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|source| io(&target, source))?;
            let mut digest = Sha256::new();
            let mut count = 0_u64;
            loop {
                let read = input
                    .read(&mut buffer)
                    .map_err(|error| io(&source, error))?;
                if read == 0 {
                    break;
                }
                count = count.checked_add(read as u64).ok_or(Error::Limit)?;
                if count > member.bytes {
                    return Err(Error::Changed(source));
                }
                digest.update(&buffer[..read]);
                output
                    .write_all(&buffer[..read])
                    .map_err(|source| io(&target, source))?;
            }
            let observed: [u8; 32] = digest.finalize().into();
            if count != member.bytes
                || observed != member.digest
                || !same(&metadata, &regular(&source)?)
            {
                return Err(Error::Changed(source));
            }
            output
                .set_permissions(fs::Permissions::from_mode(member.mode))
                .map_err(|source| io(&target, source))?;
            output.sync_all().map_err(|source| io(&target, source))?;
            if !same(
                &output.metadata().map_err(|source| io(&target, source))?,
                &regular(&target)?,
            ) {
                return Err(Error::Changed(target));
            }
        }
        // Permission restrictions are applied only after contents exist. Child
        // directories retire their writable preparation state before parents.
        directories.sort_unstable_by_key(|(index, _)| {
            let path = &self.manifest.directories[*index].path;
            std::cmp::Reverse(if path.is_empty() {
                0
            } else {
                1 + path.iter().filter(|byte| **byte == b'/').count()
            })
        });
        for (index, identity) in directories {
            let row = &self.manifest.directories[index];
            let target = if row.path.is_empty() {
                destination.clone()
            } else {
                destination.join(relative(&row.path)?)
            };
            if directory(&target)? != identity {
                return Err(Error::Changed(target));
            }
            let held = File::open(&target).map_err(|source| io(&target, source))?;
            if association(&held.metadata().map_err(|source| io(&target, source))?) != identity {
                return Err(Error::Changed(target));
            }
            held.set_permissions(fs::Permissions::from_mode(row.mode))
                .map_err(|source| io(&target, source))?;
            held.sync_all().map_err(|source| io(&target, source))?;
            if directory(&target)? != identity {
                return Err(Error::Changed(target));
            }
        }
        home.verify()?;
        self.verify()?;
        if directory(&destination)? != destination_identity
            || project::Manifest::read(&self.root)? != self.manifest
        {
            return Err(Error::Changed(self.root));
        }
        let copied = project::Manifest::read(&destination)?;
        if copied != self.manifest {
            return Err(Error::Changed(destination));
        }
        Ok(Copied {
            source: self.root,
            source_identity: self.identity,
            destination,
            raw: project::Transition::transform(self.manifest, copied),
        })
    }
}

fn parents(root: &Path, relative: &Path) -> Result<(), Error> {
    let parent = relative
        .parent()
        .ok_or_else(|| Error::Changed(relative.into()))?;
    let mut current = root.to_path_buf();
    for part in parent.components() {
        let Component::Normal(part) = part else {
            return Err(Error::Changed(relative.into()));
        };
        current.push(part);
        // Every ancestor must already have its own captured directory row.
        directory(&current)?;
    }
    Ok(())
}

fn relative(bytes: &[u8]) -> Result<PathBuf, Error> {
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()));
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Error::Changed(path));
    }
    Ok(path)
}

fn prepare_directory(path: &Path) -> Result<[u64; 2], Error> {
    let identity = directory(path)?;
    let held = File::open(path).map_err(|source| io(path, source))?;
    if association(&held.metadata().map_err(|source| io(path, source))?) != identity {
        return Err(Error::Changed(path.into()));
    }
    held.set_permissions(fs::Permissions::from_mode(0o700))
        .map_err(|source| io(path, source))?;
    if directory(path)? != identity {
        return Err(Error::Changed(path.into()));
    }
    Ok(identity)
}

fn directory(path: &Path) -> Result<[u64; 2], Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| io(path, source))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || fs::canonicalize(path).map_err(|source| io(path, source))? != path
    {
        return Err(Error::Changed(path.into()));
    }
    Ok(association(&metadata))
}

fn regular(path: &Path) -> Result<fs::Metadata, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| io(path, source))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.nlink() != 1
        || fs::canonicalize(path).map_err(|source| io(path, source))? != path
    {
        return Err(Error::Changed(path.into()));
    }
    Ok(metadata)
}

fn association(metadata: &fs::Metadata) -> [u64; 2] {
    [metadata.dev(), metadata.ino()]
}
fn same(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    right.is_file()
        && right.nlink() == 1
        && association(left) == association(right)
        && left.len() == right.len()
        && left.mode() == right.mode()
}
fn io(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rawr-corruption-seed-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            let path = fs::canonicalize(path).unwrap();
            for directory in ["owned", "registry", "git"] {
                fs::create_dir(path.join(directory)).unwrap();
            }
            fs::write(path.join("registry/member"), b"registry bytes").unwrap();
            fs::write(path.join("git/member"), b"git bytes").unwrap();
            fs::write(path.join("credentials.toml"), b"must not be copied").unwrap();
            Self(path)
        }
        fn inputs(&self) -> super::Inputs {
            super::Inputs {
                registry: self.0.join("registry"),
                git: self.0.join("git"),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn explicit_cache_inputs_copy_exact_bytes_without_ambient_configuration() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.0.join("registry/nested/empty")).unwrap();
        fs::write(fixture.0.join("registry/nested/member"), b"nested bytes").unwrap();
        for (path, mode) in [
            ("registry", 0o750),
            ("registry/nested", 0o550),
            ("registry/nested/empty", 0o1750),
            ("registry/member", 0o740),
        ] {
            fs::set_permissions(fixture.0.join(path), fs::Permissions::from_mode(mode)).unwrap();
        }
        let home = super::isolation::Home::create(&fixture.0.join("owned")).unwrap();
        let evidence = fixture.inputs().copy(&home).unwrap();
        assert!(evidence.registry.raw.changes.is_empty());
        assert!(evidence.git.raw.changes.is_empty());
        assert!(evidence.registry.raw.directory_changes.is_empty());
        assert!(evidence.git.raw.directory_changes.is_empty());
        assert_eq!(evidence.registry.raw.before, evidence.registry.raw.after);
        assert_eq!(evidence.registry.raw.after.directories.len(), 3);
        assert!(home.root().join("registry/nested/empty").is_dir());
        assert_eq!(
            fs::read(home.root().join("registry/member")).unwrap(),
            b"registry bytes"
        );
        assert!(!home.root().join("credentials.toml").exists());
        assert!(!home.root().join("config.toml").exists());
        assert!(fixture.inputs().copy(&home).is_err());
        // Let the fixture's ordinary recursive cleanup remove both trees.
        for root in [fixture.0.join("registry"), home.root().join("registry")] {
            fs::set_permissions(root.join("nested"), fs::Permissions::from_mode(0o750)).unwrap();
        }
    }

    #[test]
    fn captured_seed_permission_drift_is_not_silently_filtered() {
        let fixture = Fixture::new();
        let home = super::isolation::Home::create(&fixture.0.join("owned")).unwrap();
        fs::set_permissions(
            fixture.0.join("registry/member"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        let source = super::Source::capture(&fixture.0.join("registry")).unwrap();
        fs::set_permissions(
            fixture.0.join("registry/member"),
            fs::Permissions::from_mode(0o740),
        )
        .unwrap();
        assert!(matches!(
            source.copy(&home, "registry"),
            Err(super::Error::Changed(_))
        ));
    }

    #[test]
    fn cache_seed_rejects_links_and_preserves_external_sources() {
        let fixture = Fixture::new();
        let home = super::isolation::Home::create(&fixture.0.join("owned")).unwrap();
        symlink(
            fixture.0.join("credentials.toml"),
            fixture.0.join("registry/alias"),
        )
        .unwrap();
        assert!(fixture.inputs().copy(&home).is_err());
        assert_eq!(
            fs::read(fixture.0.join("credentials.toml")).unwrap(),
            b"must not be copied"
        );
        assert!(!home.root().join("registry").exists());
    }
}
