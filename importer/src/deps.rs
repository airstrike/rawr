//! The project's Cargo build directory, the one `cargo build` uses
//! (`build.build-dir`, else the target directory). Runner and resolution
//! builds compile into it, so a unit `cargo build` compiled is reused, and
//! the reverse. Rawr removes only its own units there.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

mod named;
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) mod nightly;
pub(crate) mod project;

/// The project's Cargo build directory, created if absent.
pub(crate) fn directory(build: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(build)?;
    Ok(build.to_path_buf())
}

/// Held by every runner build in the project across its Cargo run and any
/// read of what Cargo wrote, so no other Rawr build changes the shared files
/// in between. It lives in `<target>/rawr`, which `cargo clean` removes with
/// the rest. The nightly build has its own ([`nightly_lock`]).
pub(crate) fn lock(rawr: &Path) -> io::Result<File> {
    fs::create_dir_all(rawr)?;
    let file = File::create(rawr.join("build.lock"))?;
    file.lock()?;
    Ok(file)
}

/// The pinned nightly's check build directory, which feeds extraction: one
/// per machine per pinned toolchain, in rawr's `home` (D700). Cargo alone
/// judges what in it is fresh. A path crate's units are keyed there by its
/// absolute path, since the harness's workspace root never contains it, so
/// a same-named crate of another project never stands in for it.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn nightly_directory(home: &Path) -> PathBuf {
    home.join(NIGHTLY)
        .join(crate::cargo::toolchain::active().name())
}

const NIGHTLY: &str = "nightly";

/// What Cargo prints while another build holds its build directory. A wait
/// for the nightly directory says the same, and nothing else (D664).
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
const BLOCKING: &str = "    Blocking waiting for file lock on build directory";

/// Lock the nightly `directory` for one Cargo run and the copy of what it
/// wrote, so no other project's build rewrites those files in between and
/// cleanup skips the directory. A wait says what Cargo says of its own.
/// Records the directory's last use, which cleanup orders by.
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn nightly_lock(directory: &Path) -> io::Result<File> {
    use std::os::unix::fs::MetadataExt as _;
    let path = directory.join(".lock");
    loop {
        fs::create_dir_all(directory)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                crate::progress::say(BLOCKING);
                file.lock()?;
            }
            Err(fs::TryLockError::Error(error)) => return Err(error),
        }
        // Cleanup renames an unlocked directory aside before removing it; a
        // lock taken on the retired one is retaken on a new directory.
        let held = file.metadata()?.ino();
        if fs::metadata(&path).is_ok_and(|on_disk| on_disk.ino() == held) {
            let _ = File::open(directory)
                .and_then(|entry| entry.set_modified(std::time::SystemTime::now()));
            return Ok(file);
        }
    }
}

/// Retire, in the background, the nightly directories of other pins unused
/// for a day. It never waits for a lock or reads free disk (D608.7).
#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
pub(crate) fn retire_nightly(home: &Path, current: &Path) {
    let (root, current) = (home.join(NIGHTLY), current.to_path_buf());
    let _ = std::thread::Builder::new()
        .name("rawr-nightly-cleanup".into())
        .spawn(move || crate::runner::runtime::prune(&root, &[current.as_path()], DAY));
}

/// `rawr cache clean`: remove every nightly directory unused for an hour,
/// the current pin's included, unless a build holds it.
pub fn clean_nightly(home: &Path) {
    crate::runner::runtime::prune(&home.join(NIGHTLY), &[], HOUR);
}

#[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
const DAY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
const HOUR: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// After a successful build in `shared`, remove Rawr's own crates left by
/// other Rawr installs: units whose dep-info names sources outside the Rawr
/// root this build used. `used` is the build's Cargo JSON or dep-info, which
/// names the `rawr` crate's `src/lib.rs`. Third-party crates and per-project
/// generated crates are kept. Best effort; the caller holds the build lock.
pub(crate) fn prune_rawr(shared: &Path, used: &[u8]) {
    let contains = |needle: &str| {
        used.windows(needle.len())
            .any(|window| window == needle.as_bytes())
    };
    let (profiles, units) = rawr_units(shared);
    let Some(root) = units
        .iter()
        .find(|(krate, _, source)| krate == "rawr" && contains(&source.to_string_lossy()))
        .and_then(|(_, _, source)| source.parent()?.parent())
        .map(Path::to_path_buf)
    else {
        return;
    };
    let stale = units
        .iter()
        .filter(|(_, _, source)| !source.starts_with(&root))
        .map(|(_, hash, _)| hash.as_str())
        .collect::<Vec<_>>();
    remove_units(&profiles, &stale);
}

