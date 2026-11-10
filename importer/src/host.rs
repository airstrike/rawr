//! The Rawr source tree a generated harness builds against.
//!
//! An installed `rawr` carries the library sources a harness compiles and
//! writes them once to `$CARGO_HOME/rawr/hosts/<digest>`, a directory named
//! for those sources. A written host is never changed: builds of different
//! sources (worktrees, private installs, the installed binary) each get their
//! own, and a build of the same sources reuses it. Editing a development
//! checkout therefore never changes what an installed `rawr` builds.
//!
//! A process leases the host it uses for as long as it runs, through a
//! `.lock-<pid>` file in the host that it holds locked, and pruning skips a
//! leased host without waiting for it (D608.7). Unleased hosts go by their
//! last use, beyond the few most recently used.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

// `DIGEST`, the identity of the Rawr sources this build carries: the
// embedded sources' digest, or for a build of a written host the digest its
// `.digest` stamp names, which is the writer's own; zero for a development
// build that uses its own checkout.
include!(concat!(env!("OUT_DIR"), "/host.rs"));

/// The Rawr repository root holding `rawr` and `rawr-import`.
#[cfg(not(feature = "embedded-host"))]
pub fn root() -> crate::Result<PathBuf> {
    Ok(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")))
}

/// The Rawr repository root holding `rawr` and `rawr-import`.
/// The host is found or written once per process.
#[cfg(feature = "embedded-host")]
pub fn root() -> crate::Result<PathBuf> {
    static ROOT: std::sync::OnceLock<Lease> = std::sync::OnceLock::new();
    if let Some(lease) = ROOT.get() {
        touch(&lease.root);
        return Ok(lease.root.clone());
    }
    let home = match std::env::var_os("CARGO_HOME") {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var_os("HOME").ok_or_else(|| {
            crate::Error::Request {
                reason: "Cargo couldn't find your home directory: neither `CARGO_HOME` nor `HOME` \
                         is set"
                    .into(),
            }
        })?)
        .join(".cargo"),
    };
    let hosts = crate::store::runtime_home(&home).join("hosts");
    let lease = Lease::take(&hosts, &DIGEST, FILES).map_err(|source| crate::Error::Io {
        operation: "write",
        path: hosts.clone(),
        source,
    })?;
    prune(&hosts, &[lease.root.as_path()], HOST_IDLE, HOSTS_KEPT);
    Ok(ROOT.get_or_init(|| lease).root.clone())
}

/// A host this process uses, held locked through its own `.lock-<pid>` file
/// so that no prune removes it while the lease lives.
#[cfg_attr(not(feature = "embedded-host"), allow(dead_code))]
struct Lease {
    root: PathBuf,
    _lock: std::fs::File,
}

#[cfg_attr(not(feature = "embedded-host"), allow(dead_code))]
impl Lease {
    /// Lease the host for `digest` under `hosts`, writing it from `files`
    /// first when it is not there. A prune that retires the host while the
    /// lease is taken leaves the lease file locked by it or gone, and the
    /// second attempt writes the host anew.
    fn take(hosts: &Path, digest: &[u8; 32], files: &[(&str, &[u8])]) -> std::io::Result<Self> {
        for _ in 0..2 {
            let root = materialize(hosts, digest, files)?;
            let path = root.join(format!("{LEASE}{}", std::process::id()));
            let Ok(lock) = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
            else {
                continue;
            };
            // Held now, the lease is seen by any prune that renames the host
            // after this point; one that renamed it before took the file away.
            if lock.try_lock().is_ok() && path.exists() {
                return Ok(Self { root, _lock: lock });
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "another rawr retired this host while it was leased",
        ))
    }
}

/// The prefix of a host's lease files, one per process that uses it.
const LEASE: &str = ".lock-";

/// Every lease file of the host at `root`, locked, or `None` when a process
/// holds one. Never waits for a lock.
fn vacant(root: &Path) -> Option<Vec<std::fs::File>> {
    let mut guards = Vec::new();
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(LEASE) {
            continue;
        }
        let guard = std::fs::File::open(entry.path()).ok()?;
        guard.try_lock().ok()?;
        guards.push(guard);
    }
    Some(guards)
}

