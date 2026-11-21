//! A lock on work that one rawr does for others: rawr's runtime, the
//! nightly build directory that feeds extraction, a toolchain's driver, a
//! project's runner build, its resolution and runner caches and its store.
//!
//! A process that holds one exclusively writes its pid into the lock file,
//! and empties the file before it unlocks. A process that finds the lock
//! taken says once that it waits, as Cargo's "Blocking waiting for file
//! lock" does, naming that pid when an exclusive holder has it, and then
//! waits on the lock itself: no deadline and no polling. Leases, which are
//! shared, record nothing, so a wait for leases to end names no process.
//!
//! A holder that builds for a project writes the project after its pid,
//! and a wait names both, so a user sees which build blocks them.
//!
//! A lock whose file's bytes are fixed, such as a store's leases, records
//! nothing: a [`Fixed`] hold says the wait naming what the lock covers.
//!
//! Each hold is released by an explicit unlock, not by close, so a child
//! process that inherited the file holds nothing (D1036.1).

use std::fs::{self, File, TryLockError};
use std::io;
use std::os::unix::fs::FileExt as _;
use std::path::Path;

/// How a lock is taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Mode {
    /// A lease: any number of holders, none of them writing.
    Shared,
    /// One holder, which writes what the lock protects.
    Exclusive,
}

/// An exclusive hold on a lock file, released when this goes.
#[derive(Debug)]
pub(crate) struct Exclusive(File);

impl Exclusive {
    /// Hold `file`, the lock at `path` on `what`, waiting for its holder
    /// first and saying so on the status line.
    pub(crate) fn new(file: File, path: &Path, what: &str) -> io::Result<Self> {
        wait(&file, path, what, Mode::Exclusive, crate::progress::say)?;
        Ok(Self(file))
    }

    /// [`new`](Self::new) for a build of `project`, which a wait for this
    /// hold names after the pid.
    pub(crate) fn building(
        file: File,
        path: &Path,
        what: &str,
        project: &Path,
    ) -> io::Result<Self> {
        wait(&file, path, what, Mode::Exclusive, crate::progress::say)?;
        let pid = std::process::id().to_string();
        let project = format!(" {}", project.display());
        file.write_all_at(project.as_bytes(), pid.len() as u64)?;
        Ok(Self(file))
    }

    /// [`new`](Self::new), saying the wait through `say`.
    #[cfg(test)]
    fn new_saying(file: File, path: &Path, what: &str, say: impl FnOnce(&str)) -> io::Result<Self> {
        wait(&file, path, what, Mode::Exclusive, say)?;
        Ok(Self(file))
    }
}

impl Drop for Exclusive {
    fn drop(&mut self) {
        let _ = release(&self.0, Mode::Exclusive);
    }
}

/// An exclusive hold on a lock file whose bytes are fixed, released when
/// this goes. It records no holder, so a wait for it names only what the
/// lock covers.
#[derive(Debug)]
pub(crate) struct Fixed(File);

impl Fixed {
    /// Hold `file`, the lock on `what`, waiting for its holder first and
    /// saying so on the status line.
    pub(crate) fn new(file: File, what: &str) -> io::Result<Self> {
        Self::new_saying(file, what, crate::progress::say)
    }

    /// [`new`](Self::new), saying the wait through `say`.
    pub(crate) fn new_saying(file: File, what: &str, say: impl FnOnce(&str)) -> io::Result<Self> {
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                say(&message(what, None));
                file.lock()?;
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
        Ok(Self(file))
    }

    /// The held file.
    pub(crate) const fn file(&self) -> &File {
        &self.0
    }
}

impl Drop for Fixed {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Take `file`, the lock at `path` on `what`, in `mode`. When another
/// process has it, say so once through `say`, then wait for it. An
/// exclusive hold records this process's pid in the file.
pub(crate) fn wait(
    file: &File,
    path: &Path,
    what: &str,
    mode: Mode,
    say: impl FnOnce(&str),
) -> io::Result<()> {
    let tried = match mode {
        Mode::Shared => file.try_lock_shared(),
        Mode::Exclusive => file.try_lock(),
    };
    match tried {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            say(&message(what, builder(file, path, mode)?));
            match mode {
                Mode::Shared => file.lock_shared()?,
                Mode::Exclusive => file.lock()?,
            }
        }
        Err(TryLockError::Error(error)) => return Err(error),
    }
    if mode == Mode::Exclusive {
        file.set_len(0)?;
        file.write_all_at(std::process::id().to_string().as_bytes(), 0)?;
    }
    Ok(())
}

/// Release `file`'s lock, taken in `mode`. An exclusive holder empties
/// the file first, so no later wait names a process that is gone.
pub(crate) fn release(file: &File, mode: Mode) -> io::Result<()> {
    if mode == Mode::Exclusive {
        file.set_len(0)?;
    }
    file.unlock()
}