/// Remove Rawr's own units in `shared` whose sources lie under any of
/// `sources`: the copies of rawr a runner compiled in the project before it
/// linked rawr's shared runtime (D620). A crate the project itself takes
/// from a registry is kept. Best effort; the caller holds the build lock.
pub(crate) fn prune_rawr_from(shared: &Path, sources: &[PathBuf]) {
    let (profiles, units) = rawr_units(shared);
    let stale = units
        .iter()
        .filter(|(_, _, source)| sources.iter().any(|root| source.starts_with(root)))
        .map(|(_, hash, _)| hash.as_str())
        .collect::<Vec<_>>();
    remove_units(&profiles, &stale);
}

/// The profile directories in `shared` and Rawr's own units in them: each
/// unit's crate, hash and first source file, from `deps/<crate>-<hash>.d`.
#[allow(clippy::type_complexity)]
fn rawr_units(shared: &Path) -> (Vec<PathBuf>, Vec<(String, String, PathBuf)>) {
    // The runner's crates and its members' (`bridge::generate::member`) are
    // the project's, wherever rawr's sources are.
    let own = |name: &str| {
        name == "rawr"
            || (name.starts_with("rawr_")
                && !name.starts_with("rawr_generated_runner")
                && name != "rawr_resolution_harness")
    };
    let profiles = profiles(shared);
    let mut units = Vec::new();
    for profile in &profiles {
        for entry in fs::read_dir(profile.join("deps"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some((krate, hash)) = name
                .strip_suffix(".d")
                .and_then(|stem| stem.rsplit_once('-'))
                .filter(|(krate, _)| own(krate))
            else {
                continue;
            };
            // Only Rawr's own units' dep-info is read (#1848).
            let source = fs::read_to_string(entry.path()).ok().and_then(|text| {
                let (_, sources) = text.lines().next()?.split_once(": ")?;
                sources.split(' ').next().map(PathBuf::from)
            });
            if let Some(source) = source {
                units.push((krate.to_owned(), hash.to_owned(), source));
            }
        }
    }
    (profiles, units)
}

/// Remove the Rawr units with the `stale` hashes from each profile.
fn remove_units(profiles: &[PathBuf], stale: &[&str]) {
    if stale.is_empty() {
        return;
    }
    for profile in profiles {
        for directory in ["deps", ".fingerprint", "incremental"] {
            for entry in fs::read_dir(profile.join(directory))
                .into_iter()
                .flatten()
                .flatten()
            {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.contains("rawr") && stale.iter().any(|hash| name.contains(hash)) {
                    let path = entry.path();
                    let _ = if path.is_dir() {
                        fs::remove_dir_all(&path)
                    } else {
                        fs::remove_file(&path)
                    };
                }
            }
        }
    }
}

/// The `<crate>-<16 hex>` unit key a Cargo output name starts with
/// (`libserde-0123456789abcdef.rlib`, `serde-0123….d`, `serde-0123…`), with
/// the crate part's dashes as underscores.
fn unit_key(name: &str) -> Option<(String, &str)> {
    // Libraries carry `lib`; dep-info, objects and binaries do not (`libc`'s
    // dep-info is `libc-<hash>.d`, its rlib `liblibc-<hash>.rlib`).
    let library = [".rlib", ".rmeta", ".dylib", ".so", ".a"]
        .iter()
        .any(|extension| name.ends_with(extension));
    let name = if library {
        name.strip_prefix("lib").unwrap_or(name)
    } else {
        name
    };
    let (krate, hash) = unit_key_at(name)?;
    Some((krate.replace('-', "_"), hash))
}

fn unit_key_at(name: &str) -> Option<(&str, &str)> {
    let mut search = 0;
    while let Some(offset) = name[search..].find('-') {
        let dash = search + offset;
        let hash = name.get(dash + 1..dash + 17)?;
        let ends = name[dash + 17..]
            .chars()
            .next()
            .is_none_or(|next| next == '.');
        if hash.bytes().all(|byte| byte.is_ascii_hexdigit()) && ends && dash > 0 {
            return Some((&name[..dash], hash));
        }
        search = dash + 1;
    }
    None
}

/// The object files a 64-bit Mach-O executable's debug map (`N_OSO` stabs)
/// names; `None` for any other format, which keeps every object.
fn debug_map_objects(binary: &Path) -> Option<std::collections::HashSet<String>> {
    let bytes = fs::read(binary).ok()?;
    let u32_at = |offset: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(offset..offset + 4)?.try_into().ok()?,
        ))
    };
    if u32_at(0)? != 0xfeed_facf {
        return None;
    }
    let commands = usize::try_from(u32_at(16)?).ok()?;
    let mut offset = 32;
    let mut symtab = None;
    for _ in 0..commands {
        let (command, size) = (u32_at(offset)?, usize::try_from(u32_at(offset + 4)?).ok()?);
        if command == 0x2 {
            symtab = Some((
                usize::try_from(u32_at(offset + 8)?).ok()?,
                usize::try_from(u32_at(offset + 12)?).ok()?,
                usize::try_from(u32_at(offset + 16)?).ok()?,
            ));
        }
        offset = offset.checked_add(size.max(8))?;
    }
    let (symbols, count, strings) = symtab?;
    let mut objects = std::collections::HashSet::new();
    for index in 0..count {
        let entry = symbols.checked_add(index.checked_mul(16)?)?;
        if *bytes.get(entry + 4)? != 0x66 {
            continue;
        }
        let start = strings.checked_add(usize::try_from(u32_at(entry)?).ok()?)?;
        let length = bytes.get(start..)?.iter().position(|byte| *byte == 0)?;
        let name = std::str::from_utf8(&bytes[start..start + length]).ok()?;
        // Named by file: the build directory may be spelled differently.
        if let Some(file) = Path::new(name).file_name().and_then(|file| file.to_str()) {
            objects.insert(file.to_owned());
        }
    }
    Some(objects)
}

