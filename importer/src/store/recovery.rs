//! One bounded miss-side recovery pass.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use sha2::{Digest as _, Sha256};

use super::accounting::Charge;
use crate::node::Error;

const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DELETED_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_DELETED_MEMBERS: u64 = 262_144;
const MAX_BUNDLES: usize = 8;
const MAX_TEMPORARIES: usize = 64;
const BLOCK_BYTES: u64 = 4 * 1024;

// Publication replaces the ledger atomically. Identity and change timestamps
// distinguish a new publication (or an in-place mutation) from the cached file.
pub(super) fn same_installation(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        left.is_file()
            && right.is_file()
            && left.nlink() == 1
            && right.nlink() == 1
            && left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.len() == right.len()
            && left.mtime() == right.mtime()
            && left.mtime_nsec() == right.mtime_nsec()
            && left.ctime() == right.ctime()
            && left.ctime_nsec() == right.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        let _ = (left, right);
        false
    }
}

pub(super) struct Budget {
    metadata_bytes: u64,
    deleted: Charge,
    bundles: usize,
    temporaries: usize,
    installation: Option<(
        std::path::PathBuf,
        fs::Metadata,
        Arc<super::accounting::Ledger>,
    )>,
}

#[cfg(feature = "import-baseline")]
impl Drop for Budget {
    fn drop(&mut self) {
        use crate::metrics::accounting::Scalar;
        // Observe this pass once, including the cumulative request that refused.
        // Cleanup errors keep their original precedence and no work is repeated.
        for (key, value) in [
            (Scalar::RecoveryMetadataBytes, self.metadata_bytes),
            (Scalar::RecoveryDeletionBytes, self.deleted.bytes),
            (Scalar::RecoveryDeletionMembers, self.deleted.members),
            (Scalar::RecoveryBundles, self.bundles as u64),
            (Scalar::RecoveryTemporaries, self.temporaries as u64),
        ] {
            crate::metrics::record_scalar(key, value);
        }
    }
}

impl Budget {
    pub(super) const fn new() -> Self {
        Self {
            metadata_bytes: 0,
            deleted: Charge::zero(),
            bundles: 0,
            temporaries: 0,
            installation: None,
        }
    }

    pub(super) fn installation(
        &self,
        root: &Path,
        current: &fs::Metadata,
    ) -> Option<Arc<super::accounting::Ledger>> {
        self.installation
            .as_ref()
            .filter(|(retained, metadata, _)| {
                retained == root && same_installation(metadata, current)
            })
            .map(|(_, _, ledger)| Arc::clone(ledger))
    }

    pub(super) fn retain_installation(
        &mut self,
        root: &Path,
        ledger: Arc<super::accounting::Ledger>,
        metadata: fs::Metadata,
    ) {
        self.installation = Some((root.to_path_buf(), metadata, ledger));
    }

    #[cfg(test)]
    pub(super) const fn metadata_bytes(&self) -> u64 {
        self.metadata_bytes
    }

    pub(super) fn metadata(&mut self, bytes: usize) -> Result<(), Error> {
        let bytes = u64::try_from(bytes).map_err(|_| Error::Limit {
            resource: "typed store recovery metadata bytes",
            limit: MAX_METADATA_BYTES as usize,
            actual: usize::MAX,
        })?;
        self.metadata_bytes = self
            .metadata_bytes
            .checked_add(bytes)
            .ok_or(Error::Allocation)?;
        enforce(
            "typed store recovery metadata bytes",
            self.metadata_bytes,
            MAX_METADATA_BYTES,
        )
    }

    pub(super) fn remaining_deleted_members(&self) -> u64 {
        MAX_DELETED_MEMBERS.saturating_sub(self.deleted.members)
    }

    /// Charge work this pass already chose. Freed bytes never refuse it;
    /// they only make `fits` defer further optional work to a later pass.
    pub(super) fn deletion(&mut self, charge: Charge) -> Result<(), Error> {
        self.deleted = self.deleted.checked_add(charge)?;
        enforce(
            "typed store recovery deleted filesystem members",
            self.deleted.members,
            MAX_DELETED_MEMBERS,
        )
    }

    /// Count one bundle this pass already chose; like freed bytes, the count
    /// never refuses it and only makes `bundle_fits` defer more work.
    pub(super) fn bundle(&mut self) -> Result<(), Error> {
        self.bundles = self.bundles.checked_add(1).ok_or(Error::Allocation)?;
        Ok(())
    }

