use std::fs::{self, File, OpenOptions};
use std::path::Path;

use crate::node::{ActionKey, Digest, Error, ExecutionKey};

#[cfg(test)]
mod tests;

pub(crate) struct ExecutionGuard {
    _file: File,
}

pub(crate) struct RecoveryGuard {
    _file: File,
}

/// Borrowed proof of the outer per-project InFlight ownership lease.
/// Its constructor is private and its lifetime never outlives `with_ownership`.
pub(crate) struct Ownership<'root> {
    root: &'root Path,
    project: Digest,
}

impl Ownership<'_> {
    pub(crate) fn require(&self, root: &Path, project: Digest) -> Result<(), Error> {
        if self.root != root || self.project != project {
            return Err(Error::State(
                "InFlight ownership belongs to another store or project",
            ));
        }
        Ok(())
    }
}

pub(crate) fn with_ownership<T>(
    root: &Path,
    project: Digest,
    evaluate: impl FnOnce(&Ownership<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    let directory = super::entry::existing_directory(root, Path::new("locks/owners"))?.ok_or(
        Error::CleanupRequired("project ownership lease directory is missing"),
    )?;
    let path = directory.join(format!("{}.lock", project.directory()));
    // Store bootstrap enrolls every project before any graph operation. A miss
    // must not recreate a deleted control and split a still-held OS lease.
    validate_file(&path)?;
    with_file(&path, "acquire project InFlight ownership", || {
        evaluate(&Ownership { root, project })
    })
}

pub(crate) struct Publication<'store> {
    root: &'store Path,
}

/// Explicit project bootstrap, before evaluating even an entirely shared graph.
/// Existing controls are validated without admission or temporary recovery.
pub(super) fn enroll_project(
    root: &Path,
    project: Digest,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    with_publication(root, |publication| {
        let name = format!("{}.lock", project.directory());
        let owner = root.join("locks/owners").join(&name);
        let project_lock = root.join("locks/projects").join(&name);
        let pending = root
            .join("locks/projects")
            .join(format!("{}.pending", project.directory()));
        let exists = |path: &Path| -> Result<bool, Error> {
            match fs::symlink_metadata(path) {
                Ok(metadata) => {
                    validate_file_with(path)?;
                    if metadata.len() != 0 {
                        return Err(Error::Corrupt {
                            path: path.to_path_buf(),
                            reason: "project enrollment control bytes changed",
                        });
                    }
                    Ok(true)
                }
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(source) => Err(Error::Io {
                    operation: "inspect project enrollment",
                    path: path.to_path_buf(),
                    source,
                }),
            }
        };
        let owner_present = exists(&owner)?;
        let project_present = exists(&project_lock)?;
        let pending_present = exists(&pending)?;
        if !pending_present {
            if owner_present && project_present {
                return Ok(());
            }
            if owner_present != project_present {
                return Err(Error::CleanupRequired(if !owner_present {
                    "enrolled project ownership control is missing"
                } else {
                    "enrolled project lock control is missing"
                }));
            }
        }
        super::recovery::recover_temporaries(publication, root, budget)?;
        ensure_control_file(publication, root, &pending, budget)?;
        ensure_control_file(publication, root, &owner, budget)?;
        ensure_control_file(publication, root, &project_lock, budget)?;
        super::entry::sync_directory(&root.join("locks/owners"))?;
        super::entry::sync_directory(&root.join("locks/projects"))?;
        let admitted = super::accounting::admit_control_recovery(
            publication,
            root,
            super::control::Plan::retirement(&pending),
            budget,
        )?;
        budget.deletion(super::control::file_charge(0)?)?;
        let temporary =
            super::entry::stage_admitted_retirement(admitted, publication, root, &pending)?;
        fs::remove_file(&temporary).map_err(|source| Error::Io {
            operation: "finish project enrollment",
            path: temporary,
            source,
        })?;
        super::entry::sync_directory(&root.join("temporaries"))
    })
}

impl<'store> Publication<'store> {
    pub(crate) const fn root(&self) -> &Path {
        self.root
    }
}

pub(crate) fn with_bootstrap<T>(
    root: &Path,
    fresh: bool,
    evaluate: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let path = root.join("bootstrap.lock");
    {
        match fs::symlink_metadata(&path) {
            Ok(_) => validate_file_with(&path),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound && fresh => {
                create_bootstrap(root, &path)
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Err(
                Error::CleanupRequired("typed store bootstrap lease is missing"),
            ),
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect typed store bootstrap lease",
                    path,
                    source,
                })
            }
        }?;
    }
    with_file(&path, "acquire typed store bootstrap lease", evaluate)
}

