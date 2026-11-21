//! The project cache's resolution lease, held across one resolution or one
//! publication.
//!
//! A second process, or a second thread with its own open file, that finds
//! the lease held says once that it waits, naming the holder's pid and
//! project, and blocks on the lock as Cargo does on its build directory.
//! The OS lets go of the lock when the holder finishes or dies.
//!
//! A thread that already holds the lease would wait on itself forever, since
//! the lock belongs to the outer hold's open file. Each thread records the
//! leases it holds, and a nested hold is a rawr bug that panics at once.

use std::cell::RefCell;
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

std::thread_local! {
    /// The resolution leases this thread holds, by lease path.
    static HELD: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// A held resolution lease. It unlocks and leaves this thread's record when
/// it goes.
pub(crate) struct Lease {
    path: PathBuf,
    _held: crate::blocking::Exclusive,
}

impl Lease {
    /// Hold `file`, the lease at `path` in `project`'s cache, waiting for
    /// another holder first.
    ///
    /// # Panics
    ///
    /// When this thread already holds the lease at `path`.
    pub(crate) fn hold(
        file: File,
        path: &Path,
        project: &Path,
        operation: &'static str,
    ) -> Result<Self> {
        HELD.with_borrow(|held| {
            assert!(
                !held.iter().any(|outer| outer == path),
                "a resolution lease was taken again on the thread that holds it: {}",
                path.display()
            );
        });
        let what = format!("the resolution cache of {}", project.display());
        let held =
            crate::blocking::Exclusive::building(file, path, &what, project).map_err(|source| {
                Error::Io {
                    operation,
                    path: path.to_path_buf(),
                    source,
                }
            })?;
        HELD.with_borrow_mut(|frames| frames.push(path.to_path_buf()));
        Ok(Self {
            path: path.to_path_buf(),
            _held: held,
        })
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        HELD.with_borrow_mut(|held| {
            if let Some(index) = held.iter().rposition(|frame| *frame == self.path) {
                held.remove(index);
            }
        });
    }
}
