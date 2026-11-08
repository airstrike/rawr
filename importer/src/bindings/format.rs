//! Each extraction protocol's shared bindings and inventories, in a
//! directory of their own under rawr's home: `formats/<protocol>`.
//!
//! Every shared key covers the extraction protocol, so an entry of another
//! protocol is one this rawr never reaches again. Keeping each protocol's
//! entries apart lets them go together. A process that opens its protocol's
//! directory leases it while it runs and records itself among the
//! directory's users, as a compiled runtime records the rawr executables
//! that link it (#963). Another protocol's directory goes once it has gone a
//! day unused and no installed rawr is among its users, the rule for other
//! nightly pins (D700). A leased directory or entry stays, and nothing here
//! waits for a lock (D608.7). Path tables never come here (D608.4).

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::extract::engine::home;
use crate::runner::runtime;

/// Where the formats live, under rawr's home.
const DIRECTORY: &str = "formats";
const BINDINGS: &str = "bindings";
const INVENTORIES: &str = "inventories";
const LOCK: &str = ".lock";
/// The file digests rawr's home kept before they moved to each project's
/// target directory; nothing reads it.
const DIGESTS: &str = "digests";
/// Another format unused this long goes, unless an installed rawr uses it.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
/// An entry of the current format unused this long goes.
pub(crate) const ENTRY_IDLE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// `rawr cache clean` removes entries and formats unused this long.
const CLEAN_IDLE: Duration = Duration::from_secs(60 * 60);

/// The shared bindings of this rawr's protocol under `home`.
pub(crate) fn bindings(home: &Path) -> PathBuf {
    current(home).join(BINDINGS)
}

/// The shared inventories of this rawr's protocol under `home`.
pub(crate) fn inventories(home: &Path) -> PathBuf {
    current(home).join(INVENTORIES)
}

/// This rawr's format under `home`. The first open in a process leases it,
/// records this executable among its users, removes the stale digests file
/// and retires, in the background, what no installed rawr reaches.
fn current(home: &Path) -> PathBuf {
    static OPENED: Mutex<Vec<(PathBuf, Option<File>)>> = Mutex::new(Vec::new());
    let root = root(home, crate::extract::protocol::SCHEMA);
    let mut opened = OPENED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if opened.iter().any(|(path, _)| *path == root) {
        return root;
    }
    let lease = lease(&root);
    let _ = fs::remove_file(home.join(DIGESTS));
    let (home, keep) = (home.to_path_buf(), root.clone());
    let _ = std::thread::Builder::new()
        .name("rawr-format-cleanup".into())
        .spawn(move || retire(&home, &keep));
    opened.push((root.clone(), lease));
    root
}

/// Protocol `protocol`'s format under `home`.
fn root(home: &Path, protocol: u32) -> PathBuf {
    home.join(DIRECTORY).join(protocol.to_string())
}

/// Lease the format at `root`, record this executable among its users and
/// mark the use. A format another process is retiring this moment gives no
/// lease; its entries are then misses until a later run.
fn lease(root: &Path) -> Option<File> {
    fs::create_dir_all(root).ok()?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(LOCK))
        .ok()?;
    lock.try_lock_shared().ok()?;
    if let Ok(executable) = std::env::current_exe() {
        runtime::users::Users::of(root).enroll(&executable);
    }
    let _ = File::open(root).and_then(|directory| directory.set_modified(SystemTime::now()));
    Some(lock)
}

/// Best effort: retire the formats other than `keep` that went a day unused
/// and that no installed rawr uses, entries of the protocol-less layout
/// unused for a day, and entries of `keep`'s inventories unused for
/// [`ENTRY_IDLE`] (its bindings go as each settlement publishes).
fn retire(home: &Path, keep: &Path) {
    runtime::prune(&home.join(DIRECTORY), &[keep], DAY);
    legacy(home, DAY);
    home::prune(&keep.join(INVENTORIES), ENTRY_IDLE);
}

/// `rawr cache clean`: remove the entries of every format unused for an
/// hour, then each format unused for an hour that no installed rawr uses.
pub(crate) fn clean(home: &Path) {
    let formats = home.join(DIRECTORY);
    if let Ok(entries) = fs::read_dir(&formats) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            home::prune(&entry.path().join(BINDINGS), CLEAN_IDLE);
            home::prune(&entry.path().join(INVENTORIES), CLEAN_IDLE);
        }
    }
    runtime::prune(&formats, &[], CLEAN_IDLE);
    legacy(home, CLEAN_IDLE);
}

