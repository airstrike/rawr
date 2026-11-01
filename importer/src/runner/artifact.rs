//! Supervisor-owned post-link artifact sealing.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

pub(crate) mod retained;

/// Exact final executable and Cargo-output evidence created after linking.
pub struct BuiltArtifactProof {
    manifest: [u8; 32],
    executable: File,
    out_dir: PathBuf,
    outputs: Box<[File]>,
}

struct File {
    path: PathBuf,
    bytes: u64,
    digest: [u8; 32],
}

impl BuiltArtifactProof {
    pub(crate) fn seal(
        manifest: &super::manifest::GeneratedManifest,
        executable: &Path,
        out_dir: &Path,
        outputs: &[PathBuf],
    ) -> Result<Self, Error> {
        Self::seal_with_budget(manifest, executable, out_dir, outputs, None)
    }

    pub(crate) fn seal_budgeted(
        manifest: &super::manifest::GeneratedManifest,
        executable: &Path,
        out_dir: &Path,
        outputs: &[PathBuf],
        budget: &crate::node::GraphBudget,
    ) -> Result<Self, Error> {
        Self::seal_with_budget(manifest, executable, out_dir, outputs, Some(budget))
    }

    fn seal_with_budget(
        manifest: &super::manifest::GeneratedManifest,
        executable: &Path,
        out_dir: &Path,
        outputs: &[PathBuf],
        budget: Option<&crate::node::GraphBudget>,
    ) -> Result<Self, Error> {
        let out_dir = canonical_directory_budgeted(out_dir, budget)?;
        let executable = canonical_file_budgeted(executable, budget)?;
        if !executable.starts_with(&out_dir) {
            return Err(Error::OutsideOutput(executable));
        }
        // Artifacts are hashed as streams: nothing is buffered, so an
        // executable of any size Cargo links seals.
        let executable = hash(executable, budget)?;
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(outputs.len())
            .map_err(|_| Error::Allocation)?;
        let mut previous = None;
        for output in outputs {
            let output = canonical_file_budgeted(output, budget)?;
            if !output.starts_with(&out_dir) {
                return Err(Error::OutsideOutput(output));
            }
            if previous
                .as_ref()
                .is_some_and(|prior: &PathBuf| prior >= &output)
            {
                return Err(Error::OutputOrder);
            }
            if output == executable.path {
                return Err(Error::DuplicateExecutable);
            }
            let record = hash(output.clone(), budget)?;
            previous = Some(output);
            retained.push(record);
        }
        Ok(Self {
            manifest: manifest.digest(),
            executable,
            out_dir,
            outputs: retained.into_boxed_slice(),
        })
    }

    /// Rehash every sealed artifact before lookup or admission.
    pub fn revalidate(&self) -> Result<(), Error> {
        self.revalidate_with_budget(None)
    }

    pub(crate) fn revalidate_budgeted(
        &self,
        budget: &crate::node::GraphBudget,
    ) -> Result<(), Error> {
        self.revalidate_with_budget(Some(budget))
    }

    fn revalidate_with_budget(
        &self,
        budget: Option<&crate::node::GraphBudget>,
    ) -> Result<(), Error> {
        let out_dir = canonical_directory_budgeted(&self.out_dir, budget)?;
        if out_dir != self.out_dir {
            return Err(Error::Drift);
        }
        let executable = hash(
            canonical_file_budgeted(&self.executable.path, budget)?,
            budget,
        )?;
        if executable.bytes != self.executable.bytes || executable.digest != self.executable.digest
        {
            return Err(Error::Drift);
        }
        for expected in &self.outputs {
            let actual = hash(canonical_file_budgeted(&expected.path, budget)?, budget)?;
            if actual.bytes != expected.bytes || actual.digest != expected.digest {
                return Err(Error::Drift);
            }
        }
        Ok(())
    }

    pub(crate) fn executable(&self) -> &Path {
        &self.executable.path
    }

    pub(crate) fn executable_digest(&self) -> [u8; 32] {
        self.executable.digest
    }

    pub(crate) fn executable_bytes(&self) -> u64 {
        self.executable.bytes
    }

    pub(crate) fn digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.built-artifact/v1");
        digest.update(self.manifest);
        digest.update(self.executable.bytes.to_le_bytes());
        digest.update(self.executable.digest);
        for output in &self.outputs {
            digest.update(output.bytes.to_le_bytes());
            digest.update(output.digest);
        }
        digest.finalize().into()
    }
}

fn canonical_directory_budgeted(
    path: &Path,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<PathBuf, Error> {
    match budget {
        Some(_) => Ok(retained::directory(path)?),
        None => canonical_directory(path),
    }
}

fn canonical_file_budgeted(
    path: &Path,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<PathBuf, Error> {
    if budget.is_some() {
        let parent = path
            .parent()
            .ok_or_else(|| Error::Shape(path.to_path_buf()))?;
        if retained::directory(parent)? != parent {
            return Err(Error::Drift);
        }
    }
    canonical_file(path)
}

fn canonical_directory(path: &Path) -> Result<PathBuf, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect artifact output directory",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::Shape(path.to_path_buf()));
    }
    fs::canonicalize(path).map_err(|source| Error::Io {
        operation: "canonicalize artifact output directory",
        path: path.to_path_buf(),
        source,
    })
}

