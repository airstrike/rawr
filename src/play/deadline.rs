//! A run's deadline that another process may set while the run goes on.
//!
//! The bot gives a run no deadline while no other snippet waits, and one
//! once another arrives. It writes that deadline to a [`File`], as Unix
//! milliseconds; the run's processes read it there with a [`Watch`].

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How often a [`Watch`] reads its file again.
const READ_EVERY: Duration = Duration::from_millis(50);

/// A file holding a run's deadline, empty until one is set, removed when
/// dropped.
#[derive(Debug)]
pub struct File(PathBuf);

impl File {
    /// An empty deadline file in the temporary directory.
    pub fn new() -> io::Result<Self> {
        static FILES: AtomicU64 = AtomicU64::new(0);
        let file = FILES.fetch_add(1, Ordering::Relaxed);
        let name = format!("rawr-play-{}-{file}.deadline", std::process::id());
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, "")?;
        Ok(Self(path))
    }

    /// Where it is, for the run's processes to read.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Set the deadline to `at`. A reader sees the old content or the new,
    /// never part of it.
    pub fn set(&self, at: SystemTime) -> io::Result<()> {
        let millis = at
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        let mut next = self.0.clone().into_os_string();
        next.push(".next");
        std::fs::write(&next, millis.to_string())?;
        std::fs::rename(next, &self.0)
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A run's deadline as one of its processes sees it: a ceiling fixed at its
/// start, or the sooner one its [`File`] gives once that is set.
#[derive(Clone, Debug)]
pub struct Watch {
    ceiling: Instant,
    file: Option<PathBuf>,
    set: Option<Instant>,
    read: Instant,
}

impl Watch {
    /// A deadline of `ceiling`, or sooner when the file at `file` says so.
    #[must_use]
    pub fn new(ceiling: Instant, file: Option<PathBuf>) -> Self {
        Self {
            ceiling,
            file,
            set: None,
            read: Instant::now(),
        }
    }

    /// The deadline at `now`, reading the file when it is due.
    pub fn at(&mut self, now: Instant) -> Instant {
        if self.set.is_none()
            && now >= self.read
            && let Some(file) = &self.file
        {
            self.set = read(file).map(|at| instant(at, now));
            self.read = now + READ_EVERY;
        }
        self.set.map_or(self.ceiling, |set| set.min(self.ceiling))
    }

    /// Whether the deadline has passed at `now`.
    pub fn passed(&mut self, now: Instant) -> bool {
        now >= self.at(now)
    }
}

/// The deadline the file at `path` holds; `None` while it holds none.
fn read(path: &Path) -> Option<SystemTime> {
    let text = std::fs::read_to_string(path).ok()?;
    let millis = text.trim().parse::<u64>().ok()?;
    Some(UNIX_EPOCH + Duration::from_millis(millis))
}

/// `at` on the monotonic clock, as seen at `now`.
fn instant(at: SystemTime, now: Instant) -> Instant {
    at.duration_since(SystemTime::now())
        .map_or(now, |left| now + left)
}

#[cfg(test)]
mod tests {
    use super::{File, Watch};
    use std::time::{Duration, Instant, SystemTime};

    #[test]
    fn a_watch_keeps_its_ceiling_until_the_file_sets_a_sooner_deadline() {
        let file = File::new().unwrap();
        let start = Instant::now();
        let ceiling = start + Duration::from_secs(600);
        let mut watch = Watch::new(ceiling, Some(file.path().to_owned()));
        assert_eq!(watch.at(start), ceiling);
        assert!(!watch.passed(start));
        file.set(SystemTime::now() + Duration::from_secs(1))
            .unwrap();
        // Read again only once it is due.
        assert_eq!(watch.at(start), ceiling);
        let later = start + Duration::from_millis(60);
        let set = watch.at(later);
        assert!(set < start + Duration::from_secs(2), "{set:?}");
        assert!(watch.passed(start + Duration::from_secs(3)));
        // A deadline past the ceiling leaves the ceiling.
        let near = Instant::now() + Duration::from_millis(10);
        file.set(SystemTime::now() + Duration::from_secs(60))
            .unwrap();
        let mut watch = Watch::new(near, Some(file.path().to_owned()));
        assert_eq!(watch.at(Instant::now()), near);
        let path = file.path().to_owned();
        drop(file);
        assert!(!path.exists());
    }

    #[test]
    fn a_watch_with_no_file_is_its_ceiling() {
        let ceiling = Instant::now();
        assert!(Watch::new(ceiling, None).passed(ceiling));
    }
}