/// The digest as lowercase hex.
fn hex(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(64);
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// The host directory for `digest` under `hosts`, written from `files` when
/// it is not there yet. A host is built in a staging directory beside it and
/// renamed into place with its `.digest` stamp, so a host directory is always
/// complete; one that is present is only marked used.
#[cfg_attr(not(feature = "embedded-host"), allow(dead_code))]
fn materialize(
    hosts: &Path,
    digest: &[u8; 32],
    files: &[(&str, &[u8])],
) -> std::io::Result<PathBuf> {
    use std::fs;

    let digest = hex(digest);
    let root = hosts.join(&digest[..16]);
    let stamp = root.join(".digest");
    if fs::read(&stamp).ok().as_deref() != Some(digest.as_bytes()) {
        fs::create_dir_all(hosts)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(hosts.join(".lock"))?;
        let mut lock = fd_lock::RwLock::new(lock);
        let _guard = lock.write()?;
        if fs::read(&stamp).ok().as_deref() != Some(digest.as_bytes()) {
            if root.exists() {
                // Not a host this code wrote (no matching stamp): set it aside.
                let aside =
                    hosts.join(format!(".retired-{}-{}", &digest[..16], std::process::id()));
                fs::rename(&root, &aside)?;
                let _ = fs::remove_dir_all(&aside);
            }
            let staging = hosts.join(format!(".staging-{}-{}", &digest[..16], std::process::id()));
            let _ = fs::remove_dir_all(&staging);
            for (relative, bytes) in files {
                let path = staging.join(relative);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&path, bytes)?;
            }
            fs::create_dir_all(&staging)?;
            fs::write(staging.join(".digest"), digest.as_bytes())?;
            fs::rename(&staging, &root)?;
        }
    }
    touch(&root);
    Ok(root)
}

/// Mark a host used now; pruning keeps recently used hosts.
fn touch(root: &Path) {
    let _ = std::fs::File::open(root).and_then(|dir| dir.set_modified(SystemTime::now()));
}

/// How long a host must sit unused before it may be removed.
const HOST_IDLE: Duration = Duration::from_secs(60 * 60);

/// How many recently used hosts are kept besides the ones in use.
const HOSTS_KEPT: usize = 3;

/// `rawr cache clean`: retire idle hosts beyond the most recently used ones,
/// idle compiled runtimes the same way, idle shared bindings and inventories
/// and the protocols no installed rawr uses, idle nightly check build
/// directories and drivers built from other sources (everything else a
/// project derives lives in its own target directory).
pub fn clean(cargo_home: &Path) {
    prune(
        &crate::store::runtime_home(cargo_home).join("hosts"),
        &[],
        HOST_IDLE,
        HOSTS_KEPT,
    );
    crate::runner::runtime::clean(cargo_home);
    crate::deps::clean_nightly(&crate::store::home(cargo_home));
    #[cfg(feature = "rustc-driver")]
    crate::bindings::clean(cargo_home);
    #[cfg(feature = "rustc-driver")]
    crate::cargo::toolchain::clean(&crate::store::home(cargo_home));
}

