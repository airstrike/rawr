//! Where a run's session is: a directory a shell finds from the program's
//! root alone, one per run, holding the session's socket and the program's
//! inspector socket and breakpoints.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A program's root as its session is named by: its package's root, or
/// the single file.
#[must_use]
pub fn root(target: &rawr::project::Target) -> PathBuf {
    target.project().map_or_else(
        || target.source().path().to_path_buf(),
        |project| project.root().as_path().to_path_buf(),
    )
}

/// The directory of one run's session. Each program has a directory of
/// its own, in a directory only this user can enter, under the system's
/// temporary directory because a socket's path has a short limit, and
/// named by a digest of the program's root. Each run's place sits in it,
/// named by the pid of the process that bound the run's socket, so two
/// runs of one program each host a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    directory: PathBuf,
    pid: u32,
}

impl Place {
    /// This process's place for the program at `root`, in the user's
    /// directory, which is made when it isn't there. The place itself is
    /// made by [`Self::make`], for a run that hosts a session there.
    ///
    /// # Errors
    ///
    /// When the user's directory can't be made, or is another user's or
    /// open to others.
    pub fn new(root: &Path) -> std::io::Result<Self> {
        Self::of(root, std::process::id())
    }

    /// The place of the run of the program at `root` whose socket the
    /// process `pid` bound: a terminal's `rawr run`, or the shell that
    /// started the run.
    ///
    /// # Errors
    ///
    /// As [`Self::new`].
    pub fn of(root: &Path, pid: u32) -> std::io::Result<Self> {
        Ok(Self {
            directory: program(root)?.join(pid.to_string()),
            pid,
        })
    }

    /// The place whose socket `listener` is bound to, as the shell that
    /// started this run bound it: the directory the socket is in, whatever
    /// the shell named it, and the shell's pid, as the shell is this
    /// process's parent (`exec` keeps it). The run serves where its shell
    /// waits, and reads nothing of the shell's layout from the path, so a
    /// shell that names its places otherwise, as one built before each run
    /// had its own place does, still hears its run (#2441).
    ///
    /// # Errors
    ///
    /// When `listener` isn't bound to a path.
    pub fn bound(listener: &UnixListener) -> std::io::Result<Self> {
        let address = listener.local_addr()?;
        let directory = address
            .as_pathname()
            .and_then(Path::parent)
            .ok_or_else(|| std::io::Error::other(format!("{address:?} isn't bound to a path")))?;
        Ok(Self {
            directory: directory.to_path_buf(),
            pid: std::os::unix::process::parent_id(),
        })
    }

    /// The place at `directory`, named by its run's pid.
    fn at(directory: &Path) -> Option<Self> {
        let pid = directory.file_name()?.to_str()?.parse().ok()?;
        Some(Self {
            directory: directory.to_path_buf(),
            pid,
        })
    }

    /// The pid of the process that bound the run's socket.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Make the place, for a session to listen in.
    ///
    /// # Errors
    ///
    /// When the directory can't be made.
    pub fn make(&self) -> std::io::Result<()> {
        use std::os::unix::fs::DirBuilderExt as _;

        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700).recursive(true);
        loop {
            match builder.create(&self.directory) {
                // The program's directory went with another run's place
                // between making it and making this one in it.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                made => return made,
            }
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

    /// Remove the place, once its run is over, and the program's directory
    /// when no other run's place is in it.
    pub fn remove(&self) {
        self.clear();
        let _ = std::fs::remove_file(self.socket());
        let _ = std::fs::remove_dir(&self.directory);
        if let Some(program) = self.directory.parent() {
            let _ = std::fs::remove_dir(program);
        }
    }
}

/// The runs of one program whose sessions a shell can reach, newest
/// first.
#[derive(Debug, Default)]
pub struct Runs {
    places: Vec<Place>,
}

impl Runs {
    /// The runs of the program at `root` now. A place whose process is
    /// gone and whose socket takes no connection is a dead run's, and is
    /// removed on the way.
    ///
    /// # Errors
    ///
    /// When the user's directory can't be made, or is another user's or
    /// open to others.
    pub fn of(root: &Path) -> std::io::Result<Self> {
        let entries = match std::fs::read_dir(program(root)?) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error),
        };
        let mut runs: Vec<(SystemTime, Place)> = Vec::new();
        for entry in entries.flatten() {
            let Some(place) = Place::at(&entry.path()) else {
                continue;
            };
            let bound = std::fs::symlink_metadata(place.socket()).and_then(|meta| meta.modified());
            // A run a shell started outlives the shell that bound its socket.
            let live = alive(place.pid) || UnixStream::connect(place.socket()).is_ok();
            match bound {
                Ok(bound) if live => runs.push((bound, place)),
                // A process makes its place before it binds there.
                Err(_) if live => {}
                _ => place.remove(),
            }
        }
        runs.sort_by(|(a, first), (b, second)| b.cmp(a).then(second.pid.cmp(&first.pid)));
        Ok(Self {
            places: runs.into_iter().map(|(_, place)| place).collect(),
        })
    }

    /// Connect to the run whose pid is `pid`, or to the newest that takes
    /// the connection. `None` when none does.
    #[must_use]
    pub fn connect(&self, pid: Option<u32>) -> Option<Reached> {
        self.places
            .iter()
            .filter(|place| pid.is_none_or(|pid| place.pid == pid))
            .find_map(|place| {
                let stream = UnixStream::connect(place.socket()).ok()?;
                Some(Reached {
                    stream,
                    place: place.clone(),
                    others: self.pids().filter(|other| *other != place.pid).collect(),
                    named: pid.is_some(),
                })
            })
    }

    /// Why no run whose pid is `pid` was reached: the pids there are.
    #[must_use]
    pub fn unserved(&self, pid: u32) -> std::io::Error {
        let pids: Vec<String> = self.pids().map(|pid| pid.to_string()).collect();
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            if pids.is_empty() {
                format!("no run of pid {pid} serves this program: none does")
            } else {
                format!(
                    "no run of pid {pid} serves this program: pid {} do",
                    pids.join(", ")
                )
            },
        )
    }

    /// The pids of the runs, newest first.
    pub fn pids(&self) -> impl Iterator<Item = u32> + '_ {
        self.places.iter().map(Place::pid)
    }
}