fn create_bootstrap(root: &Path, path: &Path) -> Result<(), Error> {
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => super::entry::sync_directory(root),
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            validate_file_with(path)
        }
        Err(source) => Err(Error::Io {
            operation: "create typed store bootstrap lease",
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(crate) fn initialize(root: &Path, fresh: bool) -> Result<(), Error> {
    let locks = super::entry::existing_directory(root, Path::new("locks"))?.ok_or_else(|| {
        Error::Corrupt {
            path: root.join("locks"),
            reason: "typed store lease directory is missing",
        }
    })?;
    for name in ["heavy.lock", "publication.lock"] {
        let path = locks.join(name);
        if fresh {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => {
                    super::entry::sync_directory(&locks)?;
                }
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                    validate_file_with(&path)?;
                }
                Err(source) => {
                    return Err(Error::Io {
                        operation: "initialize typed store fixed lease",
                        path,
                        source,
                    });
                }
            }
        } else {
            validate_file_with(&path)?;
        }
    }
    Ok(())
}

pub(crate) fn with_action<T>(
    root: &Path,
    action: ActionKey,
    evaluate: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let directory = super::entry::existing_directory(root, Path::new("locks/actions"))?
        .ok_or_else(|| Error::Corrupt {
            path: root.join("locks/actions"),
            reason: "action lease directory is missing",
        })?;
    let path = directory.join(format!("{}.lock", action.directory()));
    ensure_keyed(root, &path)?;
    with_file(&path, "typed node action", evaluate)
}

pub(crate) fn with_heavy<T>(
    root: &Path,
    evaluate: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let locks = super::entry::existing_directory(root, Path::new("locks"))?.ok_or_else(|| {
        Error::Corrupt {
            path: root.join("locks"),
            reason: "typed store lease directory is missing",
        }
    })?;
    let heavy = locks.join("heavy.lock");
    with_file(&heavy, "acquire typed node heavyweight lease", evaluate)
}

pub(crate) fn with_execution<T>(
    root: &Path,
    execution: ExecutionKey,
    evaluate: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let executions = super::entry::existing_directory(root, Path::new("locks/executions"))?
        .ok_or_else(|| Error::Corrupt {
            path: root.join("locks/executions"),
            reason: "execution lease directory is missing",
        })?;
    let exact = executions.join(format!("{}.lock", execution.directory()));
    ensure_keyed(root, &exact)?;
    with_file(&exact, "acquire typed node execution lease", evaluate)
}

/// Acquire one already-created execution lease without waiting. Absence means
/// the corresponding bundle cannot have been allocated through this store.
pub(crate) fn try_execution(
    root: &Path,
    execution: ExecutionKey,
) -> Result<Option<ExecutionGuard>, Error> {
    let executions = super::entry::existing_directory(root, Path::new("locks/executions"))?
        .ok_or_else(|| Error::Corrupt {
            path: root.join("locks/executions"),
            reason: "execution lease directory is missing",
        })?;
    let path = executions.join(format!("{}.lock", execution.directory()));
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect dead execution lease",
                path,
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(path));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|source| Error::Io {
            operation: "open dead execution lease",
            path: path.clone(),
            source,
        })?;
    #[cfg(unix)]
    {
        match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(source) if source == rustix::io::Errno::WOULDBLOCK => return Ok(None),
            Err(source) => {
                return Err(Error::Io {
                    operation: "probe dead execution lease",
                    path,
                    source: std::io::Error::from_raw_os_error(source.raw_os_error()),
                });
            }
        }
        let opened = file.metadata().map_err(|source| Error::Io {
            operation: "inspect opened dead execution lease",
            path: path.clone(),
            source,
        })?;
        let current = fs::metadata(&path).map_err(|source| Error::Io {
            operation: "reinspect dead execution lease",
            path: path.clone(),
            source,
        })?;
        if hard_linked(&opened) || !same_file(&opened, &current) {
            return Err(Error::Shape(path));
        }
    }
    #[cfg(not(unix))]
    {
        return Err(Error::Io {
            operation: "probe dead execution lease",
            path,
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "execution recovery requires Unix advisory locks",
            ),
        });
    }
    Ok(Some(ExecutionGuard { _file: file }))
}

pub(crate) fn try_key(
    root: &Path,
    key: super::accounting::LeaseKey,
) -> Result<Option<RecoveryGuard>, Error> {
    let (class, key) = match key {
        super::accounting::LeaseKey::Action(action) => ("actions", action.directory()),
        super::accounting::LeaseKey::Execution(execution) => ("executions", execution.directory()),
    };
    let directory =
        super::entry::existing_directory(root, Path::new("locks").join(class).as_path())?
            .ok_or_else(|| Error::Corrupt {
                path: root.join("locks").join(class),
                reason: "typed store recovery lease directory is missing",
            })?;
    let path = directory.join(format!("{key}.lock"));
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect typed store recovery lease",
                path,
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(path));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|source| Error::Io {
            operation: "open typed store recovery lease",
            path: path.clone(),
            source,
        })?;
    #[cfg(unix)]
    {
        match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(source) if source == rustix::io::Errno::WOULDBLOCK => return Ok(None),
            Err(source) => {
                return Err(Error::Io {
                    operation: "probe typed store recovery lease",
                    path,
                    source: std::io::Error::from_raw_os_error(source.raw_os_error()),
                });
            }
        }
        let opened = file.metadata().map_err(|source| Error::Io {
            operation: "inspect opened typed store recovery lease",
            path: path.clone(),
            source,
        })?;
        let current = fs::metadata(&path).map_err(|source| Error::Io {
            operation: "reinspect typed store recovery lease",
            path: path.clone(),
            source,
        })?;
        if hard_linked(&opened) || !same_file(&opened, &current) {
            return Err(Error::Shape(path));
        }
    }
    #[cfg(not(unix))]
    {
        return Err(Error::Io {
            operation: "probe typed store recovery lease",
            path,
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "reservation recovery requires Unix advisory locks",
            ),
        });
    }
    Ok(Some(RecoveryGuard { _file: file }))
}