    fn temporary(&mut self) -> Result<(), Error> {
        self.temporaries = self.temporaries.checked_add(1).ok_or(Error::Allocation)?;
        Ok(())
    }

    pub(super) const fn bundle_fits(&self) -> bool {
        self.bundles < MAX_BUNDLES
    }

    pub(super) fn fits(&self, charge: Charge) -> bool {
        self.deleted.checked_add(charge).is_ok_and(|total| {
            total.bytes <= MAX_DELETED_BYTES && total.members <= MAX_DELETED_MEMBERS
        })
    }

    /// Measure one dead tree for removal in this pass, or `None` when the
    /// pass's deletion bound is spent: the tree then waits for a later pass.
    pub(super) fn dead_tree(&self, root: &Path) -> Result<Option<Charge>, Error> {
        match tree_charge(root, self.remaining_deleted_members()) {
            Ok(charge) if self.fits(charge) => Ok(Some(charge)),
            Ok(_) | Err(Error::Limit { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// Remove every pre-existing publication temporary while the caller owns the
/// global publication lease. No live publisher can coexist with that lease.
pub(super) fn recover_temporaries(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    budget: &mut Budget,
) -> Result<bool, Error> {
    #[cfg(feature = "import-baseline")]
    let batch = crate::metrics::principal::recovery::Batch::start(publication, root);
    #[cfg(not(feature = "import-baseline"))]
    let _ = publication;
    let temporary_root = root.join("temporaries");
    let directory = fs::read_dir(&temporary_root).map_err(|source| Error::Io {
        operation: "read typed store publication temporaries",
        path: temporary_root.clone(),
        source,
    })?;
    let mut temporaries = Vec::new();
    temporaries
        .try_reserve_exact(MAX_TEMPORARIES)
        .map_err(|_| Error::Allocation)?;
    for child in directory {
        temporaries.try_reserve(1).map_err(|_| Error::Allocation)?;
        let child = child.map_err(|source| Error::Io {
            operation: "read typed store publication temporary",
            path: temporary_root.clone(),
            source,
        })?;
        if !valid_temporary_name(&child.file_name()) {
            return Err(Error::CleanupRequired(
                "typed store publication temporary name changed",
            ));
        }
        let metadata = child.file_type().map_err(|source| Error::Io {
            operation: "inspect typed store publication temporary",
            path: child.path(),
            source,
        })?;
        if metadata.is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
            return Err(Error::Shape(child.path()));
        }
        temporaries.push((child.path(), metadata.is_dir()));
    }
    temporaries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let mut measured = Vec::new();
    measured
        .try_reserve_exact(temporaries.len())
        .map_err(|_| Error::Allocation)?;
    for (temporary, directory) in temporaries {
        let Some(charge) = budget.dead_tree(&temporary)? else {
            break;
        };
        budget.temporary()?;
        budget.deletion(charge)?;
        #[cfg(feature = "import-baseline")]
        batch.measured(
            measured.len(),
            &temporary,
            directory,
            charge.bytes,
            charge.members,
        );
        measured.push((temporary, directory, charge.members));
    }
    let recovered = !measured.is_empty();
    #[cfg(feature = "import-baseline")]
    batch.ready(measured.len());
    for (index, (temporary, directory, members)) in measured.into_iter().enumerate() {
        #[cfg(not(feature = "import-baseline"))]
        let _ = index;
        #[cfg(feature = "import-baseline")]
        let principal = batch.temporary(index);
        #[cfg(feature = "import-baseline")]
        let mut observation = crate::metrics::publication::Attempt::start(if directory {
            crate::metrics::publication::Class::RecoveredDirectory
        } else {
            crate::metrics::publication::Class::RecoveredFile
        });
        #[cfg(feature = "import-baseline")]
        observation.adopted();
        let removed = if directory {
            #[cfg(feature = "import-baseline")]
            principal.thaw();
            let thawed = thaw_tree(
                &temporary,
                members,
                #[cfg(feature = "import-baseline")]
                &principal,
            );
            #[cfg(feature = "import-baseline")]
            principal.thawed(thawed.is_ok());
            thawed?;
            #[cfg(feature = "import-baseline")]
            observation.cleanup_started();
            #[cfg(feature = "import-baseline")]
            principal.removal_started();
            fs::remove_dir_all(&temporary)
        } else {
            #[cfg(feature = "import-baseline")]
            observation.cleanup_started();
            #[cfg(feature = "import-baseline")]
            principal.removal_started();
            fs::remove_file(&temporary)
        };
        #[cfg(feature = "import-baseline")]
        observation.cleanup(removed.is_ok());
        #[cfg(feature = "import-baseline")]
        principal.removed(removed.is_ok());
        removed.map_err(|source| Error::Io {
            operation: "remove dead typed store publication temporary",
            path: temporary,
            source,
        })?;
        // This lifetime ends at terminal removal. The one batch parent sync
        // below remains outside each temporary's conservation evidence; its
        // failure still fails the owner, without rewriting successful removals.
        #[cfg(feature = "import-baseline")]
        observation.finish(Ok::<(), Error>(()))?;
        #[cfg(feature = "import-baseline")]
        principal.completed();
    }
    if recovered {
        let result = super::entry::sync_directory(&temporary_root);
        #[cfg(feature = "import-baseline")]
        batch.synced(result.is_ok());
        result?;
    }
    #[cfg(feature = "import-baseline")]
    batch.completed(recovered);
    Ok(recovered)
}

pub(crate) fn valid_temporary_name(name: &std::ffi::OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    let Some(separator) = bytes.iter().position(|byte| *byte == b'-') else {
        return false;
    };
    separator > 0
        && separator + 1 < bytes.len()
        && bytes.iter().filter(|byte| **byte == b'-').count() == 1
        && bytes[..separator].iter().all(u8::is_ascii_digit)
        && bytes[separator + 1..].iter().all(u8::is_ascii_digit)
}

pub(super) fn tree_charge(root: &Path, member_limit: u64) -> Result<Charge, Error> {
    tree_measure(root, member_limit, None)
}

/// The retirement measurement and its recheck: the charge plus a digest of
/// the tree's logical geometry, independent of directory enumeration order.
pub(super) fn tree_geometry(root: &Path, member_limit: u64) -> Result<(Charge, [u8; 32]), Error> {
    let mut members = Vec::new();
    let charge = tree_measure(root, member_limit, Some(&mut members))?;
    members.sort_unstable();
    let mut digest = Sha256::new();
    digest.update(b"RAWRBNDL-RETIREMENT-GEOMETRY-v1\0");
    digest.update((members.len() as u64).to_le_bytes());
    for member in &members {
        digest.update(member);
    }
    Ok((charge, digest.finalize().into()))
}

#[cfg(unix)]
fn geometry_entry(root: &Path, path: &Path, metadata: &fs::Metadata) -> Result<[u8; 32], Error> {
    use std::os::unix::fs::MetadataExt as _;

    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::OutsideRoot(path.to_path_buf()))?
        .as_os_str()
        .as_encoded_bytes();
    let directory = metadata.is_dir();
    let mut digest = Sha256::new();
    digest.update((relative.len() as u64).to_le_bytes());
    digest.update(relative);
    digest.update([u8::from(directory)]);
    // Retirement thaws only the root itself; every descendant mode counts.
    let mode = if relative.is_empty() {
        0
    } else {
        metadata.mode() & 0o7777
    };
    digest.update(mode.to_le_bytes());
    digest.update(if directory { 0 } else { metadata.nlink() }.to_le_bytes());
    digest.update(if directory { 0 } else { metadata.len() }.to_le_bytes());
    Ok(digest.finalize().into())
}

#[cfg(not(unix))]
fn geometry_entry(root: &Path, path: &Path, metadata: &fs::Metadata) -> Result<[u8; 32], Error> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::OutsideRoot(path.to_path_buf()))?
        .as_os_str()
        .as_encoded_bytes();
    let directory = metadata.is_dir();
    let mut digest = Sha256::new();
    digest.update((relative.len() as u64).to_le_bytes());
    digest.update(relative);
    digest.update([u8::from(directory)]);
    digest.update(if directory { 0 } else { metadata.len() }.to_le_bytes());
    Ok(digest.finalize().into())
}

fn tree_measure(
    root: &Path,
    member_limit: u64,
    mut geometry: Option<&mut Vec<[u8; 32]>>,
) -> Result<Charge, Error> {
    if member_limit == 0 {
        return Err(Error::Limit {
            resource: "typed store recovery deleted filesystem members",
            limit: 0,
            actual: 1,
        });
    }
    let mut pending = Vec::new();
    pending.try_reserve(1).map_err(|_| Error::Allocation)?;
    pending.push(root.to_path_buf());
    let mut discovered = 1_u64;
    let mut charge = Charge::zero();
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
            operation: "inspect dead typed store publication temporary",
            path: path.clone(),
            source,
        })?;
        let bytes = if metadata.file_type().is_symlink() {
            return Err(Error::Shape(path));
        } else if metadata.is_dir() {
            for child in fs::read_dir(&path).map_err(|source| Error::Io {
                operation: "read dead typed store publication temporary",
                path: path.clone(),
                source,
            })? {
                discovered = discovered.checked_add(1).ok_or(Error::Allocation)?;
                if discovered > member_limit {
                    return Err(Error::Limit {
                        resource: "typed store recovery deleted filesystem members",
                        limit: usize::try_from(member_limit).unwrap_or(usize::MAX),
                        actual: usize::try_from(discovered).unwrap_or(usize::MAX),
                    });
                }
                pending.try_reserve(1).map_err(|_| Error::Allocation)?;
                pending.push(
                    child
                        .map_err(|source| Error::Io {
                            operation: "read dead typed store publication temporary member",
                            path: path.clone(),
                            source,
                        })?
                        .path(),
                );
            }
            BLOCK_BYTES
        } else if metadata.is_file() && (geometry.is_none() || !hard_linked(&metadata)) {
            // Removing a dead tree's extra link only drops that name.
            round_block(metadata.len())?.max(round_block(allocated_bytes(&metadata)?)?)
        } else {
            return Err(Error::Shape(path));
        };
        if let Some(geometry) = geometry.as_mut() {
            geometry.try_reserve(1).map_err(|_| Error::Allocation)?;
            geometry.push(geometry_entry(root, &path, &metadata)?);
        }
        charge = charge.checked_add(Charge {
            bytes,
            allocations: 0,
            members: 1,
        })?;
    }
    Ok(charge)
}