fn canonical_file(path: &Path) -> Result<PathBuf, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect artifact file",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Error::Shape(path.to_path_buf()));
    }
    fs::canonicalize(path).map_err(|source| Error::Io {
        operation: "canonicalize artifact file",
        path: path.to_path_buf(),
        source,
    })
}

/// Hashes the artifact as a stream. The read is not bounded: Cargo links
/// executables of any size, and nothing here is held in memory.
fn hash(path: PathBuf, budget: Option<&crate::node::GraphBudget>) -> Result<File, Error> {
    if budget.is_some() {
        let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
            operation: "inspect budgeted artifact file",
            path: path.clone(),
            source,
        })?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || crate::store::bundle::hard_linked(&metadata)
        {
            return Err(Error::Shape(path));
        }
        let digest = retained::hash(&path, &metadata)?;
        return Ok(File {
            path,
            bytes: metadata.len(),
            digest,
        });
    }
    let mut source = fs::File::open(&path).map_err(|source| Error::Io {
        operation: "open artifact file",
        path: path.clone(),
        source,
    })?;
    let length = source
        .metadata()
        .map_err(|source| Error::Io {
            operation: "inspect artifact file length",
            path: path.clone(),
            source,
        })?
        .len();
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        let read = source.read(&mut buffer).map_err(|source| Error::Io {
            operation: "hash artifact file",
            path: path.clone(),
            source,
        })?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        digest.update(&buffer[..read]);
    }
    if bytes != length {
        return Err(Error::Drift);
    }
    Ok(File {
        path,
        bytes,
        digest: digest.finalize().into(),
    })
}

/// Independent artifact sealing or revalidation failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("artifact graph validation failed: {0}")]
    Node(#[from] crate::node::Error),
    #[error("artifact I/O failed while attempting to {operation} at {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("artifact path has an unsupported shape: {0}")]
    Shape(PathBuf),
    #[error("artifact is outside its fixed Cargo output directory: {0}")]
    OutsideOutput(PathBuf),
    #[error("artifact output paths are duplicated or not canonically ordered")]
    OutputOrder,
    #[error("the executable was also supplied as a secondary output")]
    DuplicateExecutable,
    #[error("artifact changed after it was sealed")]
    Drift,
    #[error("cannot allocate artifact proof")]
    Allocation,
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static FIXTURE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn artifact_canonicalization_charges_its_exact_allocations() {
        let root = std::env::temp_dir().join(format!(
            "rawr-runner-artifact-canonical-budget-{}-{}",
            std::process::id(),
            FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let executable = root.join("runner");
        fs::write(&executable, b"inert").unwrap();
        assert!(executable.as_os_str().as_encoded_bytes().len() < 384);
        // Parent traversal owns one path and canonicalizes it; the artifact
        // canonicalization owns another result. Each realpath output overlaps
        // its copied Rust bytes, even when the resulting spelling is short.
        let budget = crate::node::GraphBudget::new();
        assert_eq!(
            canonical_file_budgeted(&executable, Some(&budget)).unwrap(),
            executable
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// #518: a debug runner can pass 256 MiB. Cargo links it, so it seals:
    /// the executable is hashed as a stream with no read limit.
    #[test]
    fn an_executable_past_the_child_capture_bound_seals() {
        let root = std::env::temp_dir().join(format!(
            "rawr-runner-artifact-large-{}-{}",
            std::process::id(),
            FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let executable = root.join("runner");
        let length = crate::process::CAPTURE_BYTES as u64 + 1;
        // Sparse: the file reads as zeros and takes no disk space.
        fs::File::create(&executable)
            .unwrap()
            .set_len(length)
            .unwrap();
        let manifest = crate::runner::manifest::GeneratedManifest::new(
            crate::bridge::BuildRequestKey::fixture([1; 32]),
            Box::default(),
            Box::default(),
            Box::default(),
            [0; 32],
        );
        let budget = crate::node::GraphBudget::new();
        let sealed = BuiltArtifactProof::seal_budgeted(&manifest, &executable, &root, &[], &budget)
            .expect("an executable of any size seals");
        assert_eq!(sealed.executable_bytes(), length);
        sealed.revalidate_budgeted(&budget).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artifact_hash_and_path_checks_spend_the_shared_graph_budget() {
        let root = std::env::temp_dir().join(format!(
            "rawr-runner-artifact-budget-{}-{}",
            std::process::id(),
            FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("the artifact budget root exists");
        let root = fs::canonicalize(root).expect("the artifact budget root canonicalizes");
        let executable = root.join("runner");
        fs::write(&executable, b"four").expect("the artifact fixture writes");

        // Canonicalization spends path checks on the shared graph.
        let measured = crate::node::GraphBudget::new();
        canonical_file_budgeted(&executable, Some(&measured))
            .expect("the fresh graph canonicalizes the artifact");

        // Four content bytes plus the EOF probe byte are hashed.
        let hashes = crate::node::GraphBudget::new();
        let hashed = hash(executable, Some(&hashes)).expect("the artifact hashes");
        assert_eq!(hashed.bytes, 4);
        fs::remove_dir_all(root).expect("the artifact budget fixture retires");
    }
}