pub(crate) fn with_project<T>(
    root: &Path,
    project: Digest,
    evaluate: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let projects = super::entry::existing_directory(root, Path::new("locks/projects"))?
        .ok_or_else(|| Error::Corrupt {
            path: root.join("locks/projects"),
            reason: "project lease directory is missing",
        })?;
    let exact = projects.join(format!("{}.lock", project.directory()));
    validate_file(&exact)?;
    with_file(&exact, "acquire typed store project lease", evaluate)
}

pub(crate) fn with_publication<T>(
    root: &Path,
    evaluate: impl FnOnce(&Publication<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    let locks = super::entry::existing_directory(root, Path::new("locks"))?.ok_or_else(|| {
        Error::Corrupt {
            path: root.join("locks"),
            reason: "typed store lease directory is missing",
        }
    })?;
    with_file(
        &locks.join("publication.lock"),
        "acquire typed node publication lease",
        || evaluate(&Publication { root }),
    )
}

fn with_file<T>(
    path: &Path,
    label: &'static str,
    evaluate: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let admitted = match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || hard_linked(&metadata) =>
        {
            return Err(Error::Shape(path.to_path_buf()));
        }
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::CleanupRequired(
                "typed store lease was not admitted before acquisition",
            ));
        }
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect typed node lease",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|source| Error::Io {
            operation: "open typed node lease",
            path: path.to_path_buf(),
            source,
        })?;
    let mut lock = fd_lock::RwLock::new(file);
    let guard = lock.write().map_err(|source| Error::Io {
        operation: label,
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        let opened = guard.metadata().map_err(|source| Error::Io {
            operation: "inspect opened typed node lease",
            path: path.to_path_buf(),
            source,
        })?;
        let current = fs::symlink_metadata(path).map_err(|source| Error::Io {
            operation: "reinspect typed node lease",
            path: path.to_path_buf(),
            source,
        })?;
        if current.file_type().is_symlink()
            || !current.is_file()
            || hard_linked(&opened)
            || !same_file(&admitted, &opened)
            || !same_file(&opened, &current)
        {
            return Err(Error::Shape(path.to_path_buf()));
        }
    }
    #[cfg(not(unix))]
    let _ = (&admitted, &guard);
    evaluate()
}

pub(super) fn ensure_control_file(
    publication: &Publication<'_>,
    root: &Path,
    path: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let result = validate_file_with(path);
            return result;
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect store control",
                path: path.to_path_buf(),
                source,
            })
        }
    }
    let admitted = super::accounting::admit_control_recovery(
        publication,
        root,
        super::control::Plan::file(path),
        budget,
    )?;
    admitted.consume(publication, root, "file", path, 0)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .inspect(|_| {})
        .inspect_err(|_source| {})
        .and_then(|file| file.sync_all())
        .map_err(|source| Error::Io {
            operation: "create store control",
            path: path.to_path_buf(),
            source,
        })?;
    super::entry::sync_directory(
        path.parent()
            .ok_or(Error::State("store control has no parent"))?,
    )
}

fn ensure_keyed(root: &Path, path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let result = validate_file_with(path);
            return result;
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect typed store keyed lease",
                path: path.to_path_buf(),
                source,
            })
        }
    }
    with_publication(root, |publication| {
        super::recovery::recover_temporaries(
            publication,
            root,
            &mut super::recovery::Budget::new(),
        )?;
        match fs::symlink_metadata(path) {
            Ok(_) => {
                let result = validate_file_with(path);
                return result;
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(Error::Io {
                    operation: "reinspect typed store keyed lease",
                    path: path.to_path_buf(),
                    source,
                })
            }
        }
        let admitted =
            super::accounting::admit_control(publication, root, super::control::Plan::file(path))?;
        admitted.consume(publication, root, "file", path, 0)?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .inspect(|_| {})
            .inspect_err(|_source| {})
            .map_err(|source| Error::Io {
                operation: "create typed store keyed lease",
                path: path.to_path_buf(),
                source,
            })?;
        super::entry::sync_directory(
            path.parent()
                .ok_or(Error::State("typed store keyed lease has no parent"))?,
        )
    })
}

fn validate_file(path: &Path) -> Result<(), Error> {
    validate_file_with(path)
}

fn validate_file_with(path: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect typed store fixed lease",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(path.to_path_buf()));
    }
    Ok(())
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
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    left.dev() == right.dev() && left.ino() == right.ino()
}
