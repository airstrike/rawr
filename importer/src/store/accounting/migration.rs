//! Bootstrap-only legacy decoding and permanent old-writer fence.
use std::fs;
use std::path::{Path, PathBuf};

use super::{LeaseKey, Ledger};
use crate::node::Error;
use crate::store::{entry, lease, pins, recovery};

pub(in crate::store) enum Step {
    Complete,
    Wait(PathBuf),
}

/// Called with bootstrap then publication held. Every wait escapes this scope.
pub(in crate::store) fn advance(
    publication: &lease::Publication<'_>,
    root: &Path,
    budget: &mut recovery::Budget,
) -> Result<Step, Error> {
    if publication.root() != root {
        return Err(Error::State("migration publication changed store"));
    }
    let path = super::ledger_path(root);
    let metadata = super::recovery_metadata(root)?;
    let length = usize::try_from(metadata.len()).map_err(|_| Error::Allocation)?;
    if length > super::MAX_LEDGER_BYTES {
        return Err(Error::Limit {
            resource: "migration installation ledger bytes",
            limit: super::MAX_LEDGER_BYTES,
            actual: length,
        });
    }
    budget.metadata(length)?;
    let bytes = entry::read_file(&path, length)?;
    let schema = super::u16_at(&bytes, 8)?;
    // Legacy acceptance exists only here, under the bootstrap/publication pair.
    if schema != 4 && schema != super::LEDGER_SCHEMA {
        return Err(Error::Corrupt {
            path,
            reason: "migration ledger schema is unknown",
        });
    }
    let ledger = Ledger::decode_schema(&bytes, schema)?;
    if schema == 4 {
        let pins = match pins::lock::migration_snapshot(publication, root, budget)? {
            pins::lock::Migration::Quiescent(guards) => guards,
            pins::lock::Migration::Waiting(path) => return Ok(Step::Wait(path)),
        };
        let mut keys = Vec::new();
        budget.metadata(
            ledger
                .reservations
                .len()
                .checked_mul(std::mem::size_of::<lease::RecoveryGuard>())
                .ok_or(Error::Allocation)?,
        )?;
        keys.try_reserve_exact(ledger.reservations.len())
            .map_err(|_| Error::Allocation)?;
        for reservation in &ledger.reservations {
            let (class, name) = match reservation.key {
                LeaseKey::Action(key) => ("actions", key.directory()),
                LeaseKey::Execution(key) => ("executions", key.directory()),
            };
            let path = root.join("locks").join(class).join(format!("{name}.lock"));
            match lease::try_key(root, reservation.key)? {
                Some(guard) => keys.push(guard),
                None => match fs::symlink_metadata(&path) {
                    // Absence does not retire anything. The unchanged reservation
                    // remains charged for the normal abandoned-work recovery path.
                    Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                    Ok(_) => return Ok(Step::Wait(path)),
                    Err(source) => {
                        return Err(Error::Io {
                            operation: "inspect migration reservation waiter",
                            path,
                            source,
                        })
                    }
                },
            }
        }
        recovery::recover_temporaries(publication, root, budget)?;
        let fenced = ledger.encode_schema(super::LEDGER_SCHEMA)?;
        let plan = crate::store::control::Plan::replacement(&path, fenced.len());
        super::physical(root, &ledger)?;
        let admitted = crate::store::control::Admitted::seal(publication, root, plan)?;
        entry::replace_admitted_file(
            admitted,
            publication,
            root,
            &path,
            &fenced,
            #[cfg(feature = "import-baseline")]
            crate::metrics::publication::Replacement::InstallationMigration,
            #[cfg(feature = "import-baseline")]
            Some(
                crate::metrics::principal::ledger::Write::installation(
                    crate::metrics::principal::ledger::Cause::Migration,
                )
                .with_before(crate::metrics::principal::ledger::Before::Bytes(&bytes)),
            ),
        )?;
        // Both sets span the atomic fence. Any old waiter subsequently resumes
        // against a mandatory decoder that rejects the permanent new schema.
        drop(keys);
        drop(pins);
    }
    // Control writes do not alter the installation ledger. Preserve its exact
    // post-fence identity in the original account rather than decoding per file.
    budget.retain_installation(
        root,
        std::sync::Arc::new(ledger),
        super::recovery_metadata(root)?,
    );
    finish_controls(publication, root, budget)?;
    Ok(Step::Complete)
}

