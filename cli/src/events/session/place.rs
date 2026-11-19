//! Where a program's session is: a directory a shell finds from the
//! program's root alone, holding the session's socket and the program's
//! inspector socket and breakpoints.

use std::path::{Path, PathBuf};

/// A program's root as its session is named by: its package's root, or
/// the single file.
#[must_use]
pub fn root(target: &rawr::project::Target) -> PathBuf {
    target.project().map_or_else(
        || target.source().path().to_path_buf(),
        |project| project.root().as_path().to_path_buf(),
    )
}

/// The directory of one program's session. It sits in a directory only
/// this user can enter, under the system's temporary directory because a
/// socket's path has a short limit, and is named by a digest of the
/// program's root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    directory: PathBuf,
}

impl Place {
    /// The place of the program at `root`, in the user's directory, which
    /// is made when it isn't there. The place itself is made by
    /// [`Self::make`], for a run that hosts a session there.
    ///
    /// # Errors
    ///
    /// When the user's directory can't be made, or is another user's or
    /// open to others.
    #[cfg(unix)]
    pub fn new(root: &Path) -> std::io::Result<Self> {
        use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};

        let user = std::env::temp_dir().join(format!("rawr-{}", nix::unistd::getuid()));
        match std::fs::DirBuilder::new().mode(0o700).create(&user) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = std::fs::symlink_metadata(&user)?;
                if !metadata.is_dir()
                    || metadata.uid() != nix::unistd::getuid().as_raw()
                    || metadata.mode() & 0o077 != 0
                {
                    return Err(std::io::Error::other(format!(
                        "{} isn't a directory of this user's alone",
                        user.display()
                    )));
                }
            }
            Err(error) => return Err(error),
        }
        // A shell and the owner may spell the root apart, through a symlink.
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        Ok(Self {
            directory: user.join(digest(&root)),
        })
    }

    /// Make the place, for a session to listen in.
    ///
    /// # Errors
    ///
    /// When the directory can't be made.
    #[cfg(unix)]
    pub fn make(&self) -> std::io::Result<()> {
        use std::os::unix::fs::DirBuilderExt as _;

        match std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&self.directory)
        {
            Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => Err(error),
            _ => Ok(()),
        }
    }

    /// The socket the session listens on.
    #[must_use]
    pub fn socket(&self) -> PathBuf {
        self.directory.join("session.sock")
    }

    /// The socket the program's inspector listens on; its breakpoints are
    /// kept beside it.
    #[must_use]
    pub fn inspector(&self) -> PathBuf {
        self.directory.join("inspect.sock")
    }

    /// The file the program's inspector keeps its breakpoints in.
    #[must_use]
    pub fn breakpoints(&self) -> PathBuf {
        self.directory.join(rawr::inspect::BREAKPOINTS)
    }

    /// What a run leaves here for the next: the breakpoints and a
    /// program's stale socket. A new run starts with none.
    pub fn clear(&self) {
        let _ = std::fs::remove_file(self.inspector());
        let _ = std::fs::remove_file(self.breakpoints());
    }

    /// Remove the place, once its run is over.
    pub fn remove(&self) {
        self.clear();
        let _ = std::fs::remove_file(self.socket());
        let _ = std::fs::remove_dir(&self.directory);
    }
}

/// A short name for `root`: a 64-bit FNV-1a digest of its path.
fn digest(root: &Path) -> String {
    let digest = root
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{digest:016x}")
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;

    use super::Place;

    #[test]
    fn a_root_has_one_place_with_a_short_socket_path() {
        let place = Place::new(Path::new("/p/a")).unwrap();
        assert_eq!(place, Place::new(Path::new("/p/a")).unwrap());
        assert_ne!(place, Place::new(Path::new("/p/b")).unwrap());
        // A Unix socket's path is at most 104 bytes on macOS.
        assert!(place.inspector().as_os_str().len() < 104, "{place:?}");
    }
}
