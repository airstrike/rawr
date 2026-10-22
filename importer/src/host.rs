//! The Rawr source tree a generated harness builds against.
//!
//! An installed `rawr` carries the library sources a harness compiles and
//! writes them, read-only by convention, to `$CARGO_HOME/rawr/hosts/current`,
//! rewriting only changed files. Editing a development checkout
//! therefore never changes what an installed `rawr` builds, and a new `rawr`
//! addresses new cache entries while Cargo rebuilds only the units it changed.

use std::path::PathBuf;

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
#[cfg(feature = "embedded-host")]
pub fn root() -> crate::Result<PathBuf> {
    use std::fmt::Write as _;
    use std::fs;

    let io = |operation, path: &std::path::Path| {
        let path = path.to_path_buf();
        move |source| crate::Error::Io {
            operation,
            path,
            source,
        }
    };
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
    let mut digest = String::with_capacity(64);
    for byte in DIGEST {
        let _ = write!(digest, "{byte:02x}");
    }
    let hosts = crate::store::home(&home).join("hosts");
    // One stable path. Cargo keys a path dependency's units by its path,
    // so a new host replaces the previous host's units in place (rebuilding only
    // what changed) instead of adding a second set; caches are keyed by
    // `DIGEST` through the resolution universe, not by this path.
    let root = hosts.join(CURRENT);
    let stamp = root.join(".digest");
    if fs::read(&stamp).ok().as_deref() != Some(digest.as_bytes()) {
        fs::create_dir_all(&hosts).map_err(io("create", &hosts))?;
        let lock = hosts.join(".lock");
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock)
            .map_err(io("open", &lock))?;
        let mut lock = fd_lock::RwLock::new(lock);
        let _guard = lock.write().map_err(io("lock", &hosts))?;
        if fs::read(&stamp).ok().as_deref() != Some(digest.as_bytes()) {
            sync(&root, &stamp, &digest).map_err(io("write", &root))?;
        }
    }
    // Mark this host in use, then retire hosts of earlier layouts. Another
    // rawr may still be running from one, so only idle ones go.
    let _ = fs::File::open(&root).and_then(|dir| dir.set_modified(std::time::SystemTime::now()));
    prune(&hosts, CURRENT, HOST_IDLE);
    Ok(root)
}

/// The one host directory every install writes.
const CURRENT: &str = "current";

/// Bring `root` to exactly `FILES`, rewriting only files whose bytes differ so
/// unchanged ones keep their modification time and Cargo's fingerprints. The
/// stamp is removed first and written last, so an interrupted sync is redone.
#[cfg(feature = "embedded-host")]
fn sync(root: &std::path::Path, stamp: &std::path::Path, digest: &str) -> std::io::Result<()> {
    use std::collections::BTreeSet;
    use std::fs;

    match fs::remove_file(stamp) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut wanted = BTreeSet::new();
    for (relative, bytes) in FILES {
        let path = root.join(relative);
        wanted.insert(path.clone());
        if fs::read(&path).ok().as_deref() == Some(*bytes) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write(&path, bytes)?;
    }
    remove_unwanted(root, &wanted)?;
    write(stamp, digest.as_bytes())
}

/// Write by rename, so a concurrent reader sees the old or the new bytes.
#[cfg(feature = "embedded-host")]
fn write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut staging = path.as_os_str().to_owned();
    staging.push(format!(".{}.tmp", std::process::id()));
    std::fs::write(&staging, bytes)?;
    std::fs::rename(&staging, path)
}

/// Remove files under `directory` that are not `wanted` (the stamp aside), and
/// directories left empty.
#[cfg(feature = "embedded-host")]
fn remove_unwanted(
    directory: &std::path::Path,
    wanted: &std::collections::BTreeSet<PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            remove_unwanted(&path, wanted)?;
            let _ = std::fs::remove_dir(&path);
        } else if !wanted.contains(&path) && entry.file_name() != ".digest" {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// How long another install's host must sit unused before it is removed.
const HOST_IDLE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// `rawr cache clean`: retire other installs' idle hosts, the Rawr-wide cache
/// under `$CARGO_HOME/rawr` (everything a project derives lives in its
/// own target directory).
pub fn clean(cargo_home: &std::path::Path) {
    prune(
        &crate::store::home(cargo_home).join("hosts"),
        CURRENT,
        HOST_IDLE,
    );
}

/// Best effort: remove folders (hosts, shared build directories) other than
/// `current` unused for `idle`. Each is first renamed aside, so a partial
/// removal is never mistaken for a live folder and is finished by a later launch.
pub(crate) fn prune(hosts: &std::path::Path, current: &str, idle: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(hosts) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let mut path = entry.path();
        if !name.starts_with(".retired-") {
            let idle = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age > idle));
            if name == current || name.starts_with('.') || !idle {
                continue;
            }
            let retired = hosts.join(format!(".retired-{name}"));
            if std::fs::rename(&path, &retired).is_err() {
                continue;
            }
            path = retired;
        }
        let _ = std::fs::remove_dir_all(&path);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::SystemTime;

    #[test]
    fn prune_keeps_the_current_and_recent_hosts_and_removes_the_rest() {
        let hosts = std::env::temp_dir().join(format!("rawr-hosts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&hosts);
        let now = SystemTime::now();
        let ages = [
            ("current", 2 * super::HOST_IDLE),
            ("old", 2 * super::HOST_IDLE),
            ("running", super::HOST_IDLE / 2),
            (".staging.1", 2 * super::HOST_IDLE),
        ];
        for (name, age) in ages {
            fs::create_dir_all(hosts.join(name).join("rawr")).unwrap();
            fs::File::open(hosts.join(name))
                .and_then(|dir| dir.set_modified(now - age))
                .unwrap();
        }
        fs::create_dir_all(hosts.join(".retired-partial/rawr")).unwrap();
        super::prune(&hosts, "current", super::HOST_IDLE);
        let mut left = fs::read_dir(&hosts)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        left.sort();
        assert_eq!(left, [".staging.1", "current", "running"]);
        fs::remove_dir_all(hosts).unwrap();
    }
}
