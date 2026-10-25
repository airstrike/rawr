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
    byte_limit: usize,
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
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        Self::seal_with_budget(manifest, executable, out_dir, outputs, limits, None)
    }

    pub(crate) fn seal_budgeted(
        manifest: &super::manifest::GeneratedManifest,
        executable: &Path,
        out_dir: &Path,
        outputs: &[PathBuf],
        limits: &crate::cargo::Limits,
        budget: &crate::node::GraphBudget,
    ) -> Result<Self, Error> {
        Self::seal_with_budget(manifest, executable, out_dir, outputs, limits, Some(budget))
    }

    fn seal_with_budget(
        manifest: &super::manifest::GeneratedManifest,
        executable: &Path,
        out_dir: &Path,
        outputs: &[PathBuf],
        limits: &crate::cargo::Limits,
        budget: Option<&crate::node::GraphBudget>,
    ) -> Result<Self, Error> {
        let out_dir = canonical_directory_budgeted(out_dir, budget)?;
        let executable = canonical_file_budgeted(executable, budget)?;
        if !executable.starts_with(&out_dir) {
            return Err(Error::OutsideOutput(executable));
        }
        // Each artifact file is an artifact-class read bound to the
        // physical `process::CAPTURE_BYTES`. The cumulative size against the
        // JSON target is accounting: it is recorded on a runner account and
        // sealing continues.
        let mut account = crate::surface::account::Account::new(crate::exceedance::Account::Runner);
        let executable = hash(executable, crate::process::CAPTURE_BYTES, budget)?;
        if let Some(budget) = budget {
            budget.decode(
                outputs
                    .len()
                    .checked_mul(std::mem::size_of::<File>())
                    .ok_or(Error::Allocation)?,
            )?;
        }
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(outputs.len())
            .map_err(|_| Error::Allocation)?;
        let mut total = usize::try_from(executable.bytes).map_err(|_| Error::Limit)?;
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
            let record = hash(output.clone(), crate::process::CAPTURE_BYTES, budget)?;
            total = total
                .checked_add(usize::try_from(record.bytes).map_err(|_| Error::Limit)?)
                .ok_or(Error::Limit)?;
            previous = Some(output);
            retained.push(record);
        }
        account.exceeded("runner artifact bytes", limits.json_bytes, total);
        Ok(Self {
            manifest: manifest.digest(),
            executable,
            out_dir,
            outputs: retained.into_boxed_slice(),
            byte_limit: crate::process::CAPTURE_BYTES,
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
            self.byte_limit,
            budget,
        )?;
        if executable.bytes != self.executable.bytes || executable.digest != self.executable.digest
        {
            return Err(Error::Drift);
        }
        for expected in &self.outputs {
            let actual = hash(
                canonical_file_budgeted(&expected.path, budget)?,
                self.byte_limit,
                budget,
            )?;
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
        Some(budget) => Ok(retained::directory(path, budget)?),
        None => canonical_directory(path),
    }
}

