//! Where a snippet with a frontmatter block is written and its runner
//! built: one directory for each such snippet that runs at once, `0`, `1`,
//! .. under the scripts' directory. A run holds its slot's lock file until
//! it ends, and the next run takes the first slot no run holds.
//!
//! rawr keeps a script's manifest, lockfile and runner by the script's
//! path. A slot is the same path from run to run, so what the snippets
//! rawr keeps is bounded by how many run at once, and a snippet that names
//! the dependencies the last one in its slot named finds them locked.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::SOURCE_NAME;

/// One slot, held until dropped.
#[derive(Debug)]
pub(crate) struct Slot {
    directory: PathBuf,
    /// Locked while the slot is in use: closing it frees the slot, however
    /// the run ends.
    _lock: fs::File,
}

impl Slot {
    /// The first slot under `scripts` that no run holds, held from now.
    pub(crate) fn take(scripts: &Path) -> io::Result<Self> {
        let mut index = 0_usize;
        loop {
            let directory = scripts.join(index.to_string());
            fs::create_dir_all(&directory)?;
            let lock = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(directory.join(".lock"))?;
            match lock.try_lock() {
                Ok(()) => {
                    // rawr keys the script by its canonical path, which its
                    // runner's server must load it at too.
                    let directory = fs::canonicalize(&directory)?;
                    return Ok(Self {
                        directory,
                        _lock: lock,
                    });
                }
                Err(fs::TryLockError::WouldBlock) => index += 1,
                Err(fs::TryLockError::Error(error)) => return Err(error),
            }
        }
    }

    /// The slot's directory.
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    /// The script's file: `play.rs`, the name every report on it shows.
    pub(crate) fn script(&self) -> PathBuf {
        self.directory.join(SOURCE_NAME)
    }

    /// The directory rawr-import writes the script's runner to.
    pub(crate) fn out(&self) -> PathBuf {
        self.directory.join("out")
    }
}

#[cfg(test)]
mod tests {
    use super::Slot;

    /// A slot in use is not taken again, and a freed one is.
    #[test]
    fn each_run_at_once_takes_a_slot_of_its_own() {
        let scripts = std::env::temp_dir().join(format!("rawr-play-slots-{}", std::process::id()));
        let first = Slot::take(&scripts).unwrap();
        let second = Slot::take(&scripts).unwrap();
        assert_ne!(first.directory(), second.directory());
        assert!(
            first.script().ends_with("0/play.rs"),
            "{}",
            first.script().display()
        );
        drop(first);
        let again = Slot::take(&scripts).unwrap();
        assert!(
            again.directory().ends_with("0"),
            "{}",
            again.directory().display()
        );
        drop((second, again));
        let _ = std::fs::remove_dir_all(&scripts);
    }
}