/// The profile directories of the build directory `shared`:
/// `<shared>/<profile>` and `<shared>/<target>/<profile>`.
fn profiles(shared: &Path) -> Vec<PathBuf> {
    let mut profiles = Vec::new();
    for child in fs::read_dir(shared).into_iter().flatten().flatten() {
        let child = child.path();
        if child.join(".fingerprint").is_dir() {
            profiles.push(child.clone());
        }
        for grandchild in fs::read_dir(&child).into_iter().flatten().flatten() {
            if grandchild.path().join(".fingerprint").is_dir() {
                profiles.push(grandchild.path());
            }
        }
    }
    profiles
}

/// The directory rawr generates its packages in, and where it resolves to.
/// A path in it is rawr's unless it resolves outside, as a project's path
/// package reached through one of the runner's links does (#1777): that
/// package's units are the ones the project's own `cargo build` makes.
pub(crate) struct Generated<'a> {
    directory: &'a Path,
    resolved: PathBuf,
}

impl<'a> Generated<'a> {
    pub(crate) fn new(directory: &'a Path) -> Self {
        Self {
            directory,
            resolved: fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf()),
        }
    }

    /// Whether `path` lies in the directory and resolves there. A path that
    /// no longer resolves, as a removed generated file, is judged by where
    /// it is.
    pub(crate) fn holds(&self, path: &Path) -> bool {
        path.starts_with(self.directory)
            && fs::canonicalize(path).map_or(true, |path| path.starts_with(&self.resolved))
    }
}

/// The member names of the archive at `path`, an rlib: BSD names
/// (`#1/<length>`, the name after the header) or GNU ones (`/<offset>` into
/// the `//` table). `None` when it is not an archive.
fn archive_members(path: &Path) -> Option<std::collections::HashSet<String>> {
    let bytes = fs::read(path).ok()?;
    if !bytes.starts_with(b"!<arch>\n") {
        return None;
    }
    let text = |range: std::ops::Range<usize>| -> Option<&str> {
        std::str::from_utf8(bytes.get(range)?).ok()
    };
    let mut members = std::collections::HashSet::new();
    let mut table = None;
    let mut offset = 8;
    while let Some(header) = bytes.get(offset..offset + 60) {
        if &header[58..60] != b"`\n" {
            break;
        }
        let id = text(offset..offset + 16)?.trim_end();
        let length: usize = text(offset + 48..offset + 58)?.trim().parse().ok()?;
        let body = offset + 60;
        if let Some(size) = id.strip_prefix("#1/") {
            let size: usize = size.parse().ok()?;
            members.insert(text(body..body + size)?.trim_end_matches('\0').to_owned());
        } else if id == "//" {
            table = Some(body..body + length);
        } else if let Some(start) = id
            .strip_prefix('/')
            .and_then(|start| start.parse::<usize>().ok())
        {
            let names = text(table.clone()?)?;
            let name = names.get(start..)?;
            members.insert(name[..name.find("/\n")?].to_owned());
        } else {
            members.insert(id.trim_end_matches('/').to_owned());
        }
        offset = body + length + length % 2;
    }
    Some(members)
}