fn canonical_file_budgeted(
    path: &Path,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<PathBuf, Error> {
    if let Some(budget) = budget {
        let parent = path
            .parent()
            .ok_or_else(|| Error::Shape(path.to_path_buf()))?;
        if retained::directory(parent, budget)? != parent {
            return Err(Error::Drift);
        }
        budget.paths(2)?;
        crate::store::path::input(path, budget)?;
        crate::store::path::canonical(path, budget)?;
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

fn hash(
    path: PathBuf,
    limit: usize,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<File, Error> {
    if let Some(budget) = budget {
        budget.paths(1)?;
        crate::store::path::input(&path, budget)?;
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
        if metadata.len() > limit as u64 {
            return Err(Error::Limit);
        }
        let digest = retained::hash(&path, &metadata, limit, budget)?;
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
    if length > u64::try_from(limit).unwrap_or(u64::MAX) {
        return Err(Error::Limit);
    }
    if let Some(budget) = budget {
        budget.hash(usize::try_from(length).map_err(|_| Error::Limit)?)?;
    }
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
        bytes = bytes.checked_add(read as u64).ok_or(Error::Limit)?;
        if bytes > u64::try_from(limit).unwrap_or(u64::MAX) {
            return Err(Error::Limit);
        }
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
    #[error("artifact byte limit was exceeded")]
    Limit,
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
    fn artifact_canonicalization_admits_exact_allocations_and_records_one_byte_floor() {
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
        let required = 3 * std::mem::size_of::<PathBuf>()
            + root.as_os_str().as_encoded_bytes().len()
            + 1
            + 2 * (1024 + 1023);
        let paths = root.components().count() + 2;
        for floor in [false, true] {
            let budget = crate::node::GraphBudget::new();
            budget
                .decode(
                    crate::node::MAX_DECODED_BYTES_PER_GRAPH as usize - required
                        + usize::from(floor),
                )
                .unwrap();
            budget
                .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - paths)
                .unwrap();
            // The exact allocation fills both targets with no
            // exceedance; one byte less remaining still canonicalizes and
            // records the decoded axis at exactly target + 1.
            assert_eq!(
                canonical_file_budgeted(&executable, Some(&budget)).unwrap(),
                executable
            );
            if floor {
                let exceeded = budget.exceedances();
                let axis = exceeded
                    .axis("typed store decoded candidate bytes per graph")
                    .unwrap();
                assert_eq!(axis.target, crate::node::MAX_DECODED_BYTES_PER_GRAPH);
                assert_eq!(
                    axis.first_requested,
                    crate::node::MAX_DECODED_BYTES_PER_GRAPH + 1
                );
                assert_eq!(
                    axis.high_water,
                    crate::node::MAX_DECODED_BYTES_PER_GRAPH + 1
                );
                assert!(exceeded.axis(crate::node::PATHS_RESOURCE).is_none());
            } else {
                assert!(!budget.exceedances().is_exceeded());
                let (decoded, paths, _) = budget.census_usage().unwrap();
                assert_eq!(decoded, crate::node::MAX_DECODED_BYTES_PER_GRAPH);
                assert_eq!(paths, crate::node::MAX_PATH_CHECKS_PER_GRAPH);
                budget.decode(1).unwrap();
                budget.paths(1).unwrap();
                let exceeded = budget.exceedances();
                assert_eq!(
                    exceeded
                        .axis(crate::node::DECODED_RESOURCE)
                        .unwrap()
                        .high_water,
                    crate::node::MAX_DECODED_BYTES_PER_GRAPH + 1
                );
                assert_eq!(
                    exceeded
                        .axis(crate::node::PATHS_RESOURCE)
                        .unwrap()
                        .high_water,
                    crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
                );
            }
        }
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

        // A graph at its path target still canonicalizes and records
        // the shared path axis at exactly target + the canonicalization's own
        // checks.
        let measured = crate::node::GraphBudget::new();
        canonical_file_budgeted(&executable, Some(&measured))
            .expect("the fresh graph canonicalizes the artifact");
        let canonical_paths = measured.census_usage().unwrap().1;
        assert!(canonical_paths > 0);
        let paths = crate::node::GraphBudget::new();
        paths
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("the exact path cap precharges");
        assert_eq!(
            canonical_file_budgeted(&executable, Some(&paths))
                .expect("the spent graph still canonicalizes"),
            fs::canonicalize(&executable).unwrap()
        );
        let axis = paths
            .exceedances()
            .axis("typed store candidate path checks per graph")
            .copied()
            .unwrap();
        assert_eq!(
            axis.high_water,
            (crate::node::MAX_PATH_CHECKS_PER_GRAPH + canonical_paths) as u64
        );

        // Four content bytes plus the EOF probe byte cross a target three
        // below the cap by exactly two bytes.
        let hashes = crate::node::GraphBudget::new();
        hashes
            .hash((crate::node::MAX_HASHED_BYTES_PER_GRAPH - 3) as usize)
            .expect("the three-below hash cap precharges");
        let hashed = hash(executable, 4, Some(&hashes)).expect("the spent graph still hashes");
        assert_eq!(hashed.bytes, 4);
        let axis = hashes
            .exceedances()
            .axis("typed store hashed candidate bytes per graph")
            .copied()
            .unwrap();
        assert_eq!(axis.target, crate::node::MAX_HASHED_BYTES_PER_GRAPH);
        assert_eq!(
            axis.first_requested,
            crate::node::MAX_HASHED_BYTES_PER_GRAPH + 2
        );
        assert_eq!(axis.high_water, crate::node::MAX_HASHED_BYTES_PER_GRAPH + 2);
        fs::remove_dir_all(root).expect("the artifact budget fixture retires");
    }
}
