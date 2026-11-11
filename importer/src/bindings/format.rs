//! Each extraction protocol's entries, in a directory of their own:
//! `formats/<protocol>`, under rawr's home for the shared bindings and
//! inventories, and under a project's `target/rawr` for what the project
//! keeps of its own (its bindings, staged records, inventories, extraction
//! tables and API memo).
//!
//! Every such key covers the extraction protocol, so an entry of another
//! protocol is one this rawr never reaches again. Keeping each protocol's
//! entries apart lets them go together. A process that opens its protocol's
//! directory leases it while it runs and records itself among the
//! directory's users, as a compiled runtime records the rawr executables
//! that link it (#963). Another protocol's directory goes when this rawr
//! first opens the directory above it in a process, once no installed rawr
//! is among its users: no timer decides it (#1677). What an older rawr kept
//! straight under the home or `target/rawr`, before each protocol had its
//! own directory, goes then too. A leased directory or entry stays, and
//! nothing here waits for a lock (D608.7). Path tables never come here
//! (D608.4).

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::extract::engine::home;
use crate::runner::runtime;

/// Where the formats live, under rawr's home or `target/rawr`.
const DIRECTORY: &str = "formats";
const BINDINGS: &str = "bindings";
const INVENTORIES: &str = "inventories";
const LOCK: &str = ".lock";
/// The file digests rawr's home kept before they moved to each project's
/// target directory; nothing reads it.
const DIGESTS: &str = "digests";
/// An entry of the current format unused this long goes.
pub(crate) const ENTRY_IDLE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// `rawr cache clean` removes entries and formats unused this long.
const CLEAN_IDLE: Duration = Duration::from_secs(60 * 60);

/// What a project keeps of one protocol, in its format under `target/rawr`.
#[derive(Clone, Copy)]
pub enum Kept {
    /// The settled bindings of its path crates.
    Bindings,
    /// The records of the tables extraction made before they settle.
    Staged,
    /// Its crates' intrinsic inventory entries, keyed by their witnesses.
    Inventories,
    /// The tables the nightly compiler wrote, which staged records name.
    Extraction,
    /// The API content each capture's semantic inputs produced.
    Memo,
}

impl Kept {
    const ALL: [Self; 5] = [
        Self::Bindings,
        Self::Staged,
        Self::Inventories,
        Self::Extraction,
        Self::Memo,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Bindings => BINDINGS,
            Self::Staged => "staged",
            Self::Inventories => INVENTORIES,
            Self::Extraction => "extraction",
            Self::Memo => "api-memo",
        }
    }
}

/// The directory a format serves.
#[derive(Clone, Copy)]
enum Base {
    /// Rawr's home, shared by every project on the machine.
    Home,
    /// A project's Rawr directory, `target/rawr`.
    Project,
}

impl Base {
    /// What an older rawr kept straight under the base, before formats.
    fn legacy(self) -> Vec<&'static str> {
        match self {
            Self::Home => vec![BINDINGS, INVENTORIES],
            Self::Project => Kept::ALL.iter().map(|kept| kept.name()).collect(),
        }
    }
}

/// The shared bindings of this rawr's protocol under `home`.
pub(crate) fn bindings(home: &Path) -> PathBuf {
    current(home, Base::Home).join(BINDINGS)
}

/// The shared inventories of this rawr's protocol under `home`.
pub(crate) fn inventories(home: &Path) -> PathBuf {
    current(home, Base::Home).join(INVENTORIES)
}

/// Where the project whose Rawr directory is `rawr` keeps `kept` for this
/// rawr's protocol.
#[must_use]
pub fn project(rawr: &Path, kept: Kept) -> PathBuf {
    current(rawr, Base::Project).join(kept.name())
}

/// This rawr's format under `directory`. The first open in a process
/// leases it, records this executable among its users, removes the home's
/// stale digests file and retires, in the background, what no installed
/// rawr reaches.
fn current(directory: &Path, base: Base) -> PathBuf {
    static OPENED: Mutex<Vec<(PathBuf, Option<File>)>> = Mutex::new(Vec::new());
    let root = root(directory, crate::extract::protocol::SCHEMA);
    let mut opened = OPENED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if opened.iter().any(|(path, _)| *path == root) {
        return root;
    }
    let lease = lease(&root);
    if matches!(base, Base::Home) {
        let _ = fs::remove_file(directory.join(DIGESTS));
    }
    let (directory, keep) = (directory.to_path_buf(), root.clone());
    let _ = std::thread::Builder::new()
        .name("rawr-format-cleanup".into())
        .spawn(move || retire(&directory, &keep, base));
    opened.push((root.clone(), lease));
    root
}

