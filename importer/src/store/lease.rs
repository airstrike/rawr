#[cfg(feature = "import-baseline")]
use crate::metrics::principal::{creation, retirement};

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
    #[cfg(feature = "import-baseline")]
    graph: &'store crate::node::GraphBudget,
    #[cfg(feature = "import-baseline")]
    observation: u64,
}

/// Explicit project bootstrap, before evaluating even an entirely shared graph.
/// Existing controls are validated without admission or temporary recovery.
pub(super) fn enroll_project(
    root: &Path,
    graph: &crate::node::GraphBudget,
    project: Digest,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    with_publication(root, graph, |publication| {
        let name = format!("{}.lock", project.directory());
        let owner = root.join("locks/owners").join(&name);
        let project_lock = root.join("locks/projects").join(&name);
        let pending = root
            .join("locks/projects")
            .join(format!("{}.pending", project.directory()));
        let exists = |path: &Path| -> Result<bool, Error> {
            match fs::symlink_metadata(path) {
                Ok(metadata) => {
                    #[cfg(feature = "import-baseline")]
                    let observation =
                        creation::Context::published(publication, creation::Source::Migration)
                            .start(path, false);
                    validate_file_with(
                        path,
                        #[cfg(feature = "import-baseline")]
                        Some(&observation),
                    )?;
                    if metadata.len() != 0 {
                        return Err(Error::Corrupt {
                            path: path.to_path_buf(),
                            reason: "project enrollment control bytes changed",
                        });
                    }
                    #[cfg(feature = "import-baseline")]
                    observation.completed(true);
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
        ensure_migration_file(publication, root, &pending, budget)?;
        ensure_migration_file(publication, root, &owner, budget)?;
        ensure_migration_file(publication, root, &project_lock, budget)?;
        super::entry::sync_directory(&root.join("locks/owners"))?;
        super::entry::sync_directory(&root.join("locks/projects"))?;
        let admitted = super::accounting::admit_control_recovery(
            publication,
            root,
            super::control::Plan::retirement(&pending),
            budget,
        )?;
        budget.deletion(super::control::file_charge(0)?)?;
        #[cfg(feature = "import-baseline")]
        let mut observation = crate::metrics::publication::Attempt::start(
            crate::metrics::publication::Class::Retirement(
                crate::metrics::publication::Retirement::Enrollment,
            ),
        );
        #[cfg(feature = "import-baseline")]
        let retirement = retirement::Attempt::start(
            publication,
            &pending,
            retirement::Cause::Enrollment,
            retirement::Facts::default(),
        );
        let temporary = super::entry::stage_admitted_retirement(
            admitted,
            publication,
            root,
            &pending,
            #[cfg(feature = "import-baseline")]
            &mut observation,
            #[cfg(feature = "import-baseline")]
            Some(super::entry::Retirement::Allocation(&retirement)),
        )?;
        #[cfg(feature = "import-baseline")]
        observation.cleanup_started();
        #[cfg(feature = "import-baseline")]
        retirement.removal_started();
        let cleanup = fs::remove_file(&temporary).map_err(|source| Error::Io {
            operation: "finish project enrollment",
            path: temporary,
            source,
        });
        #[cfg(feature = "import-baseline")]
        observation.cleanup(cleanup.is_ok());
        #[cfg(feature = "import-baseline")]
        retirement.removal_finished(cleanup.is_ok());
        cleanup?;
        let result = super::entry::sync_directory(&root.join("temporaries"));
        #[cfg(feature = "import-baseline")]
        retirement.completed(result.is_ok());
        #[cfg(feature = "import-baseline")]
        let result = observation.finish(result);
        result
    })
}

impl<'store> Publication<'store> {
    pub(crate) const fn root(&self) -> &Path {
        self.root
    }

    #[cfg(feature = "import-baseline")]
    pub(crate) const fn graph(&self) -> &'store crate::node::GraphBudget {
        self.graph
    }

    #[cfg(feature = "import-baseline")]
    pub(crate) const fn observation(&self) -> u64 {
        self.observation
    }
}

pub(crate) fn with_bootstrap<T>(
    root: &Path,
    fresh: bool,
    #[cfg(feature = "import-baseline")] graph: &crate::node::GraphBudget,
    evaluate: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let path = root.join("bootstrap.lock");
    {
        #[cfg(feature = "import-baseline")]
        let observation = creation::Context::unleased(root, graph, creation::Source::Bootstrap)
            .start(&path, false);
        let result = match fs::symlink_metadata(&path) {
            Ok(_) => validate_file_with(
                &path,
                #[cfg(feature = "import-baseline")]
                Some(&observation),
            ),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound && fresh => {
                create_bootstrap(
                    root,
                    &path,
                    #[cfg(feature = "import-baseline")]
                    Some(&observation),
                )
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
        };
        #[cfg(feature = "import-baseline")]
        observation.completed(result.is_ok());
        result?;
    }
    with_file(&path, "acquire typed store bootstrap lease", evaluate)
}

fn create_bootstrap(
    root: &Path,
    path: &Path,
    #[cfg(feature = "import-baseline")] observation: Option<&creation::Attempt<'_>>,
) -> Result<(), Error> {
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => {
            #[cfg(feature = "import-baseline")]
            if let Some(observation) = observation {
                observation.created(creation::Outcome::Created);
            }
            let result = super::entry::sync_directory(root);
            #[cfg(feature = "import-baseline")]
            if let Some(observation) = observation {
                observation.synced(creation::Sync::Parent, result.is_ok());
            }
            result
        }
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            #[cfg(feature = "import-baseline")]
            if let Some(observation) = observation {
                observation.created(creation::Outcome::Contended);
            }
            validate_file_with(
                path,
                #[cfg(feature = "import-baseline")]
                observation,
            )
        }
        Err(source) => {
            #[cfg(feature = "import-baseline")]
            if let Some(observation) = observation {
                observation.created(creation::Outcome::Failed);
            }
            Err(Error::Io {
                operation: "create typed store bootstrap lease",
                path: path.to_path_buf(),
                source,
            })
        }
    }
}

pub(crate) fn initialize(
    root: &Path,
    fresh: bool,
    #[cfg(feature = "import-baseline")] graph: &crate::node::GraphBudget,
) -> Result<(), Error> {
    let locks = super::entry::existing_directory(root, Path::new("locks"))?.ok_or_else(|| {
        Error::Corrupt {
            path: root.join("locks"),
            reason: "typed store lease directory is missing",
        }
    })?;
    for name in ["heavy.lock", "publication.lock"] {
        let path = locks.join(name);
        #[cfg(feature = "import-baseline")]
        let observation =
            creation::Context::unleased(root, graph, creation::Source::Fixed).start(&path, false);
        if fresh {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => {
                    #[cfg(feature = "import-baseline")]
                    observation.created(creation::Outcome::Created);
                    let result = super::entry::sync_directory(&locks);
                    #[cfg(feature = "import-baseline")]
                    observation.synced(creation::Sync::Parent, result.is_ok());
                    result?;
                }
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                    #[cfg(feature = "import-baseline")]
                    observation.created(creation::Outcome::Contended);
                    validate_file_with(
                        &path,
                        #[cfg(feature = "import-baseline")]
                        Some(&observation),
                    )?;
                }
                Err(source) => {
                    #[cfg(feature = "import-baseline")]
                    observation.created(creation::Outcome::Failed);
                    return Err(Error::Io {
                        operation: "initialize typed store fixed lease",
                        path,
                        source,
                    });
                }
            }
        } else {
            validate_file_with(
                &path,
                #[cfg(feature = "import-baseline")]
                Some(&observation),
            )?;
        }
        #[cfg(feature = "import-baseline")]
        observation.completed(true);
    }
    Ok(())
}