/// The package name of a Cargo package id: `path+file:///a#name@0.1.0`,
/// `path+file:///a/name#0.1.0` or `name 0.1.0 (path+file:///a)`.
fn package_name(id: &str) -> Option<&str> {
    let Some((url, fragment)) = id.split_once('#') else {
        return id.split_once(' ').map(|(name, _)| name);
    };
    match fragment.split_once('@') {
        Some((name, _)) => Some(name),
        None => url.rsplit('/').next(),
    }
}

/// Update `record`, rawr's incremental directories in a project's build
/// directory, from a runner build. A directory is `<crate>-<stable crate
/// id>`, which no message names, so a project crate of the same name has one
/// of the same shape; only a recorded one is rawr's. Each directory of a
/// crate in `rebuilt` that the build touched is recorded and kept. A crate
/// keeps at most one directory for each of its units this build named
/// (`current`), the touched ones first and then its newest recorded ones;
/// the rest go. Each kept one keeps only its last session.
fn prune_incremental(
    profiles: &[PathBuf],
    current: &BTreeMap<&str, usize>,
    rebuilt: &BTreeSet<&str>,
    record: &Path,
    before: &dyn Fn(&Path) -> bool,
) {
    let krate = |path: &Path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.rsplit_once('-'))
            .map(|(krate, _)| krate.to_owned())
    };
    let mut kept = BTreeMap::<String, Vec<PathBuf>>::new();
    for profile in profiles {
        for entry in fs::read_dir(profile.join("incremental"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if let Some(name) = krate(&path).filter(|name| rebuilt.contains(name.as_str()))
                && !before(&path)
            {
                kept.entry(name).or_default().push(path);
            }
        }
    }
    let mut untouched = Vec::new();
    for recorded in fs::read_to_string(record).unwrap_or_default().lines() {
        let recorded = PathBuf::from(recorded);
        let touched = kept.values().flatten().any(|path| *path == recorded);
        if !touched && profiles.iter().any(|profile| recorded.starts_with(profile)) {
            let time = fs::metadata(&recorded)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            untouched.push((time, recorded));
        }
    }
    untouched.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    for (_, recorded) in untouched {
        let Some(name) = krate(&recorded) else {
            continue;
        };
        let room = current.get(name.as_str()).copied().unwrap_or(0);
        let directories = kept.entry(name).or_default();
        if directories.len() < room {
            directories.push(recorded);
        } else if before(&recorded) {
            let _ = fs::remove_dir_all(&recorded);
        }
    }
    let mut text = String::new();
    for path in kept.values().flatten() {
        prune_sessions(path);
        text.push_str(&path.to_string_lossy());
        text.push('\n');
    }
    let _ = fs::write(record, text);
}