fn thaw_tree(
    root: &Path,
    expected_members: u64,
    #[cfg(feature = "import-baseline")] observation: &crate::metrics::principal::recovery::Attempt<
        '_,
    >,
) -> Result<(), Error> {
    if expected_members == 0 {
        return Err(Error::State(
            "typed store recovery cannot thaw an empty filesystem tree",
        ));
    }
    let mut pending = Vec::new();
    pending.try_reserve(1).map_err(|_| Error::Allocation)?;
    pending.push(root.to_path_buf());
    let mut discovered = 1_u64;
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
            operation: "inspect frozen typed store publication temporary",
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(Error::Shape(path));
        }
        if metadata.is_dir() {
            set_writable(
                &path,
                &metadata,
                #[cfg(feature = "import-baseline")]
                observation,
            )?;
            for child in fs::read_dir(&path).map_err(|source| Error::Io {
                operation: "read frozen typed store publication temporary",
                path: path.clone(),
                source,
            })? {
                discovered = discovered.checked_add(1).ok_or(Error::Allocation)?;
                if discovered > expected_members {
                    return Err(Error::Shape(root.to_path_buf()));
                }
                pending.try_reserve(1).map_err(|_| Error::Allocation)?;
                pending.push(
                    child
                        .map_err(|source| Error::Io {
                            operation: "read frozen typed store publication temporary member",
                            path: path.clone(),
                            source,
                        })?
                        .path(),
                );
            }
        } else if metadata.is_file() {
            // Unlinking needs only the parent; never chmod a shared inode.
            if !hard_linked(&metadata) {
                set_writable(
                    &path,
                    &metadata,
                    #[cfg(feature = "import-baseline")]
                    observation,
                )?;
            }
        } else {
            return Err(Error::Shape(path));
        }
    }
    if discovered != expected_members {
        return Err(Error::Shape(root.to_path_buf()));
    }
    Ok(())
}