pub(crate) fn with_action<T>(
    root: &Path,
    graph: &crate::node::GraphBudget,
    action: ActionKey,
    evaluate: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let directory = super::entry::existing_directory(root, Path::new("locks/actions"))?
        .ok_or_else(|| Error::Corrupt {
            path: root.join("locks/actions"),
            reason: "action lease directory is missing",
        })?;
    let path = directory.join(format!("{}.lock", action.directory()));
    ensure_keyed(root, graph, &path)?;
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
    graph: &crate::node::GraphBudget,
    execution: ExecutionKey,
    evaluate: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let executions = super::entry::existing_directory(root, Path::new("locks/executions"))?
        .ok_or_else(|| Error::Corrupt {
            path: root.join("locks/executions"),
            reason: "execution lease directory is missing",
        })?;
    let exact = executions.join(format!("{}.lock", execution.directory()));
    ensure_keyed(root, graph, &exact)?;
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
    #[cfg_attr(not(feature = "import-baseline"), allow(unused_variables))]
    graph: &crate::node::GraphBudget,
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
        || {
            evaluate(&Publication {
                root,
                #[cfg(feature = "import-baseline")]
                graph,
                #[cfg(feature = "import-baseline")]
                observation: if crate::metrics::principal_enabled() {
                    crate::metrics::principal::next()
                } else {
                    0
                },
            })
        },
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

/// Wait only after dropping publication and every migration probe guard.
pub(super) fn wait_migration(path: &Path) -> Result<(), Error> {
    let before = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect legacy migration waiter",
                path: path.to_path_buf(),
                source,
            })
        }
    };
    if before.file_type().is_symlink() || !before.is_file() || hard_linked(&before) {
        return Err(Error::Shape(path.to_path_buf()));
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(Error::Io {
                operation: "open legacy migration waiter",
                path: path.to_path_buf(),
                source,
            })
        }
    };
    let opened = file.metadata().map_err(|source| Error::Io {
        operation: "inspect opened legacy migration waiter",
        path: path.to_path_buf(),
        source,
    })?;
    if hard_linked(&opened) || !same_file(&before, &opened) {
        return Err(Error::Shape(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive).map_err(
            |source| Error::Io {
                operation: "wait for legacy store operation",
                path: path.to_path_buf(),
                source: std::io::Error::from_raw_os_error(source.raw_os_error()),
            },
        )?;
    }
    #[cfg(not(unix))]
    return Err(Error::State(
        "ownership migration requires Unix advisory locks",
    ));
    // No authority is derived from the waiter: bootstrap rescans under publication.
    Ok(())
}

