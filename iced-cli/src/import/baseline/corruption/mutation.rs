//! One exact, parent-owned equal-extent corruption under the real bundle lease.

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek as _, Write as _};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use rawr_import::{
    context::Context,
    metrics::{pipeline::target::Target, recorder::Snapshot, report::Scenario},
};
use sha2::{Digest as _, Sha256};

use super::isolation;

const FILE_BYTES: u64 = 1_207_959_552;

pub(super) struct Selected {
    target: Target,
    store: PathBuf,
    bundle: PathBuf,
    file: PathBuf,
    lease: PathBuf,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct Injected {
    pub target: Target,
    pub relative: Box<[u8]>,
    pub before: Member,
    pub after: Member,
    offset: u64,
    original: u8,
    corrupted: u8,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct Member {
    pub device: u64,
    pub inode: u64,
    pub bytes: u64,
    pub mode: u32,
    pub digest: [u8; 32],
    xor_first_digest: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error(transparent)]
    Home(#[from] isolation::Error),
    #[error("corruption scenario has no exact observed bundle target")]
    Missing,
    #[error("corruption target is not bound to the parent-owned isolated Cargo home")]
    Context,
    #[error("corruption target shape or identity changed at {0}")]
    Changed(PathBuf),
    #[error("corruption fixture I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("corruption fixture refused a live or unavailable bundle lease at {path}: {source}")]
    Lease {
        path: PathBuf,
        #[source]
        source: std::fs::TryLockError,
    },
    #[error(
        "corruption attempt failed: {original}; restoring the exact mode also failed: {restore}"
    )]
    Restore {
        original: Box<Error>,
        restore: Box<Error>,
    },
}

impl Selected {
    pub(super) fn from_setup(
        home: &isolation::Home,
        context: &Context,
        scenario: Scenario,
        counters: &Snapshot,
    ) -> Result<Self, Error> {
        home.verify()?;
        if context.cargo_home() != home.root() {
            return Err(Error::Context);
        }
        let pipeline = counters.pipeline.as_ref().ok_or(Error::Missing)?;
        let resolution = pipeline.resolution_target.ok_or(Error::Missing)?;
        let runner = pipeline.runner_target.ok_or(Error::Missing)?;
        if resolution == runner {
            return Err(Error::Missing);
        }
        let target = match scenario {
            Scenario::CorruptRunner => runner,
            Scenario::CorruptResolution => resolution,
            _ => return Err(Error::Missing),
        };
        let store = home.root().join("rawr/v1");
        directory(&home.root().join("rawr"))?;
        directory(&store)?;
        let bundle = store.join(target.bundle_relative());
        directory(&bundle)?;
        let file = match scenario {
            Scenario::CorruptRunner => {
                let runner = counters.runner.as_ref().ok_or(Error::Missing)?;
                PathBuf::from(std::ffi::OsString::from_vec(
                    runner.executable_path.to_vec(),
                ))
            }
            Scenario::CorruptResolution => store.join(target.record_relative()),
            _ => return Err(Error::Missing),
        };
        // The bundle association comes from the actual owner; a path prefix alone
        // never supplies either installation ownership or the association.
        if !file.starts_with(&bundle) || file == bundle {
            return Err(Error::Changed(file));
        }
        regular(&file)?;
        let lease = store.join(target.lease_relative());
        regular(&lease)?;
        Ok(Self {
            target,
            store,
            bundle,
            file,
            lease,
        })
    }

    pub(super) fn inject(self, home: &isolation::Home) -> Result<Injected, Error> {
        home.verify()?;
        let store_identity = directory(&self.store)?;
        let bundle_identity = directory(&self.bundle)?;
        let lease_metadata = regular(&self.lease)?;
        let lease = File::open(&self.lease).map_err(|source| io(&self.lease, source))?;
        if !same(
            &lease_metadata,
            &lease.metadata().map_err(|source| io(&self.lease, source))?,
        ) {
            return Err(Error::Changed(self.lease));
        }
        lease.try_lock().map_err(|source| Error::Lease {
            path: self.lease.clone(),
            source,
        })?;
        if !same(&lease_metadata, &regular(&self.lease)?) {
            return Err(Error::Changed(self.lease));
        }
        let metadata = regular(&self.file)?;
        if metadata.len() == 0 || metadata.len() > FILE_BYTES {
            return Err(Error::Changed(self.file));
        }
        let mut held = File::open(&self.file).map_err(|source| io(&self.file, source))?;
        if !same(
            &metadata,
            &held.metadata().map_err(|source| io(&self.file, source))?,
        ) {
            return Err(Error::Changed(self.file));
        }
        let before = capture(&self.file, &mut held, &metadata)?;
        held.seek(std::io::SeekFrom::Start(0))
            .map_err(|source| io(&self.file, source))?;
        let mut byte = [0_u8; 1];
        held.read_exact(&mut byte)
            .map_err(|source| io(&self.file, source))?;
        let original = byte[0];
        byte[0] ^= 1;
        held.set_permissions(fs::Permissions::from_mode(metadata.mode() | 0o200))
            .map_err(|source| io(&self.file, source))?;
        let changed = (|| {
            home.verify()?;
            let writable = regular(&self.file)?;
            if association(&writable) != association(&metadata) || writable.len() != metadata.len()
            {
                return Err(Error::Changed(self.file.clone()));
            }
            let mut output = OpenOptions::new()
                .write(true)
                .open(&self.file)
                .map_err(|source| io(&self.file, source))?;
            if !same(
                &writable,
                &output.metadata().map_err(|source| io(&self.file, source))?,
            ) {
                return Err(Error::Changed(self.file.clone()));
            }
            output
                .write_all(&byte)
                .map_err(|source| io(&self.file, source))?;
            output.sync_all().map_err(|source| io(&self.file, source))?;
            Ok(())
        })();
        let restored = held
            .set_permissions(fs::Permissions::from_mode(metadata.mode()))
            .map_err(|source| io(&self.file, source));
        match (changed, restored) {
            (Err(original), Err(restore)) => {
                return Err(Error::Restore {
                    original: Box::new(original),
                    restore: Box::new(restore),
                })
            }
            (Err(error), Ok(())) | (Ok(()), Err(error)) => return Err(error),
            (Ok(()), Ok(())) => {}
        }
        let after = capture(&self.file, &mut held, &metadata)?;
        home.verify()?;
        if directory(&self.store)? != store_identity
            || directory(&self.bundle)? != bundle_identity
            || !same(&lease_metadata, &regular(&self.lease)?)
            || before.digest == after.digest
            || after.digest != before.xor_first_digest
            || before.digest != after.xor_first_digest
        {
            return Err(Error::Changed(self.file));
        }
        let relative = self
            .file
            .strip_prefix(&self.store)
            .map_err(|_| Error::Context)?
            .as_os_str()
            .as_bytes()
            .into();
        // The lease drops before the sole ordinary retained child performs repair.
        Ok(Injected {
            target: self.target,
            relative,
            before,
            after,
            offset: 0,
            original,
            corrupted: byte[0],
        })
    }
}