/// Protocol `protocol`'s format under `directory`.
fn root(directory: &Path, protocol: u32) -> PathBuf {
    directory.join(DIRECTORY).join(protocol.to_string())
}

/// Lease the format at `root`, record this executable among its users and
/// mark the use. A format another process is retiring this moment gives no
/// lease, and its entries are misses for this run. One retired between
/// this open and the lock took the lock file away, and the second attempt
/// makes the format anew.
fn lease(root: &Path) -> Option<File> {
    let path = root.join(LOCK);
    for _ in 0..2 {
        fs::create_dir_all(root).ok()?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .ok()?;
        lock.try_lock_shared().ok()?;
        // Held now, the lease is seen by any cleanup that would rename the
        // format after this point.
        if !path.exists() {
            continue;
        }
        if let Ok(executable) = std::env::current_exe() {
            runtime::users::Users::of(root).enroll(&executable);
        }
        let _ = File::open(root).and_then(|directory| directory.set_modified(SystemTime::now()));
        return Some(lock);
    }
    None
}

/// Best effort: retire the formats under `directory` other than `keep`
/// that no installed rawr uses and no process leases, and what an older
/// rawr kept straight under `directory`. In the home, entries of `keep`'s
/// inventories unused for [`ENTRY_IDLE`] go too (its bindings go as each
/// settlement publishes).
fn retire(directory: &Path, keep: &Path, base: Base) {
    runtime::prune(&directory.join(DIRECTORY), &[keep], Duration::ZERO);
    legacy(directory, &base.legacy());
    if matches!(base, Base::Home) {
        home::prune(&keep.join(INVENTORIES), ENTRY_IDLE);
    }
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
    legacy(home, &Base::Home.legacy());
}

/// Retire each of `names` under `directory`, the stores an older rawr kept
/// there before formats, which no key of this rawr reaches. Each goes whole
/// once no reader or writer holds one of its entries' guards. It is first
/// renamed aside among the formats, where the next cleanup finishes an
/// interrupted removal.
fn legacy(directory: &Path, names: &[&str]) {
    let formats = directory.join(DIRECTORY);
    for name in names {
        let path = directory.join(name);
        let Some(_guards) = vacant(&path) else {
            continue;
        };
        let retired = formats.join(format!(".retired-{name}-{}", std::process::id()));
        if fs::create_dir_all(&formats).is_ok() && fs::rename(&path, &retired).is_ok() {
            let _ = fs::remove_dir_all(&retired);
        }
    }
}

