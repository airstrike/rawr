//! Bounded current-file witness transformed from the exact snapshot transport.

use std::fs::{self, File, Metadata};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::node::{Error, GraphBudget};

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
    pub(crate) fn freshness(
        &self,
        budget: &GraphBudget,
        limits: &crate::cargo::Limits,
    ) -> Result<Freshness, Error> {
        budget.decode(self.framed.len())?;
        let mut cursor = self
            .framed
            .strip_prefix(super::ENTRY_DOMAIN)
            .ok_or(Error::State("bound snapshot lost its framing"))?;
        let malformed = |_| Error::State("bound snapshot lost its canonical fields");
        // The transport was validated against its accounting targets when it
        // was bound; this re-parse only reads the exact framed fields.
        let _ = limits;
        super::take_string_field(&mut cursor).map_err(malformed)?;
        let mut files = Vec::new();
        while !cursor.is_empty() {
            let path = super::take_string_field(&mut cursor).map_err(malformed)?;
            let contents = super::take_bytes(&mut cursor).map_err(malformed)?;
            budget.paths(1)?;
            budget.hash(contents.len())?;
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

    pub(crate) fn revalidate(&self, budget: &GraphBudget) -> Result<(), Error> {
        for source in &self.files {
            source.revalidate(budget)?;
        }
        Ok(())
    }
}

impl Source {
    fn revalidate(&self, budget: &GraphBudget) -> Result<(), Error> {
        // Canonical spelling rejects aliases and symlink parents as well as a
        // symlink at the leaf. Charge the traversal before touching the path.
        budget.paths(
            self.path
                .components()
                .count()
                .saturating_mul(2)
                .saturating_add(5),
        )?;
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
        budget.hash(self.bytes)?;
        let mut file = File::open(&self.path).map_err(io)?;
        let opened = file.metadata().map_err(io)?;
        if !same(&before, &opened) {
            return Err(drift(&self.path));
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
        Ok(())
    }
}

fn drift(path: &Path) -> Error {
    Error::Corrupt {
        path: path.to_path_buf(),
        reason: "bound Rawr snapshot source changed before admission",
    }
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
        let path = root.join("main.rawr");
        fs::write(&path, b"before").unwrap();
        let limits = crate::cargo::Limits::standard();
        let path = path.to_str().unwrap();
        let mut snapshot = crate::surface::Snapshot::builder(&limits);
        snapshot.entry(path).unwrap();
        snapshot.file(path, b"before").unwrap();
        let snapshot = snapshot.seal().unwrap();
        let mut bytes = super::super::Bytes::capture(&limits);
        bytes.entry(path).unwrap();
        bytes.file(path, b"before").unwrap();
        (root, bytes.bind(snapshot, &limits).unwrap())
    }

    #[test]
    fn bound_snapshot_rejects_silent_same_length_source_drift_and_path_aliases() {
        let (root, bytes) = fixture();
        let freshness = bytes
            .freshness(&GraphBudget::new(), &crate::cargo::Limits::standard())
            .unwrap();
        freshness.revalidate(&GraphBudget::new()).unwrap();
        fs::write(root.join("main.rawr"), b"after!").unwrap();
        assert!(matches!(
            freshness.revalidate(&GraphBudget::new()),
            Err(Error::Corrupt { .. })
        ));
        fs::remove_file(root.join("main.rawr")).unwrap();
        fs::write(root.join("other.rawr"), b"before").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("other.rawr"), root.join("main.rawr")).unwrap();
            assert!(matches!(
                freshness.revalidate(&GraphBudget::new()),
                Err(Error::Shape(_))
            ));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn freshness_charges_exact_decode_hash_and_path_caps_before_work() {
        let (root, bytes) = fixture();
        let limits = crate::cargo::Limits::standard();
        // Each cap is an accounting target. The exact remaining amount
        // records nothing; one unit short still completes the work and records
        // an exceedance whose high water is exactly one past the target, which
        // proves the exact charge without any refusal.
        let high_water = |budget: &GraphBudget, resource: &str| {
            budget
                .exceedances()
                .axis(resource)
                .map(|axis| axis.high_water)
        };
        for short in [false, true] {
            let budget = GraphBudget::new();
            budget
                .decode(
                    crate::node::MAX_DECODED_BYTES_PER_GRAPH as usize - bytes.framed.len()
                        + usize::from(short),
                )
                .unwrap();
            bytes
                .freshness(&budget, &limits)
                .expect("an accounting axis never refuses");
            assert_eq!(
                high_water(&budget, crate::node::DECODED_RESOURCE),
                short.then_some(crate::node::MAX_DECODED_BYTES_PER_GRAPH + 1)
            );
        }
        let freshness = bytes.freshness(&GraphBudget::new(), &limits).unwrap();
        let source = &freshness.files[0];
        for short in [false, true] {
            let budget = GraphBudget::new();
            budget
                .hash(
                    crate::node::MAX_HASHED_BYTES_PER_GRAPH as usize - source.bytes
                        + usize::from(short),
                )
                .unwrap();
            freshness
                .revalidate(&budget)
                .expect("an accounting axis never refuses");
            assert_eq!(
                high_water(&budget, crate::node::HASHED_RESOURCE),
                short.then_some(crate::node::MAX_HASHED_BYTES_PER_GRAPH + 1)
            );
        }
        let charge = source.path.components().count() * 2 + 5;
        let budget = GraphBudget::new();
        budget
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - charge)
            .unwrap();
        freshness.revalidate(&budget).unwrap();
        assert_eq!(high_water(&budget, crate::node::PATHS_RESOURCE), None);
        // A nonexistent file distinguishes the precharged traversal, recorded
        // before the path is touched, from the I/O failure the work then reports.
        fs::remove_file(&source.path).unwrap();
        let budget = GraphBudget::new();
        budget
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - charge + 1)
            .unwrap();
        assert!(matches!(
            freshness.revalidate(&budget),
            Err(Error::Io {
                operation: "revalidate bound Rawr snapshot source",
                ..
            })
        ));
        assert_eq!(
            high_water(&budget, crate::node::PATHS_RESOURCE),
            Some(crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)
        );
        fs::remove_dir_all(root).unwrap();
    }
}
