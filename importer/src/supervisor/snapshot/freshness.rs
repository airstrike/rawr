//! Bounded current-file witness transformed from the exact snapshot transport.

use std::fs::{self, File, Metadata};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::node::Error;

pub(crate) struct Freshness {
    files: Box<[Source]>,
    snapshot: [u8; 32],
}

struct Source {
    path: PathBuf,
    bytes: usize,
    digest: [u8; 32],
}

impl super::Bytes {
    pub(crate) fn freshness(&self) -> Result<Freshness, Error> {
        let mut cursor = self
            .framed
            .strip_prefix(super::ENTRY_DOMAIN)
            .ok_or(Error::State("bound snapshot lost its framing"))?;
        let malformed = |_| Error::State("bound snapshot lost its canonical fields");
        // The transport was validated when it was bound; this re-parse only
        // reads the exact framed fields.
        super::take_string_field(&mut cursor).map_err(malformed)?;
        super::take_features(&mut cursor).map_err(malformed)?;
        let mut files = Vec::new();
        while !cursor.is_empty() {
            let path = super::take_string_field(&mut cursor).map_err(malformed)?;
            let contents = super::take_bytes(&mut cursor).map_err(malformed)?;
            files.try_reserve(1).map_err(|_| Error::Allocation)?;
            files.push(Source {
                path: PathBuf::from(path),
                bytes: contents.len(),
                digest: Sha256::digest(contents).into(),
            });
        }
        Ok(Freshness {
            files: files.into_boxed_slice(),
            snapshot: self.digest,
        })
    }
}

impl Freshness {
    pub(crate) fn matches(&self, snapshot: &super::Bytes) -> bool {
        self.snapshot == snapshot.digest
    }

    pub(crate) fn revalidate(&self) -> Result<(), Error> {
        for source in &self.files {
            source.revalidate()?;
        }
        Ok(())
    }
}

impl Source {
    fn revalidate(&self) -> Result<(), Error> {
        let io = |source| Error::Io {
            operation: "revalidate bound Rawr snapshot source",
            path: self.path.clone(),
            source,
        };
        if !self.path.is_absolute() || fs::canonicalize(&self.path).map_err(io)? != self.path {
            return Err(Error::Shape(self.path.clone()));
        }
        let before = fs::symlink_metadata(&self.path).map_err(io)?;
        if !before.is_file() || before.file_type().is_symlink() {
            return Err(Error::Shape(self.path.clone()));
        }
        if before.len() != self.bytes as u64 {
            return Err(drift(&self.path));
        }
        let mut file = File::open(&self.path).map_err(io)?;
        let opened = file.metadata().map_err(io)?;
        if !same(&before, &opened) {
            return Err(drift(&self.path));
        }
        // A file this process hashed at this very identity holds the same
        // bytes: only a file written since is read again (#1755).
        let identity = crate::file_digest::Identity::of(&opened);
        if crate::file_digest::get(identity, crate::file_digest::Domain::Sha256)
            .is_some_and(|digest| digest == self.digest)
        {
            return Ok(());
        }
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 16 * 1024];
        let mut remaining = self.bytes;
        while remaining != 0 {
            let length = remaining.min(buffer.len());
            let read = file.read(&mut buffer[..length]).map_err(io)?;
            if read == 0 {
                return Err(drift(&self.path));
            }
            digest.update(&buffer[..read]);
            remaining -= read;
        }
        if file.read(&mut buffer[..1]).map_err(io)? != 0
            || <[u8; 32]>::from(digest.finalize()) != self.digest
            || !same(&opened, &file.metadata().map_err(io)?)
            || !same(&opened, &fs::symlink_metadata(&self.path).map_err(io)?)
            || fs::canonicalize(&self.path).map_err(io)? != self.path
        {
            return Err(drift(&self.path));
        }
        crate::file_digest::insert(identity, crate::file_digest::Domain::Sha256, self.digest);
        Ok(())
    }
}

/// A save after the snapshot was read: a newer preparation follows it.
fn drift(path: &Path) -> Error {
    Error::SourceChanged(path.to_path_buf())
}

fn same(left: &Metadata, right: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.mode() == right.mode()
            && left.len() == right.len()
            && left.mtime() == right.mtime()
            && left.mtime_nsec() == right.mtime_nsec()
            && left.ctime() == right.ctime()
            && left.ctime_nsec() == right.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        left.is_file() == right.is_file()
            && left.len() == right.len()
            && left.modified().ok() == right.modified().ok()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn fixture() -> (PathBuf, super::super::Bytes) {
        let root = std::env::temp_dir().join(format!(
            "rawr-snapshot-freshness-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let path = root.join("main.rs");
        fs::write(&path, b"before").unwrap();
        let limits = crate::limits::Limits::standard();
        let path = path.to_str().unwrap();
        let mut snapshot = crate::surface::Snapshot::builder(&limits);
        snapshot.entry(path).unwrap();
        snapshot.file(path, b"before").unwrap();
        let snapshot = snapshot.seal().unwrap();
        let mut bytes = super::super::Bytes::capture(&limits);
        bytes.entry(path).unwrap();
        bytes.file(path, b"before").unwrap();
        (root, bytes.bind(snapshot).unwrap())
    }

    #[test]
    fn bound_snapshot_rejects_silent_same_length_source_drift_and_path_aliases() {
        let (root, bytes) = fixture();
        let freshness = bytes.freshness().unwrap();
        freshness.revalidate().unwrap();
        fs::write(root.join("main.rs"), b"after!").unwrap();
        assert!(matches!(
            freshness.revalidate(),
            Err(Error::SourceChanged(_))
        ));
        fs::remove_file(root.join("main.rs")).unwrap();
        fs::write(root.join("other.rs"), b"before").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("other.rs"), root.join("main.rs")).unwrap();
            assert!(matches!(freshness.revalidate(), Err(Error::Shape(_))));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn freshness_charges_exact_decode_hash_and_path_work_before_it_runs() {
        let (root, bytes) = fixture();
        let freshness = bytes.freshness().unwrap();
        let source = &freshness.files[0];
        freshness.revalidate().unwrap();
        // A nonexistent file distinguishes the precharged traversal, charged
        // before the path is touched, from the I/O failure the work then reports.
        fs::remove_file(&source.path).unwrap();
        assert!(matches!(
            freshness.revalidate(),
            Err(Error::Io {
                operation: "revalidate bound Rawr snapshot source",
                ..
            })
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