/// Every entry guard of the store at `directory`, held, or `None` when the
/// store is not there or a reader or writer holds one. Never waits.
fn vacant(directory: &Path) -> Option<Vec<File>> {
    let mut guards = Vec::new();
    for entry in fs::read_dir(directory).ok()?.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(LOCK) {
            continue;
        }
        let guard = File::open(entry.path()).ok()?;
        guard.try_lock().ok()?;
        guards.push(guard);
    }
    Some(guards)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::{Base, Kept, BINDINGS, DIGESTS, INVENTORIES, LOCK};
    use crate::extract::engine::home;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

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
        used(&root, hours);
        root
    }

    /// Mark `directory` last used `hours` ago.
    fn used(directory: &Path, hours: u64) {
        let used = SystemTime::now() - Duration::from_secs(hours * 60 * 60);
        fs::File::open(directory)
            .and_then(|directory| directory.set_modified(used))
            .unwrap();
    }

    /// An executable at `path` holding `bytes`.
    fn executable(path: &Path, bytes: &[u8]) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        path.to_path_buf()
    }

    /// A development build of rawr under `root`: Cargo's output in a profile
    /// directory, which an older rawr recorded among `format`'s users.
    fn development(root: &Path, format: &Path) -> PathBuf {
        let profile = root.join("target/debug");
        fs::create_dir_all(profile.join(".fingerprint")).unwrap();
        let built = executable(&profile.join("rawr-import"), b"dev");
        let metadata = fs::metadata(&built).unwrap();
        let modified = metadata
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap();
        fs::write(
            format.join("users"),
            format!(
                "{} {} {}\n",
                metadata.len(),
                modified.as_nanos(),
                built.display()
            ),
        )
        .unwrap();
        built
    }

    fn formats(home: &Path) -> Vec<String> {
        let mut names = fs::read_dir(home.join(super::DIRECTORY))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn names(directory: &Path) -> Vec<String> {
        let mut names = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| !name.starts_with('.'))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    /// Bindings and inventories of this protocol share one directory, which
    /// is leased while the process runs. This test binary is a Cargo build's
    /// output, so it is not recorded among the users. Opening the home
    /// removes its stale digests file.
    #[test]
    fn a_format_directory_holds_both_stores_and_is_leased() {
        let home = scratch("open");
        fs::write(home.join(DIGESTS), b"stale").unwrap();
        let root = super::root(&home, crate::extract::protocol::SCHEMA);
        assert_eq!(super::bindings(&home), root.join(BINDINGS));
        assert_eq!(super::inventories(&home), root.join(INVENTORIES));
        assert!(!home.join(DIGESTS).exists(), "the digests file is removed");
        assert!(!root.join("users").exists(), "a test build is no user");
        // The lease keeps cleanup away while this process runs.
        let lock = fs::File::open(root.join(LOCK)).unwrap();
        assert!(lock.try_lock().is_err(), "the format is leased");
        // A second open in the process is the same directory.
        assert_eq!(super::bindings(&home), root.join(BINDINGS));
        drop(lock);
        let _ = fs::remove_dir_all(home);
    }

    /// A project keeps each store of this protocol in its own format under
    /// `target/rawr`, leased as the home's is, and its digests stay.
    #[test]
    fn a_project_keeps_its_protocols_stores_in_its_format() {
        let rawr = scratch("project").join("target/rawr");
        fs::create_dir_all(&rawr).unwrap();
        fs::write(rawr.join(DIGESTS), b"live").unwrap();
        let root = super::root(&rawr, crate::extract::protocol::SCHEMA);
        for (kept, name) in [
            (Kept::Bindings, "bindings"),
            (Kept::Staged, "staged"),
            (Kept::Inventories, "inventories"),
            (Kept::Extraction, "extraction"),
            (Kept::Memo, "api-memo"),
        ] {
            assert_eq!(super::project(&rawr, kept), root.join(name));
        }
        assert!(
            rawr.join(DIGESTS).is_file(),
            "a project's digests are its own"
        );
        let lock = fs::File::open(root.join(LOCK)).unwrap();
        assert!(lock.try_lock().is_err(), "the format is leased");
        drop(lock);
        let _ = fs::remove_dir_all(rawr.parent().unwrap().parent().unwrap());
    }

    /// After a protocol bump, the first open leaves only the current format
    /// and the formats an installed rawr uses. Another format goes however
    /// recently it was used, as does one whose only users are a development
    /// build left on disk or an executable a reinstall replaced. What an
    /// older rawr kept straight under the home goes whole (#1677).
    #[test]
    fn after_a_protocol_bump_the_first_open_leaves_only_the_current_format() {
        if !home::alone(
            "bindings::format::tests::after_a_protocol_bump_the_first_open_leaves_only_the_current_format",
        ) {
            return;
        }
        let home = scratch("bump");
        let replaced = executable(&home.join("old/rawr"), b"old");
        let current = format(&home, 73, 0, &[]);
        for protocol in 62..=68 {
            let format = format(&home, protocol, 1, &[]);
            development(&home, &format);
        }
        format(&home, 71, 0, &[replaced.as_path()]);
        format(&home, 72, 0, &[]);
        // A reinstall replaces the executable that used format 71.
        executable(&replaced, b"reinstalled from other sources");
        fs::File::options()
            .write(true)
            .open(&replaced)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        // The protocol-less layout, its entries recent.
        for name in [BINDINGS, INVENTORIES] {
            home::publish(&home.join(name), "old", &[("table.json", b"{}")]).unwrap();
        }
        assert_eq!(formats(&home).len(), 10);

        super::retire(&home, &current, Base::Home);
        assert_eq!(formats(&home), ["73"], "only the current format is left");
        assert!(current.join(BINDINGS).join("entry/table.json").is_file());
        assert!(current.join(INVENTORIES).join("entry/table.json").is_file());
        assert!(
            !home.join(BINDINGS).exists(),
            "the protocol-less bindings go"
        );
        assert!(!home.join(INVENTORIES).exists());
        fs::remove_dir_all(home).unwrap();
    }

    /// A project directory with entries of an old protocol, in an old format
    /// and in the layout before formats, keeps only the current format's
    /// after one check. Its other directories (the store, the runner, the
    /// parse) are no format's and stay.
    #[test]
    fn a_project_keeps_only_the_current_formats_entries_after_one_check() {
        if !home::alone(
            "bindings::format::tests::a_project_keeps_only_the_current_formats_entries_after_one_check",
        ) {
            return;
        }
        let rawr = scratch("project-bump").join("target/rawr");
        let current = format(&rawr, 73, 0, &[]);
        let old = format(&rawr, 68, 0, &[]);
        development(rawr.parent().unwrap().parent().unwrap(), &old);
        for kept in Kept::ALL {
            home::publish(&rawr.join(kept.name()), "old", &[("entry.json", b"{}")]).unwrap();
        }
        fs::write(rawr.join("inventories/union-00.json"), b"[]").unwrap();
        fs::write(rawr.join("inventories/applied"), b"").unwrap();
        for kept in ["store", "runner", "parse", "lowered"] {
            fs::create_dir_all(rawr.join(kept)).unwrap();
        }
        fs::write(rawr.join(DIGESTS), b"live").unwrap();
        let before = names(&rawr);
        assert_eq!(before.len(), 11, "{before:?}");

        super::retire(&rawr, &current, Base::Project);
        assert_eq!(formats(&rawr), ["73"]);
        assert_eq!(
            names(&rawr),
            ["digests", "formats", "lowered", "parse", "runner", "store"],
            "only what no format holds is left beside the current format"
        );
        assert!(current.join(BINDINGS).join("entry/table.json").is_file());
        fs::remove_dir_all(rawr.parent().unwrap().parent().unwrap()).unwrap();
    }

    /// A leased format, and a store of the layout before formats one of
    /// whose entries a reader holds, survive the open, without a wait
    /// (D608.7). Once released, the next open takes them.
    #[test]
    fn a_leased_format_or_entry_survives() {
        if !home::alone("bindings::format::tests::a_leased_format_or_entry_survives") {
            return;
        }
        let rawr = scratch("leased").join("target/rawr");
        let current = format(&rawr, 73, 0, &[]);
        let leased = format(&rawr, 72, 0, &[]);
        let lease = fs::File::open(leased.join(LOCK)).unwrap();
        lease.try_lock_shared().unwrap();
        home::publish(&rawr.join("inventories"), "held", &[("entry.json", b"{}")]).unwrap();
        home::publish(&rawr.join("inventories"), "free", &[("entry.json", b"{}")]).unwrap();
        home::publish(&rawr.join("staged"), "free", &[("meta.json", b"{}")]).unwrap();
        let held = home::hold(&rawr.join("inventories"), "held").unwrap();

        super::retire(&rawr, &current, Base::Project);
        assert_eq!(formats(&rawr), ["72", "73"], "the leased format stays");
        assert!(leased.join(INVENTORIES).join("entry/table.json").is_file());
        assert!(
            rawr.join("inventories/held/entry.json").is_file(),
            "a held entry's store stays"
        );
        assert!(rawr.join("inventories/free/entry.json").is_file());
        assert!(!rawr.join("staged").exists(), "a store nobody holds goes");

        drop((lease, held));
        super::retire(&rawr, &current, Base::Project);
        assert_eq!(formats(&rawr), ["73"]);
        assert!(!rawr.join("inventories").exists());
        fs::remove_dir_all(rawr.parent().unwrap().parent().unwrap()).unwrap();
    }

    /// A format that an installed rawr uses stays however long it has gone
    /// unused: that rawr reaches its entries on its next run.
    #[test]
    fn a_format_a_registered_rawr_uses_stays() {
        if !home::alone("bindings::format::tests::a_format_a_registered_rawr_uses_stays") {
            return;
        }
        let home = scratch("registered");
        let installed = executable(&home.join("bin/rawr"), b"installed");
        let current = format(&home, 60, 1, &[]);
        let used = format(&home, 59, 24 * 90, &[installed.as_path()]);
        super::retire(&home, &current, Base::Home);
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
    /// entry unused for an hour goes and a recent one stays, in the current
    /// format. The protocol-less layout goes whole.
    #[test]
    fn cache_clean_reaches_inventories() {
        if !home::alone("bindings::format::tests::cache_clean_reaches_inventories") {
            return;
        }
        let home = scratch("clean");
        let installed = executable(&home.join("bin/rawr"), b"installed");
        let current = format(&home, 60, 0, &[installed.as_path()]);
        let inventories = current.join(INVENTORIES);
        home::publish(&inventories, "recent", &[("entry.json", b"{}")]).unwrap();
        fs::File::open(inventories.join("entry"))
            .and_then(|entry| entry.set_modified(SystemTime::now() - 2 * DAY))
            .unwrap();
        let legacy = home.join(INVENTORIES);
        home::publish(&legacy, "old", &[("entry.json", b"{}")]).unwrap();
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