/// Remove every session in the incremental directory `directory` but the
/// last one written, with their `.lock`s: each compile writes a new
/// `s-<time>-<id>[-<svh>]` session and rustc may leave the earlier ones
/// (#1782). The caller holds the build directory, so no compile is
/// writing one.
fn prune_sessions(directory: &Path) {
    let mut sessions: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("s-") && !name.ends_with(".lock"))
        })
        .map(|path| {
            let time = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (time, path)
        })
        .collect();
    sessions.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    // A session's lock is `s-<time>-<id>.lock`.
    let lock = |path: &Path| {
        let name = path.file_name()?.to_str()?;
        let stem: Vec<&str> = name.splitn(4, '-').take(3).collect();
        Some(path.with_file_name(format!("{}.lock", stem.join("-"))))
    };
    let newest = sessions.first().and_then(|(_, path)| lock(path));
    for (_, path) in sessions.iter().skip(1) {
        let _ = fs::remove_dir_all(path);
        if let Some(stale) = lock(path).filter(|stale| Some(stale) != newest.as_ref()) {
            let _ = fs::remove_file(stale);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn the_build_directory_is_the_projects_and_the_lock_is_rawrs() {
        let root = std::env::temp_dir().join(format!("rawr-project-build-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (target, rawr) = (root.join("target"), root.join("target/rawr"));
        assert_eq!(super::directory(&target).unwrap(), target);
        let held = super::lock(&rawr).unwrap();
        assert!(rawr.join("build.lock").is_file());
        drop(held);
        fs::remove_dir_all(root).unwrap();
    }

    /// The nightly check build's directory is one per pin in rawr's home.
    /// Cleanup reaches it by last use: another pin's goes after a day, `rawr
    /// cache clean` takes any idle for an hour, and a held one stays, with
    /// no wait (D608.7, D700).
    #[test]
    fn nightly_directories_go_by_last_use_and_a_held_one_stays() {
        use std::time::{Duration, SystemTime};
        let home = std::env::temp_dir().join(format!("rawr-nightly-home-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let current = super::nightly_directory(&home);
        assert_eq!(
            current,
            home.join("nightly").join(crate::cargo::toolchain::CHANNEL)
        );
        let root = home.join("nightly");
        let age = |name: &str, hours: u64| {
            let directory = root.join(name);
            drop(super::nightly_lock(&directory).unwrap());
            let used = SystemTime::now() - Duration::from_secs(hours * 60 * 60);
            fs::File::open(&directory)
                .unwrap()
                .set_modified(used)
                .unwrap();
        };
        age(crate::cargo::toolchain::CHANNEL, 30);
        age("nightly-old", 30);
        age("nightly-held", 30);
        age("nightly-recent", 2);
        let held = super::nightly_lock(&root.join("nightly-held")).unwrap();
        let left = || {
            let mut names = fs::read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            names.sort();
            names
        };
        // Each build: other pins idle for a day, the current one kept.
        crate::runner::runtime::prune(&root, &[current.as_path()], super::DAY);
        assert_eq!(
            left(),
            [
                crate::cargo::toolchain::CHANNEL,
                "nightly-held",
                "nightly-recent"
            ]
        );
        // `rawr cache clean`: anything idle for an hour unless held.
        super::clean_nightly(&home);
        assert_eq!(left(), ["nightly-held"]);
        // Taking the lock was a use; once it is an hour old and released,
        // the directory goes too.
        drop(held);
        super::clean_nightly(&home);
        assert_eq!(left(), ["nightly-held"]);
        age("nightly-held", 2);
        super::clean_nightly(&home);
        assert!(left().is_empty());
        fs::remove_dir_all(home).unwrap();
    }

    /// A build that finds the nightly directory held waits for it, and the
    /// directory it then holds is the live one.
    #[test]
    fn a_nightly_lock_waits_for_the_holder() {
        let home = std::env::temp_dir().join(format!("rawr-nightly-wait-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let directory = super::nightly_directory(&home);
        let held = super::nightly_lock(&directory).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let waiting = directory.clone();
        let waiter = std::thread::spawn(move || {
            let lock = super::nightly_lock(&waiting).unwrap();
            sender.send(()).unwrap();
            drop(lock);
        });
        assert!(receiver
            .recv_timeout(std::time::Duration::from_millis(300))
            .is_err());
        drop(held);
        receiver.recv().unwrap();
        waiter.join().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn prune_removes_only_rawr_units_of_another_install() {
        let project = crate::store::TestProject::new("rawr-deps-prune");
        let shared = project.path().join("shared");
        let (current, old) = (
            project.path().join("hosts/a"),
            project.path().join("hosts/b"),
        );
        let mut kept = Vec::new();
        let mut removed = Vec::new();
        for (profile, krate, hash, root) in [
            ("debug", "rawr", "1111111111111111", &current),
            ("debug", "rawr_iced", "2222222222222222", &current),
            (
                "aarch64-apple-darwin/debug",
                "rawr",
                "3333333333333333",
                &old,
            ),
            (
                "aarch64-apple-darwin/debug",
                "rawr_import",
                "4444444444444444",
                &old,
            ),
            (
                "aarch64-apple-darwin/debug",
                "serde",
                "5555555555555555",
                &old,
            ),
            ("debug", "rawr_resolution_harness", "6666666666666666", &old),
        ] {
            let profile = shared.join(profile);
            let deps = profile.join("deps");
            let fingerprint = profile
                .join(".fingerprint")
                .join(format!("{}-{hash}", krate.replace('_', "-")));
            fs::create_dir_all(&deps).unwrap();
            fs::create_dir_all(&fingerprint).unwrap();
            let rlib = deps.join(format!("lib{krate}-{hash}.rlib"));
            fs::write(&rlib, b"").unwrap();
            let source = root.join("src/lib.rs");
            fs::write(
                deps.join(format!("{krate}-{hash}.d")),
                format!("{}: {}\n", rlib.display(), source.display()),
            )
            .unwrap();
            let stale = root == &old && krate != "serde" && krate != "rawr_resolution_harness";
            if stale {
                removed.extend([rlib, fingerprint]);
            } else {
                kept.extend([rlib, fingerprint]);
            }
        }
        // This build used the current install's `rawr`.
        let used = format!(
            r#"{{"target":{{"src_path":"{}"}}}}"#,
            current.join("src/lib.rs").display()
        );
        super::prune_rawr(&shared, used.as_bytes());
        assert!(kept.iter().all(|path| path.exists()), "{kept:?}");
        assert!(removed.iter().all(|path| !path.exists()), "{removed:?}");
    }
}