fn capture(path: &Path, file: &mut File, metadata: &fs::Metadata) -> Result<Member, Error> {
    file.seek(std::io::SeekFrom::Start(0))
        .map_err(|source| io(path, source))?;
    let mut digest = Sha256::new();
    let mut xor_first_digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        let count = file.read(&mut buffer).map_err(|source| io(path, source))?;
        if count == 0 {
            break;
        }
        let first = bytes == 0;
        bytes = bytes
            .checked_add(count as u64)
            .ok_or_else(|| Error::Changed(path.into()))?;
        if bytes > metadata.len() {
            return Err(Error::Changed(path.into()));
        }
        digest.update(&buffer[..count]);
        if first {
            buffer[0] ^= 1;
        }
        xor_first_digest.update(&buffer[..count]);
    }
    if bytes != metadata.len()
        || !same(metadata, &regular(path)?)
        || !same(
            metadata,
            &file.metadata().map_err(|source| io(path, source))?,
        )
    {
        return Err(Error::Changed(path.into()));
    }
    Ok(Member {
        device: metadata.dev(),
        inode: metadata.ino(),
        bytes,
        mode: metadata.mode(),
        digest: digest.finalize().into(),
        xor_first_digest: xor_first_digest.finalize().into(),
    })
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

    struct Fixture {
        root: std::path::PathBuf,
        home: super::isolation::Home,
        target: super::Target,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "rawr-corruption-mutation-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let root = fs::canonicalize(root).unwrap();
            let home = super::isolation::Home::create(&root).unwrap();
            let target = serde_json::from_value(
                serde_json::json!({"execution": vec![1; 32], "bundle": vec![2; 32]}),
            )
            .unwrap();
            let fixture = Self { root, home, target };
            let selected = fixture.selected();
            fs::create_dir_all(selected.file.parent().unwrap()).unwrap();
            fs::write(&selected.file, b"original immutable bytes").unwrap();
            fs::set_permissions(&selected.file, fs::Permissions::from_mode(0o444)).unwrap();
            fs::write(&selected.lease, b"fixture lease").unwrap();
            fixture
        }
        fn selected(&self) -> super::Selected {
            let store = self.home.root().join("rawr/v1");
            super::Selected {
                target: self.target,
                bundle: store.join(self.target.bundle_relative()),
                file: store.join(self.target.record_relative()),
                lease: store.join(self.target.lease_relative()),
                store,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn injection_changes_exactly_one_byte_and_restores_extent_and_mode() {
        let fixture = Fixture::new();
        let selected = fixture.selected();
        let file = selected.file.clone();
        let before = fs::read(&file).unwrap();
        let injected = selected.inject(&fixture.home).unwrap();
        let after = fs::read(&file).unwrap();
        assert_eq!(before.len(), after.len());
        assert_eq!(
            before
                .iter()
                .zip(&after)
                .filter(|(left, right)| left != right)
                .count(),
            1
        );
        assert_eq!(injected.before.bytes, injected.after.bytes);
        assert_eq!(injected.before.mode, injected.after.mode);
        assert_eq!(injected.before.inode, injected.after.inode);
        assert_ne!(injected.before.digest, injected.after.digest);
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o444
        );
    }

    #[test]
    fn live_bundle_lease_refuses_before_any_byte_or_mode_change() {
        let fixture = Fixture::new();
        let selected = fixture.selected();
        let file = selected.file.clone();
        let held = fs::File::open(&selected.lease).unwrap();
        held.lock_shared().unwrap();
        assert!(matches!(
            selected.inject(&fixture.home),
            Err(super::Error::Lease { .. })
        ));
        assert_eq!(fs::read(&file).unwrap(), b"original immutable bytes");
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o444
        );
    }

    #[test]
    fn target_symlink_is_not_mutation_authority() {
        let fixture = Fixture::new();
        let selected = fixture.selected();
        let outside = fixture.root.join("outside");
        fs::write(&outside, b"untouched").unwrap();
        fs::remove_file(&selected.file).unwrap();
        symlink(&outside, &selected.file).unwrap();
        assert!(selected.inject(&fixture.home).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"untouched");
    }
}