fn finish_controls(
    publication: &lease::Publication<'_>,
    root: &Path,
    budget: &mut recovery::Budget,
) -> Result<(), Error> {
    let marker = root.join("locks/ownership.ready");
    match fs::symlink_metadata(&marker) {
        Ok(metadata) => {
            if metadata.len() != 0 {
                return Err(Error::Corrupt {
                    path: marker,
                    reason: "ownership completion bytes changed",
                });
            }
            lease::ensure_migration_file(publication, root, &marker, budget)?;
            if entry::existing_directory(root, Path::new("locks/owners"))?.is_none() {
                return Err(Error::CleanupRequired(
                    "completed ownership directory disappeared",
                ));
            }
            return Ok(());
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect migration completion",
                path: marker,
                source,
            })
        }
    }
    recovery::recover_temporaries(publication, root, budget)?;
    if entry::existing_directory(root, Path::new("locks/owners"))?.is_none() {
        let owners = root.join("locks/owners");
        #[cfg(feature = "import-baseline")]
        let observation = crate::metrics::principal::creation::Context::published(
            publication,
            crate::metrics::principal::creation::Source::OwnershipDirectory,
        )
        .start(&owners, true);
        let admitted = super::admit_control_recovery(
            publication,
            root,
            crate::store::control::Plan::directory(&owners),
            budget,
        )?;
        admitted.consume(publication, root, "directory", &owners, 0)?;
        fs::create_dir(&owners)
            .inspect(|_| {
                #[cfg(feature = "import-baseline")]
                observation.created(crate::metrics::principal::creation::Outcome::Created);
            })
            .inspect_err(|_source| {
                #[cfg(feature = "import-baseline")]
                observation.created(if _source.kind() == std::io::ErrorKind::AlreadyExists {
                    crate::metrics::principal::creation::Outcome::Contended
                } else {
                    crate::metrics::principal::creation::Outcome::Failed
                });
            })
            .map_err(|source| Error::Io {
                operation: "create ownership directory",
                path: owners,
                source,
            })?;
        let result = entry::sync_directory(&root.join("locks"));
        #[cfg(feature = "import-baseline")]
        {
            observation.synced(
                crate::metrics::principal::creation::Sync::Parent,
                result.is_ok(),
            );
            observation.completed(result.is_ok());
        }
        result?;
    }
    let projects = root.join("locks/projects");
    let entries = fs::read_dir(&projects).map_err(|source| Error::Io {
        operation: "read prior project controls",
        path: projects.clone(),
        source,
    })?;
    let mut names = Vec::new();
    for child in entries {
        let child = child.map_err(|source| Error::Io {
            operation: "read prior project control",
            path: projects.clone(),
            source,
        })?;
        let name = child.file_name();
        budget.metadata(
            name.as_encoded_bytes()
                .len()
                .checked_add(std::mem::size_of::<std::ffi::OsString>())
                .ok_or(Error::Allocation)?,
        )?;
        names.try_reserve(1).map_err(|_| Error::Allocation)?;
        names.push(name);
    }
    names.sort_unstable();
    for name in names {
        // Existing project controls are validated before their ownership peer.
        lease::ensure_migration_file(publication, root, &projects.join(&name), budget)?;
        lease::ensure_migration_file(
            publication,
            root,
            &root.join("locks/owners").join(name),
            budget,
        )?;
    }
    entry::sync_directory(&root.join("locks/owners"))?;
    lease::ensure_migration_file(publication, root, &marker, budget)?;
    entry::sync_directory(&root.join("locks"))
}

#[cfg(test)]
mod tests;