/// A connection to one run's session, and the program's other runs.
#[derive(Debug)]
pub struct Reached {
    /// The connection to the run's session.
    pub stream: UnixStream,
    /// The pids of the program's other runs, newest first.
    pub others: Vec<u32>,
    place: Place,
    named: bool,
}

impl Reached {
    /// The pid of the process that bound the run's socket.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.place.pid
    }

    /// The run's session socket.
    #[must_use]
    pub fn socket(&self) -> PathBuf {
        self.place.socket()
    }

    /// What a shell tells when it attached to the newest of several runs:
    /// the others' pids, and how to pick one, `pick`. `None` when the
    /// program has one run, or the shell named it.
    #[must_use]
    pub fn notice(&self, pick: &str) -> Option<String> {
        if self.named || self.others.is_empty() {
            return None;
        }
        let others: Vec<String> = self.others.iter().map(u32::to_string).collect();
        Some(format!(
            "rawr: other runs serve this program too (pid {}), so this attached to the newest, pid {}. {pick}",
            others.join(", "),
            self.pid(),
        ))
    }
}

/// Whether the process `pid` is there.
fn alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    !matches!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
        Err(nix::errno::Errno::ESRCH)
    )
}

/// The directory of the runs of the program at `root`, in the user's
/// directory, which is made when it isn't there.
fn program(root: &Path) -> std::io::Result<PathBuf> {
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
    Ok(user.join(digest(&root)))
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

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::{Place, Runs};

    /// The place of the run of `root` whose socket the process `pid` bound.
    fn place_of(root: &Path, pid: u32) -> Place {
        Place::of(root, pid).unwrap()
    }

    /// A pid no process has now: a child's, once it ended.
    fn dead_pid() -> u32 {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn each_run_of_a_root_has_its_own_place_with_a_short_socket_path() {
        let place = Place::new(Path::new("/p/a")).unwrap();
        assert_eq!(place, Place::new(Path::new("/p/a")).unwrap());
        assert_ne!(place, Place::new(Path::new("/p/b")).unwrap());
        assert_eq!(place.pid(), std::process::id());
        let other = place_of(Path::new("/p/a"), place.pid() + 1);
        assert_ne!(place.socket(), other.socket());
        assert_eq!(
            place.socket().parent(),
            Some(place.inspector().parent().unwrap())
        );
        // A Unix socket's path is at most 104 bytes on macOS, and a pid
        // has at most 7 digits.
        let longest = place_of(Path::new("/p/a"), 9_999_999);
        assert!(longest.inspector().as_os_str().len() < 104, "{longest:?}");
    }

    #[test]
    fn a_run_a_shell_started_finds_its_place_from_the_listener_it_was_handed() {
        let root = format!("/rawr-place-test/handed/{}", std::process::id());
        let shell = place_of(Path::new(&root), std::os::unix::process::parent_id());
        shell.make().unwrap();
        let listener = UnixListener::bind(shell.socket()).unwrap();
        assert_eq!(Place::bound(&listener).unwrap(), shell);
        drop(listener);
        shell.remove();
    }

    #[test]
    fn a_dead_run_s_place_is_removed_and_the_newest_live_run_is_reached() {
        let root = format!("/rawr-place-test/runs/{}", std::process::id());
        let root = Path::new(&root);
        let mut other = Command::new("cat").stdin(Stdio::piped()).spawn().unwrap();

        let dead = place_of(root, dead_pid());
        dead.make().unwrap();
        drop(UnixListener::bind(dead.socket()).unwrap());
        let older = place_of(root, other.id());
        older.make().unwrap();
        let _older = UnixListener::bind(older.socket()).unwrap();
        let newest = Place::new(root).unwrap();
        newest.make().unwrap();
        let _newest = UnixListener::bind(newest.socket()).unwrap();

        let runs = Runs::of(root).unwrap();
        assert!(!dead.socket().exists(), "a dead run's socket isn't busy");
        let pids: Vec<u32> = runs.pids().collect();
        assert_eq!(pids.len(), 2, "{pids:?}");
        assert!(pids.contains(&older.pid()) && pids.contains(&newest.pid()));

        let reached = runs.connect(None).unwrap();
        assert_eq!(reached.pid(), pids[0]);
        assert_eq!(reached.others, [pids[1]]);
        assert!(reached
            .notice("pick")
            .unwrap()
            .contains(&pids[1].to_string()));
        let named = runs.connect(Some(older.pid())).unwrap();
        assert_eq!(named.pid(), older.pid());
        assert_eq!(named.notice("pick"), None);
        assert!(runs.connect(Some(dead.pid())).is_none());

        other.kill().unwrap();
        other.wait().unwrap();
        older.remove();
        newest.remove();
    }
}