/// Best effort: remove host directories other than `keep` that no process
/// leases and that are both unused for `idle` and older than the `kept` most
/// recently used ones. Each is first renamed aside, so a partial removal is
/// never mistaken for a live host and is finished by a later launch; a lease
/// taken before the rename sends it back. Staging directories left by an
/// interrupted write go once idle.
fn prune(hosts: &Path, keep: &[&Path], idle: Duration, kept: usize) {
    let Ok(entries) = std::fs::read_dir(hosts) else {
        return;
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with(".retired-") {
            if let Some(_guards) = vacant(&path) {
                let _ = std::fs::remove_dir_all(&path);
            }
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) || keep.contains(&path.as_path()) {
            continue;
        }
        let used = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or_else(|_| SystemTime::now());
        let idle = used.elapsed().is_ok_and(|age| age > idle);
        if name.starts_with(".staging-") {
            if idle {
                let _ = std::fs::remove_dir_all(&path);
            }
            continue;
        }
        if name.starts_with('.') {
            continue;
        }
        candidates.push((used, idle, name, path));
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    for (_, idle, name, path) in candidates.into_iter().skip(kept) {
        if !idle || vacant(&path).is_none() {
            continue;
        }
        let retired = hosts.join(format!(".retired-{name}"));
        if std::fs::rename(&path, &retired).is_err() {
            continue;
        }
        match vacant(&retired) {
            Some(_guards) => {
                let _ = std::fs::remove_dir_all(&retired);
            }
            // Leased since the first look: put it back. Should a new copy
            // have taken its place, the retired one goes once unleased.
            None => {
                let _ = std::fs::rename(&retired, &path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn scratch(name: &str) -> PathBuf {
        let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn hosts_of_different_sources_coexist_and_are_reused() {
        let hosts = scratch("hosts").join("home/hosts");
        let first = super::materialize(&hosts, &[1; 32], &[("rawr/src/lib.rs", b"one")]).unwrap();
        let second = super::materialize(&hosts, &[2; 32], &[("rawr/src/lib.rs", b"two")]).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(first.join("rawr/src/lib.rs")).unwrap(), b"one");
        assert_eq!(fs::read(second.join("rawr/src/lib.rs")).unwrap(), b"two");
        // The same sources find their host; it is never rewritten.
        let written = fs::metadata(first.join("rawr/src/lib.rs"))
            .unwrap()
            .modified()
            .unwrap();
        let again = super::materialize(&hosts, &[1; 32], &[("rawr/src/lib.rs", b"other")]).unwrap();
        assert_eq!(again, first);
        assert_eq!(fs::read(first.join("rawr/src/lib.rs")).unwrap(), b"one");
        assert_eq!(
            fs::metadata(first.join("rawr/src/lib.rs"))
                .unwrap()
                .modified()
                .unwrap(),
            written
        );
        let mut left = fs::read_dir(&hosts)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| !name.starts_with('.'))
            .collect::<Vec<_>>();
        left.sort();
        assert_eq!(
            left,
            [
                super::hex(&[1; 32])[..16].to_owned(),
                super::hex(&[2; 32])[..16].to_owned()
            ]
        );
        fs::remove_dir_all(hosts.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn prune_keeps_hosts_in_use_and_recently_used_and_removes_the_rest() {
        let hosts = scratch("prune");
        let now = SystemTime::now();
        let ages = [
            ("current", 9 * super::HOST_IDLE),
            ("in-use", 8 * super::HOST_IDLE),
            ("old1", 2 * super::HOST_IDLE),
            ("old2", 3 * super::HOST_IDLE),
            ("old3", 4 * super::HOST_IDLE),
            ("running", super::HOST_IDLE / 2),
            ("leased", 10 * super::HOST_IDLE),
            (".staging-a-1", 2 * super::HOST_IDLE),
            (".staging-b-2", super::HOST_IDLE / 2),
        ];
        for (name, age) in ages {
            fs::create_dir_all(hosts.join(name).join("rawr")).unwrap();
            fs::File::open(hosts.join(name))
                .and_then(|dir| dir.set_modified(now - age))
                .unwrap();
        }
        fs::create_dir_all(hosts.join(".retired-partial/rawr")).unwrap();
        fs::create_dir_all(hosts.join(".retired-held/rawr")).unwrap();
        // A process that leased "leased" hours ago and one whose host was
        // retired while it ran; a lease file whose process ended is free.
        let lease = |root: PathBuf, pid: u32| {
            let lock = fs::File::create(root.join(format!("{}{pid}", super::LEASE))).unwrap();
            lock.try_lock().unwrap();
            lock
        };
        let _leased = lease(hosts.join("leased"), 1);
        let _held = lease(hosts.join(".retired-held"), 2);
        drop(lease(hosts.join("old2"), 3));
        // Recreating the lease files moved the times; set them back.
        for (name, age) in [
            ("leased", 10 * super::HOST_IDLE),
            ("old2", 3 * super::HOST_IDLE),
        ] {
            fs::File::open(hosts.join(name))
                .and_then(|dir| dir.set_modified(now - age))
                .unwrap();
        }
        let in_use = hosts.join("in-use");
        super::prune(&hosts, &[in_use.as_path()], super::HOST_IDLE, 2);
        let mut left = fs::read_dir(&hosts)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        left.sort();
        // The two most recently used ("running", "old1") stay, as do the host in
        // use, a host a live process leases however old, a retired host still
        // leased and a staging directory still being written; the legacy
        // `current` host and an old host whose lessee ended go once idle.
        assert_eq!(
            left,
            [
                ".retired-held",
                ".staging-b-2",
                "in-use",
                "leased",
                "old1",
                "running"
            ]
        );
        fs::remove_dir_all(hosts).unwrap();
    }

    #[test]
    fn a_leased_host_survives_a_prune_that_keeps_nothing() {
        let hosts = scratch("lease").join("hosts");
        let files: &[(&str, &[u8])] = &[("rawr/src/lib.rs", b"one")];
        let lease = super::Lease::take(&hosts, &[3; 32], files).unwrap();
        let old = SystemTime::now() - 2 * super::HOST_IDLE;
        fs::File::open(&lease.root)
            .and_then(|dir| dir.set_modified(old))
            .unwrap();
        super::prune(&hosts, &[], super::HOST_IDLE, 0);
        assert_eq!(
            fs::read(lease.root.join("rawr/src/lib.rs")).unwrap(),
            b"one"
        );
        let root = lease.root.clone();
        drop(lease);
        fs::File::open(&root)
            .and_then(|dir| dir.set_modified(old))
            .unwrap();
        super::prune(&hosts, &[], super::HOST_IDLE, 0);
        assert!(!root.exists(), "an unleased idle host goes");
        fs::remove_dir_all(hosts.parent().unwrap()).unwrap();
    }
}