/// The protocol-less layout, `bindings` and `inventories` straight under
/// rawr's home, records no users: each entry goes once unused for `idle`,
/// and a directory left with only its entries' guards goes with them.
fn legacy(home: &Path, idle: Duration) {
    for name in [BINDINGS, INVENTORIES] {
        let directory = home.join(name);
        home::prune(&directory, idle);
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        let mut guards = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let held = name.starts_with(".lock-").then(|| {
                File::open(entry.path())
                    .ok()
                    .filter(|guard| guard.try_lock().is_ok())
            });
            match held.flatten() {
                Some(guard) => guards.push(guard),
                None => break,
            }
        }
        // Every guard is held, so no reader or writer is inside.
        if guards.len() == count(&directory) {
            let _ = fs::remove_dir_all(&directory);
        }
    }
}

fn count(directory: &Path) -> usize {
    fs::read_dir(directory).map_or(usize::MAX, Iterator::count)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    use super::{BINDINGS, DAY, DIGESTS, INVENTORIES, LOCK};
    use crate::extract::engine::home;

    fn scratch(name: &str) -> PathBuf {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-formats-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// A format of `protocol` holding one binding and one inventory, last
    /// used `hours` ago, whose users are `users`.
    fn format(home: &Path, protocol: u32, hours: u64, users: &[&Path]) -> PathBuf {
        let root = super::root(home, protocol);
        for store in [BINDINGS, INVENTORIES] {
            home::publish(&root.join(store), "entry", &[("table.json", b"{}")]).unwrap();
        }
        fs::write(root.join(LOCK), b"").unwrap();
        for user in users {
            crate::runner::runtime::users::Users::of(&root).enroll(user);
        }
        let used = SystemTime::now() - Duration::from_secs(hours * 60 * 60);
        fs::File::open(&root)
            .and_then(|directory| directory.set_modified(used))
            .unwrap();
        root
    }

    fn formats(home: &Path) -> Vec<String> {
        let mut names = fs::read_dir(home.join(super::DIRECTORY))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    /// Bindings and inventories of this protocol share one directory, which
    /// records this executable among its users and is leased while the
    /// process runs. Opening it removes the home's stale digests file.
    #[test]
    fn a_format_directory_holds_both_stores_and_records_its_users() {
        let home = scratch("open");
        fs::write(home.join(DIGESTS), b"stale").unwrap();
        let root = super::root(&home, crate::extract::protocol::SCHEMA);
        assert_eq!(super::bindings(&home), root.join(BINDINGS));
        assert_eq!(super::inventories(&home), root.join(INVENTORIES));
        assert!(!home.join(DIGESTS).exists(), "the digests file is removed");
        let users = fs::read_to_string(root.join("users")).unwrap();
        let executable = std::env::current_exe().unwrap();
        assert_eq!(users.lines().count(), 1);
        assert!(users.ends_with(&format!(" {}\n", executable.display())));
        assert!(crate::runner::runtime::users::Users::of(&root).any_installed());
        // The lease keeps cleanup away while this process runs.
        let lock = fs::File::open(root.join(LOCK)).unwrap();
        assert!(lock.try_lock().is_err(), "the format is leased");
        // A second open in the process is the same directory, unrecorded again.
        assert_eq!(super::bindings(&home), root.join(BINDINGS));
        assert_eq!(fs::read_to_string(root.join("users")).unwrap(), users);
        drop(lock);
        let _ = fs::remove_dir_all(home);
    }

    /// A format no installed rawr uses goes once it has gone a day unused,
    /// with its bindings and inventories. One used within the day, the
    /// current one and a leased one stay, without a wait (D608.7, D700).
    #[test]
    fn a_format_no_installed_rawr_uses_goes_after_a_day() {
        if !home::alone("bindings::format::tests::a_format_no_installed_rawr_uses_goes_after_a_day")
        {
            return;
        }
        let home = scratch("retire");
        let replaced = home.join("old/rawr");
        fs::create_dir_all(replaced.parent().unwrap()).unwrap();
        fs::write(&replaced, b"old").unwrap();
        let current = format(&home, 60, 30, &[]);
        format(&home, 57, 30, &[]);
        format(&home, 58, 30, &[replaced.as_path()]);
        format(&home, 59, 2, &[]);
        let leased = format(&home, 56, 30, &[]);
        // A reinstall replaces the executable that used format 58.
        fs::write(&replaced, b"reinstalled from other sources").unwrap();
        fs::File::options()
            .write(true)
            .open(&replaced)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        let lease = fs::File::open(leased.join(LOCK)).unwrap();
        lease.try_lock_shared().unwrap();
        // The protocol-less layout: an entry unused for two days and one in use.
        let legacy = home.join(BINDINGS);
        home::publish(&legacy, "old", &[("table.json", b"{}")]).unwrap();
        fs::File::open(legacy.join("old"))
            .and_then(|entry| entry.set_modified(SystemTime::now() - 2 * DAY))
            .unwrap();
        home::publish(&home.join(INVENTORIES), "old", &[("entry.json", b"{}")]).unwrap();
        fs::File::open(home.join(INVENTORIES).join("old"))
            .and_then(|entry| entry.set_modified(SystemTime::now() - 2 * DAY))
            .unwrap();
        let held = home::hold(&home.join(INVENTORIES), "old").unwrap();
        fs::File::open(home.join(INVENTORIES).join("old"))
            .and_then(|entry| entry.set_modified(SystemTime::now() - 2 * DAY))
            .unwrap();

        super::retire(&home, &current);
        assert_eq!(formats(&home), ["56", "59", "60"]);
        assert!(!legacy.exists(), "the unused protocol-less bindings go");
        assert!(
            home.join(INVENTORIES).join("old/entry.json").is_file(),
            "a leased protocol-less entry stays"
        );
        assert!(current.join(INVENTORIES).join("entry/table.json").is_file());
        drop((lease, held));
        super::retire(&home, &current);
        assert_eq!(formats(&home), ["59", "60"]);
        assert!(!home.join(INVENTORIES).exists());
        fs::remove_dir_all(home).unwrap();
    }

    /// A format that an installed rawr uses stays however long it has gone
    /// unused: that rawr reaches its entries on its next run.
    #[test]
    fn a_format_a_registered_rawr_uses_stays() {
        if !home::alone("bindings::format::tests::a_format_a_registered_rawr_uses_stays") {
            return;
        }
        let home = scratch("registered");
        let installed = home.join("bin/rawr");
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        fs::write(&installed, b"installed").unwrap();
        let current = format(&home, 60, 1, &[]);
        let used = format(&home, 59, 24 * 90, &[installed.as_path()]);
        super::retire(&home, &current);
        assert_eq!(formats(&home), ["59", "60"]);
        assert!(used.join(BINDINGS).join("entry/table.json").is_file());
        assert!(used.join(INVENTORIES).join("entry/table.json").is_file());
        // `rawr cache clean` keeps it too, but takes its idle entries.
        for store in [BINDINGS, INVENTORIES] {
            fs::File::open(used.join(store).join("entry"))
                .and_then(|entry| entry.set_modified(SystemTime::now() - 2 * DAY))
                .unwrap();
        }
        super::clean(&home);
        assert!(formats(&home).contains(&"59".to_owned()));
        assert!(!used.join(BINDINGS).join("entry").exists());
        assert!(!used.join(INVENTORIES).join("entry").exists());
        fs::remove_dir_all(home).unwrap();
    }

    /// `rawr cache clean` reaches inventories as it reaches bindings: an
    /// entry unused for an hour goes, a recent one stays, in the current
    /// format and in the protocol-less layout.
    #[test]
    fn cache_clean_reaches_inventories() {
        if !home::alone("bindings::format::tests::cache_clean_reaches_inventories") {
            return;
        }
        let home = scratch("clean");
        let installed = home.join("bin/rawr");
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        fs::write(&installed, b"installed").unwrap();
        let current = format(&home, 60, 0, &[installed.as_path()]);
        let inventories = current.join(INVENTORIES);
        home::publish(&inventories, "recent", &[("entry.json", b"{}")]).unwrap();
        fs::File::open(inventories.join("entry"))
            .and_then(|entry| entry.set_modified(SystemTime::now() - 2 * DAY))
            .unwrap();
        let legacy = home.join(INVENTORIES);
        home::publish(&legacy, "old", &[("entry.json", b"{}")]).unwrap();
        fs::File::open(legacy.join("old"))
            .and_then(|entry| entry.set_modified(SystemTime::now() - 2 * DAY))
            .unwrap();
        super::clean(&home);
        assert!(
            !inventories.join("entry").exists(),
            "an idle inventory goes"
        );
        assert!(inventories.join("recent/entry.json").is_file());
        assert!(!legacy.exists(), "the protocol-less inventories go");
        assert_eq!(formats(&home), ["60"]);
        fs::remove_dir_all(home).unwrap();
    }
}