pub(super) fn ensure_migration_file(
    publication: &Publication<'_>,
    root: &Path,
    path: &Path,
    budget: &mut super::recovery::Budget,
) -> Result<(), Error> {
    #[cfg(feature = "import-baseline")]
    let observation =
        creation::Context::published(publication, creation::Source::Migration).start(path, false);
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let result = validate_file_with(
                path,
                #[cfg(feature = "import-baseline")]
                Some(&observation),
            );
            #[cfg(feature = "import-baseline")]
            observation.completed(result.is_ok());
            return result;
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect migration control",
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
        .inspect(|_| {
            #[cfg(feature = "import-baseline")]
            observation.created(creation::Outcome::Created);
        })
        .inspect_err(|_source| {
            #[cfg(feature = "import-baseline")]
            observation.created(if _source.kind() == std::io::ErrorKind::AlreadyExists {
                creation::Outcome::Contended
            } else {
                creation::Outcome::Failed
            });
        })
        .and_then(|file| {
            let result = file.sync_all();
            #[cfg(feature = "import-baseline")]
            observation.synced(creation::Sync::File, result.is_ok());
            result
        })
        .map_err(|source| Error::Io {
            operation: "create migration control",
            path: path.to_path_buf(),
            source,
        })?;
    let result = super::entry::sync_directory(
        path.parent()
            .ok_or(Error::State("migration control has no parent"))?,
    );
    #[cfg(feature = "import-baseline")]
    {
        observation.synced(creation::Sync::Parent, result.is_ok());
        observation.completed(result.is_ok());
    }
    result
}

fn ensure_keyed(root: &Path, graph: &crate::node::GraphBudget, path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            #[cfg(feature = "import-baseline")]
            let observation = creation::Context::unleased(root, graph, creation::Source::Keyed)
                .start(path, false);
            let result = validate_file_with(
                path,
                #[cfg(feature = "import-baseline")]
                Some(&observation),
            );
            #[cfg(feature = "import-baseline")]
            observation.completed(result.is_ok());
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
    with_publication(root, graph, |publication| {
        super::recovery::recover_temporaries(
            publication,
            root,
            &mut super::recovery::Budget::new(),
        )?;
        #[cfg(feature = "import-baseline")]
        let observation =
            creation::Context::published(publication, creation::Source::Keyed).start(path, false);
        match fs::symlink_metadata(path) {
            Ok(_) => {
                let result = validate_file_with(
                    path,
                    #[cfg(feature = "import-baseline")]
                    Some(&observation),
                );
                #[cfg(feature = "import-baseline")]
                observation.completed(result.is_ok());
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
            .inspect(|_| {
                #[cfg(feature = "import-baseline")]
                observation.created(creation::Outcome::Created);
            })
            .inspect_err(|_source| {
                #[cfg(feature = "import-baseline")]
                observation.created(if _source.kind() == std::io::ErrorKind::AlreadyExists {
                    creation::Outcome::Contended
                } else {
                    creation::Outcome::Failed
                });
            })
            .map_err(|source| Error::Io {
                operation: "create typed store keyed lease",
                path: path.to_path_buf(),
                source,
            })?;
        let result = super::entry::sync_directory(
            path.parent()
                .ok_or(Error::State("typed store keyed lease has no parent"))?,
        );
        #[cfg(feature = "import-baseline")]
        {
            observation.synced(creation::Sync::Parent, result.is_ok());
            observation.completed(result.is_ok());
        }
        result
    })
}

fn validate_file(path: &Path) -> Result<(), Error> {
    validate_file_with(
        path,
        #[cfg(feature = "import-baseline")]
        None,
    )
}

fn validate_file_with(
    path: &Path,
    #[cfg(feature = "import-baseline")] observation: Option<&creation::Attempt<'_>>,
) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect typed store fixed lease",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(path.to_path_buf()));
    }
    #[cfg(feature = "import-baseline")]
    if let Some(observation) = observation {
        observation.adopted(&metadata);
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
