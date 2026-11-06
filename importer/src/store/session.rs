//! A project's session lease on the store it shares a target directory with.
//!
//! Each process that opens a project's store holds that project's lease,
//! shared, for as long as it keeps the store. Collection takes another
//! project's units only while it holds that project's lease exclusively, so
//! it never deletes what a running check or run of that project uses. The
//! leases live beside the store root (`store.sessions` beside `store`),
//! outside the store's own inventory.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::node::{Digest, Error};

const DIRECTORY: &str = "sessions";

/// One process's shared hold on a project's lease.
pub(crate) struct Session {
    _file: File,
}

/// A project's lease held exclusively: no process has its store open, and
/// none opens it until this is dropped.
pub(super) struct Idle {
    path: PathBuf,
    _file: File,
}

impl Session {
    /// Enter `project`'s session in the store at `root`, waiting while a
    /// collection holds it.
    pub(super) fn enter(root: &Path, project: Digest) -> Result<Self, Error> {
        let path = lease(root, project);
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory).map_err(|source| Error::Io {
                operation: "create the store's session leases",
                path: directory.to_path_buf(),
                source,
            })?;
        }
        loop {
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&path)
                .map_err(|source| Error::Io {
                    operation: "open the project's session lease",
                    path: path.clone(),
                    source,
                })?;
            file.lock_shared().map_err(|source| Error::Io {
                operation: "enter the project's session",
                path: path.clone(),
                source,
            })?;
            // A collection removes the lease of the project it collected;
            // a hold taken through the removed file guards nothing.
            if current(&file, &path) {
                return Ok(Self { _file: file });
            }
        }
    }
}

impl Idle {
    /// `project`'s lease when no process holds it; `None` while one does.
    /// A project without one, enrolled before leases or collected before,
    /// gets one, so a process that opens it meanwhile waits for this.
    pub(super) fn take(root: &Path, project: Digest) -> Result<Option<Self>, Error> {
        let path = lease(root, project);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| Error::Io {
                operation: "open another project's session lease",
                path: path.clone(),
                source,
            })?;
        match file.try_lock() {
            Ok(()) if current(&file, &path) => Ok(Some(Self { path, _file: file })),
            Ok(()) | Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(source)) => Err(Error::Io {
                operation: "take another project's session lease",
                path,
                source,
            }),
        }
    }

    /// Remove the lease of a project whose units were collected, so later
    /// opens of the store find no other project to collect.
    pub(super) fn retire(self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Whether any project other than `project` holds or held a lease here.
pub(super) fn others(root: &Path, project: Digest) -> bool {
    let own = name(project);
    let Some(directory) = lease(root, project).parent().map(Path::to_path_buf) else {
        return false;
    };
    fs::read_dir(directory).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().to_str().is_some_and(|other| other != own))
    })
}

fn lease(root: &Path, project: Digest) -> PathBuf {
    root.with_file_name(format!(
        "{}.{DIRECTORY}",
        root.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("store")
    ))
    .join(name(project))
}

fn name(project: Digest) -> String {
    format!("{}.lock", project.directory())
}

/// Whether `file` is still the lease at `path`.
fn current(file: &File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        match (file.metadata(), fs::metadata(path)) {
            (Ok(held), Ok(now)) => held.dev() == now.dev() && held.ino() == now.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        path.exists()
    }
}