/// The exclusive holder of the lock at `path`, which `file` failed to take
/// in `mode`: its pid, and the project it builds when it wrote one. A lock
/// that a shared take would get has only leases, so no builder.
fn builder(file: &File, path: &Path, mode: Mode) -> io::Result<Option<Builder>> {
    if mode == Mode::Exclusive {
        match file.try_lock_shared() {
            Ok(()) => {
                file.unlock()?;
                return Ok(None);
            }
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
    // The holder writes its pid once it has the lock, so a read in between
    // finds the file empty.
    Ok(fs::read_to_string(path).ok().and_then(|text| {
        let (pid, project) = text.split_once(' ').unwrap_or((text.as_str(), ""));
        Some(Builder {
            pid: pid.trim().parse().ok()?,
            project: project.trim().to_owned(),
        })
    }))
}

/// The process that holds a lock exclusively, as its file records it.
struct Builder {
    pid: u32,
    /// The project it builds, empty when it recorded none.
    project: String,
}

/// What a wait for the lock on `what` says.
fn message(what: &str, builder: Option<Builder>) -> String {
    match builder {
        Some(Builder { pid, project }) if project.is_empty() => {
            format!("    Blocking waiting for file lock on {what}, held by process {pid}")
        }
        Some(Builder { pid, project }) => format!(
            "    Blocking waiting for file lock on {what}, held by process {pid} building {project}"
        ),
        None => format!("    Blocking waiting for file lock on {what}"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File, OpenOptions};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

    use super::{Exclusive, Mode};

    fn open(path: &Path) -> File {
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .expect("the scratch directory is writable")
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("rawr-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("the temporary directory is writable");
        directory
    }

    /// Wait for the lock at `path` in `mode` on another thread, and return
    /// what it said once it is blocked, with the thread, which ends once it
    /// holds the lock and lets go. `say` runs before the wait blocks, so
    /// nothing here waits on a clock, and it runs at most once.
    fn blocked(path: &Path, mode: Mode) -> (String, std::thread::JoinHandle<()>) {
        let (said, message) = mpsc::channel();
        let path = path.to_path_buf();
        let waiter = std::thread::spawn(move || {
            let file = open(&path);
            super::wait(&file, &path, "rawr's runtime", mode, |line| {
                said.send(line.to_owned()).expect("the test listens");
            })
            .expect("the lock is taken once its holder lets go");
            super::release(&file, mode).expect("the lock is held");
        });
        let message = message.recv().expect("a blocked wait says so");
        (message, waiter)
    }

    /// A builder records its pid while it holds the lock and clears it when
    /// it lets go. A rawr that waits for it, for a lease or to build, says
    /// so once, naming it, and takes the lock once the builder is done
    /// (#1408).
    #[test]
    fn a_wait_for_a_builder_says_so_once_naming_its_process() {
        let directory = scratch("blocking-builder");
        let path = directory.join(".lock");
        let held = Exclusive::new_saying(open(&path), &path, "rawr's runtime", |_| {
            panic!("nothing else holds the lock")
        })
        .expect("the lock is free");
        let pid = std::process::id();
        assert_eq!(fs::read_to_string(&path).unwrap(), pid.to_string());
        let named =
            format!("    Blocking waiting for file lock on rawr's runtime, held by process {pid}");
        let (leasing, lease) = blocked(&path, Mode::Shared);
        let (building, build) = blocked(&path, Mode::Exclusive);
        assert_eq!(leasing, named);
        assert_eq!(building, named);
        assert!(
            !lease.is_finished() && !build.is_finished(),
            "both wait for the builder"
        );
        drop(held);
        lease.join().unwrap();
        build.join().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "",
            "each builder clears its pid"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    /// A lock whose bytes are fixed keeps them: a wait for it names what it
    /// covers and no process, and goes on once the holder lets go.
    #[test]
    fn a_fixed_lock_records_nothing_and_its_wait_names_what_it_covers() {
        let directory = scratch("blocking-fixed");
        let path = directory.join(".lock");
        let held = super::Fixed::new_saying(open(&path), "the store", |_| {
            panic!("nothing else holds the lock")
        })
        .expect("the lock is free");
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        let (said, message) = mpsc::channel();
        let waiting = path.clone();
        let waiter = std::thread::spawn(move || {
            drop(
                super::Fixed::new_saying(open(&waiting), "the store", |line| {
                    said.send(line.to_owned()).expect("the test listens");
                })
                .expect("the lock is taken once its holder lets go"),
            );
        });
        assert_eq!(
            message.recv().expect("a blocked wait says so"),
            "    Blocking waiting for file lock on the store"
        );
        drop(held);
        waiter.join().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        fs::remove_dir_all(directory).unwrap();
    }

    /// A rawr that waits for other rawrs' leases to end names no process:
    /// none of them builds, and the last builder's pid is gone from the
    /// file.
    #[test]
    fn a_wait_for_leases_names_no_process() {
        let directory = scratch("blocking-leases");
        let path = directory.join(".lock");
        drop(Exclusive::new(open(&path), &path, "rawr's runtime").expect("the lock is free"));
        let lease = open(&path);
        super::wait(&lease, &path, "rawr's runtime", Mode::Shared, |_| {
            panic!("a lease waits for no other lease")
        })
        .unwrap();
        let (message, waiter) = blocked(&path, Mode::Exclusive);
        assert_eq!(
            message,
            "    Blocking waiting for file lock on rawr's runtime"
        );
        super::release(&lease, Mode::Shared).unwrap();
        waiter.join().unwrap();
        fs::remove_dir_all(directory).unwrap();
    }
}
