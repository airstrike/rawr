//! The Rawr source tree a generated harness builds against.
//!
//! An installed `rawr` carries the library sources a harness compiles and
//! writes them once to `$CARGO_HOME/rawr/hosts/<digest>`, a directory named
//! for those sources. A written host is never changed: builds of different
//! sources (worktrees, private installs, the installed binary) each get their
//! own, and a build of the same sources reuses it. Editing a development
//! checkout therefore never changes what an installed `rawr` builds.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[cfg(feature = "embedded-host")]
include!(concat!(env!("OUT_DIR"), "/host.rs"));

/// Identity of the Rawr sources this build carries; zero for a development
/// build that uses its own checkout.
#[cfg(not(feature = "embedded-host"))]
pub const DIGEST: [u8; 32] = [0; 32];

/// The Rawr repository root holding `rawr`, `rawr-import` and `rawr-iced`.
#[cfg(not(feature = "embedded-host"))]
pub fn root() -> crate::Result<PathBuf> {
    Ok(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")))
}

/// The Rawr repository root holding `rawr`, `rawr-import` and `rawr-iced`.
/// The host is found or written once per process.
#[cfg(feature = "embedded-host")]
pub fn root() -> crate::Result<PathBuf> {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    if let Some(root) = ROOT.get() {
        touch(root);
        return Ok(root.clone());
    }
    let home = match std::env::var_os("CARGO_HOME") {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(
            std::env::var_os("HOME").ok_or_else(|| crate::Error::Request {
                field: "Rawr host",
                reason: "requires CARGO_HOME or HOME".into(),
            })?,
        )
        .join(".cargo"),
    };
    let hosts = crate::store::home(&home).join("hosts");
    let root = materialize(&hosts, &DIGEST, FILES).map_err(|source| crate::Error::Io {
        operation: "write",
        path: hosts.clone(),
        source,
    })?;
    prune(&hosts, &[root.as_path()], HOST_IDLE, HOSTS_KEPT);
    Ok(ROOT.get_or_init(|| root).clone())
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

/// `rawr cache clean`: retire idle hosts beyond the most recently used ones;
/// they are the only Rawr-wide cache under `$CARGO_HOME/rawr` (everything a
/// project derives lives in its own target directory).
pub fn clean(cargo_home: &Path) {
    prune(
        &crate::store::home(cargo_home).join("hosts"),
        &[],
        HOST_IDLE,
        HOSTS_KEPT,
    );
}

/// Best effort: remove host directories other than `keep` that are both
/// unused for `idle` and older than the `kept` most recently used ones. Each
/// is first renamed aside, so a partial removal is never mistaken for a live
/// host and is finished by a later launch. Staging directories left by an
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
            let _ = std::fs::remove_dir_all(&path);
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
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, idle, name, path) in candidates.into_iter().skip(kept) {
        if !idle {
            continue;
        }
        let retired = hosts.join(format!(".retired-{name}"));
        if std::fs::rename(&path, &retired).is_ok() {
            let _ = std::fs::remove_dir_all(&retired);
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
        let in_use = hosts.join("in-use");
        super::prune(&hosts, &[in_use.as_path()], super::HOST_IDLE, 2);
        let mut left = fs::read_dir(&hosts)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        left.sort();
        // The two most recently used ("running", "old1") stay, as do the host in
        // use and a staging directory still being written; the legacy
        // `current` host goes once idle like any other.
        assert_eq!(left, [".staging-b-2", "in-use", "old1", "running"]);
        fs::remove_dir_all(hosts).unwrap();
    }
}