fn round_block(bytes: u64) -> Result<u64, Error> {
    bytes
        .checked_add(BLOCK_BYTES - 1)
        .map(|bytes| bytes / BLOCK_BYTES * BLOCK_BYTES)
        .ok_or(Error::Allocation)
}

#[cfg(unix)]
fn allocated_bytes(metadata: &fs::Metadata) -> Result<u64, Error> {
    use std::os::unix::fs::MetadataExt as _;

    let measured = metadata
        .blocks()
        .checked_mul(512)
        .ok_or(Error::Allocation)?;
    #[cfg(test)]
    let measured = super::bundle::geometry::seam::allocated(metadata, measured);
    Ok(measured)
}

#[cfg(not(unix))]
fn allocated_bytes(metadata: &fs::Metadata) -> Result<u64, Error> {
    Ok(metadata.len())
}

#[cfg(unix)]
fn hard_linked(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    metadata.nlink() != 1
}

#[cfg(not(unix))]
fn hard_linked(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn set_writable(
    path: &Path,
    metadata: &fs::Metadata,
    #[cfg(feature = "import-baseline")] observation: &crate::metrics::principal::recovery::Attempt<
        '_,
    >,
) -> Result<(), Error> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let mode = if metadata.is_dir() {
        metadata.mode() | 0o700
    } else {
        metadata.mode() | 0o600
    };
    #[cfg(feature = "import-baseline")]
    let change = observation.mode(path, metadata.is_dir(), metadata.mode(), mode);
    let result = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    #[cfg(feature = "import-baseline")]
    observation.mode_result(change, result.is_ok());
    result.map_err(|source| Error::Io {
        operation: "thaw typed store publication temporary",
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn set_writable(
    path: &Path,
    metadata: &fs::Metadata,
    #[cfg(feature = "import-baseline")] observation: &crate::metrics::principal::recovery::Attempt<
        '_,
    >,
) -> Result<(), Error> {
    #[cfg(feature = "import-baseline")]
    let _ = observation;
    let mut permissions = metadata.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).map_err(|source| Error::Io {
        operation: "thaw typed store publication temporary",
        path: path.to_path_buf(),
        source,
    })
}

fn enforce(resource: &'static str, actual: u64, limit: u64) -> Result<(), Error> {
    if actual > limit {
        return Err(Error::Limit {
            resource,
            limit: usize::try_from(limit).unwrap_or(usize::MAX),
            actual: usize::try_from(actual).unwrap_or(usize::MAX),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE: AtomicU64 = AtomicU64::new(1);

    #[cfg(feature = "import-baseline")]
    #[test]
    fn recovery_observation_keeps_the_refused_cumulative_request() {
        use crate::metrics::{accounting::Scalar, recorder::Recorder};
        let recorder = Recorder::transform(10).unwrap();
        {
            let _scope = recorder.scope().unwrap();
            let mut budget = Budget::new();
            budget.metadata(MAX_METADATA_BYTES as usize).unwrap();
            assert!(
                matches!(budget.metadata(1), Err(Error::Limit { actual, .. }) if actual == MAX_METADATA_BYTES as usize + 1)
            );
        }
        let evidence = recorder.snapshot().unwrap().accounting;
        assert_eq!(
            evidence.scalar(Scalar::RecoveryMetadataBytes).unwrap().max,
            MAX_METADATA_BYTES + 1
        );
        assert_eq!(evidence.scalar(Scalar::RecoveryBundles).unwrap().total, 0);
        assert_eq!(
            evidence
                .scalar(Scalar::RecoveryDeletionBytes)
                .unwrap()
                .count,
            1
        );
        assert!(evidence.scalar(Scalar::BundleMeasuredBytes).is_none());
    }

    #[test]
    fn recovery_limits_admit_exact_caps_reject_unit_floors_and_defer_freed_bytes() {
        let mut metadata = Budget::new();
        metadata
            .metadata(MAX_METADATA_BYTES as usize)
            .expect("the exact metadata cap admits");
        assert!(matches!(
            metadata.metadata(1),
            Err(Error::Limit {
                resource: "typed store recovery metadata bytes",
                ..
            })
        ));

        let mut deletion = Budget::new();
        deletion
            .deletion(Charge {
                bytes: MAX_DELETED_BYTES,
                allocations: 0,
                members: MAX_DELETED_MEMBERS,
            })
            .expect("the exact deletion caps admit");
        let over = Charge {
            bytes: 1,
            allocations: 0,
            members: 0,
        };
        // Chosen deletion past the byte target proceeds; only further
        // optional work is deferred to a later pass.
        assert!(!deletion.fits(over));
        deletion
            .deletion(over)
            .expect("freeing more bytes is never refused");

        let mut members = Budget::new();
        assert!(matches!(
            members.deletion(Charge {
                bytes: 0,
                allocations: 0,
                members: MAX_DELETED_MEMBERS + 1,
            }),
            Err(Error::Limit {
                resource: "typed store recovery deleted filesystem members",
                ..
            })
        ));

        let mut counts = Budget::new();
        for _ in 0..MAX_BUNDLES {
            assert!(counts.bundle_fits());
            counts.bundle().expect("the exact bundle cap admits");
        }
        assert!(!counts.bundle_fits());
        counts
            .bundle()
            .expect("a bundle already chosen is never refused");
    }

    #[test]
    fn temporary_recovery_removes_every_dead_temporary_in_one_pass() {
        let root = fixture("temporary-cap");
        for index in 0..=MAX_TEMPORARIES {
            let temporary = root.join("temporaries").join(format!("12-{index}"));
            fs::create_dir(&temporary).expect("the dead temporary exists");
            fs::write(temporary.join("payload"), b"dead")
                .expect("the dead temporary payload exists");
        }
        let linked = root.join("linked");
        fs::write(&linked, b"shared").expect("the hard-link source exists");
        fs::hard_link(&linked, root.join("temporaries/12-0/linked"))
            .expect("the dead temporary holds a hard link");
        assert!(recover_temporaries(&root, &mut Budget::new())
            .expect("an interrupted run's temporaries never block recovery"));
        assert_eq!(
            fs::read_dir(root.join("temporaries"))
                .expect("the recovered directory reads")
                .count(),
            0
        );
        assert_eq!(
            fs::read(&linked).unwrap(),
            b"shared",
            "only the dead name goes"
        );
        fs::remove_dir_all(root).expect("the temporary-cap fixture retires");
    }

    #[test]
    fn replacement_files_share_the_bounded_recoverable_temporary_grammar() {
        let root = fixture("replacement-file");
        let temporary = root.join("temporaries/78-9");
        fs::write(&temporary, b"partial ledger").expect("the replacement file temporary exists");
        assert!(recover_temporaries(&root, &mut Budget::new())
            .expect("the replacement file temporary recovers"));
        assert!(!temporary.exists());

        let tree = root.join("temporaries/78-10");
        fs::create_dir(&tree).expect("the bounded tree exists");
        fs::write(tree.join("first"), b"one").expect("the first tree member exists");
        fs::write(tree.join("second"), b"two").expect("the second tree member exists");
        assert!(matches!(
            tree_charge(&tree, 2),
            Err(Error::Limit {
                resource: "typed store recovery deleted filesystem members",
                limit: 2,
                actual: 3,
            })
        ));
        assert!(tree.exists(), "bounded discovery rejects before deletion");
        fs::remove_dir_all(root).expect("the replacement-file fixture retires");
    }

    #[test]
    fn recovery_thaws_one_frozen_staged_tree_before_recursive_retirement() {
        let root = fixture("frozen-tree");
        let temporary = root.join("temporaries/91-4");
        let nested = temporary.join("bundle/bin");
        fs::create_dir_all(&nested).expect("the staged frozen tree exists");
        fs::write(nested.join("runner"), b"immutable").expect("the staged frozen payload exists");
        freeze_fixture_tree(&temporary);

        assert!(recover_temporaries(&root, &mut Budget::new())
            .expect("the staged frozen tree recovers"));
        assert!(!temporary.exists());
        fs::remove_dir_all(root).expect("the frozen-tree fixture retires");
    }

    fn freeze_fixture_tree(path: &Path) {
        let metadata = fs::symlink_metadata(path).expect("the fixture member inspects");
        if metadata.is_dir() {
            for child in fs::read_dir(path).expect("the fixture directory reads") {
                freeze_fixture_tree(&child.expect("the fixture child reads").path());
            }
        }
        freeze_fixture_member(path, &metadata);
    }

    #[cfg(unix)]
    fn freeze_fixture_member(path: &Path, metadata: &fs::Metadata) {
        use std::os::unix::fs::PermissionsExt as _;

        let mode = if metadata.is_dir() { 0o755 } else { 0o444 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .expect("the fixture member freezes");
    }

    #[cfg(not(unix))]
    fn freeze_fixture_member(path: &Path, metadata: &fs::Metadata) {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions).expect("the fixture member freezes");
    }

    fn recover_temporaries(root: &Path, budget: &mut Budget) -> Result<bool, Error> {
        let graph = crate::node::GraphBudget::new();
        super::super::lease::with_publication(root, &graph, |publication| {
            super::recover_temporaries(publication, root, budget)
        })
    }

    fn fixture(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rawr-store-recovery-{label}-{}-{}",
            std::process::id(),
            FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("the recovery fixture root exists");
        let store =
            crate::store::Store::open(&root, [77; 32]).expect("the recovery fixture store opens");
        store.root().to_path_buf()
    }
}
