//! The project's Cargo build directory, the one `cargo build` uses
//! (`build.build-dir`, else the target directory). Runner and resolution
//! builds compile into it, so a unit `cargo build` compiled is reused, and
//! the reverse. Rawr removes only its own units there.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// The project's Cargo build directory, created if absent.
pub(crate) fn directory(build: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(build)?;
    Ok(build.to_path_buf())
}

/// Held by every rawr build across its Cargo run and any read of what Cargo
/// wrote, so no other Rawr build changes the shared files in between. It
/// lives in `<target>/rawr`, which `cargo clean` removes with the rest.
pub(crate) fn lock(rawr: &Path) -> io::Result<File> {
    fs::create_dir_all(rawr)?;
    let file = File::create(rawr.join("build.lock"))?;
    file.lock()?;
    Ok(file)
}

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
    let own = |name: &str| {
        name == "rawr"
            || (name.starts_with("rawr_")
                && !matches!(name, "rawr_generated_runner" | "rawr_resolution_harness"))
    };
    // Profile directories: `<shared>/<profile>` and `<shared>/<target>/<profile>`.
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
            else {
                continue;
            };
            let source = fs::read_to_string(entry.path()).ok().and_then(|text| {
                let (_, sources) = text.lines().next()?.split_once(": ")?;
                sources.split(' ').next().map(PathBuf::from)
            });
            if let (true, Some(source)) = (own(krate), source) {
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

/// The runner's unit hashes in the profiles' deps directories: those whose
/// dep-info names a file in `sources`, the runner's source directory. The
/// runner takes the script crate's name (D594), and so does the project's own
/// binary in the same build directory, so the name alone does not tell them
/// apart.
fn runner_hashes(profiles: &[PathBuf], runner: &str, sources: &str) -> BTreeSet<String> {
    let mut hashes = BTreeSet::new();
    for profile in profiles {
        for entry in fs::read_dir(profile.join("deps"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let file = entry.file_name();
            let Some(hash) = file
                .to_str()
                .and_then(|file| file.strip_suffix(".d"))
                .and_then(|stem| stem.strip_prefix(runner))
                .and_then(|rest| rest.strip_prefix('-'))
                .filter(|hash| {
                    hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            else {
                continue;
            };
            let named = fs::read_to_string(entry.path()).is_ok_and(|text| {
                text.lines()
                    .next()
                    .is_some_and(|line| line.contains(sources))
            });
            if named {
                hashes.insert(hash.to_owned());
            }
        }
    }
    hashes
}

/// The newest linked runner binary among the runner's unit `hashes`, and its
/// hash.
fn current_runner(
    profiles: &[PathBuf],
    runner: &str,
    hashes: &BTreeSet<String>,
) -> Option<(String, PathBuf)> {
    let mut newest: Option<(std::time::SystemTime, String, PathBuf)> = None;
    for profile in profiles {
        for hash in hashes {
            let binary = profile
                .join("deps")
                .join(format!("{runner}-{hash}{}", std::env::consts::EXE_SUFFIX));
            let Ok(modified) = fs::metadata(&binary).and_then(|metadata| metadata.modified())
            else {
                continue;
            };
            if newest.as_ref().is_none_or(|(time, _, _)| modified > *time) {
                newest = Some((modified, hash.clone(), binary));
            }
        }
    }
    newest.map(|(_, hash, path)| (hash, path))
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

/// After a successful runner build in `shared` that began at `started`, keep
/// what this build's Cargo messages (JSON) name and remove what they supersede:
/// a unit of a crate this build used under another hash (an earlier
/// install's `rawr_iced`) that no other workspace building in `shared` last
/// used, the runner's own outputs this build did not write (each incremental
/// build leaves a new set of codegen objects beside the binary), and
/// incremental state of the runner's this build did not touch. `work` is the
/// runner's workspace, where `incremental-directories` lists the runner's
/// incremental directories, since the project's own binary may share the
/// runner's crate name. Crates this build did not use are kept, and so is
/// every crate that is not Rawr's, since `cargo build` shares the directory.
/// Best effort; the caller holds the build lock.
pub(crate) fn prune_superseded(
    shared: &Path,
    messages: &[u8],
    started: std::time::SystemTime,
    work: &Path,
) {
    let record = &work.join("incremental-directories");
    let mut live = std::collections::BTreeSet::new();
    let mut crates = std::collections::BTreeSet::new();
    let mut name = |path: &str| {
        let path = Path::new(path);
        for part in [path.file_name(), path.parent().and_then(Path::file_name)] {
            if let Some((krate, hash)) = part.and_then(|part| part.to_str()).and_then(unit_key) {
                crates.insert(krate.clone());
                live.insert(format!("{krate}-{hash}"));
            }
        }
    };
    let mut rebuilt = false;
    // The runner's binary target, named as the script's crate when its
    // proxies stand for script types (D594).
    let mut runner = String::from("rawr_generated_runner");
    // The directory of the runner's sources, which its dep-info names.
    let mut sources = None;
    for line in messages.split(|byte| *byte == b'\n') {
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        let is_runner = (message
            .get("package_id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|package| package.contains("rawr-generated-runner"))
            || message
                .pointer("/target/name")
                .and_then(serde_json::Value::as_str)
                == Some("rawr-generated-runner"))
            && message
                .pointer("/target/kind/0")
                .and_then(serde_json::Value::as_str)
                == Some("bin");
        if is_runner
            && let Some(name) = message
                .pointer("/target/name")
                .and_then(serde_json::Value::as_str)
        {
            runner = name.replace('-', "_");
            sources = message
                .pointer("/target/src_path")
                .and_then(serde_json::Value::as_str)
                .and_then(|path| Path::new(path).parent())
                .map(|directory| directory.to_string_lossy().into_owned());
        }
        rebuilt |=
            message.get("fresh").and_then(serde_json::Value::as_bool) == Some(false) && is_runner;
        for file in message
            .get("filenames")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .chain(message.get("executable"))
            .chain(message.get("out_dir"))
            .filter_map(serde_json::Value::as_str)
        {
            name(file);
        }
    }
    if live.is_empty() {
        return;
    }
    let others = shared_units(shared, work, &live);
    let runner = runner.as_str();
    let before = |path: &Path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|time| time < started)
    };
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
    let hashes = sources
        .as_deref()
        .map(|sources| runner_hashes(&profiles, runner, sources))
        .unwrap_or_default();
    // The runner this build linked, and the object files its debug map names
    // (macOS keeps debug info in them, by path): a rebuilt runner's earlier
    // objects, and every output of its other unit hashes, are superseded.
    // Without a Mach-O debug map (an ELF runner on Linux) the other hashes
    // still go, and every object of the current one stays.
    let linked = rebuilt
        .then(|| current_runner(&profiles, runner, &hashes))
        .flatten()
        .map(|(hash, binary)| (hash, debug_map_objects(&binary)));
    if rebuilt {
        prune_incremental(&profiles, runner, record, &before);
    }
    for profile in &profiles {
        for directory in ["deps", ".fingerprint", "build"] {
            for entry in fs::read_dir(profile.join(directory))
                .into_iter()
                .flatten()
                .flatten()
            {
                let file = entry.file_name();
                let Some(file) = file.to_str() else { continue };
                let stale = if let Some((krate, hash)) = unit_key(file) {
                    // The build directory is the project's: only Rawr's own
                    // crates are Rawr's to remove. Another workspace's unit,
                    // or one a build that is still running wrote, stays.
                    let key = format!("{krate}-{hash}");
                    let superseded = owned(&krate)
                        && crates.contains(&krate)
                        && !live.contains(&key)
                        && !others.contains(&key)
                        && before(&entry.path());
                    if krate == runner {
                        // A binary unit's messages name its uplifted copy, not
                        // its hash. A unit of this name that is not the
                        // runner's is the project's binary.
                        directory == "deps"
                            && hashes.contains(hash)
                            && linked.as_ref().is_some_and(|(current, objects)| {
                                hash != current
                                    || (file.ends_with(".rcgu.o")
                                        && objects
                                            .as_ref()
                                            .is_some_and(|objects| !objects.contains(file)))
                            })
                    } else {
                        superseded
                    }
                } else {
                    false
                };
                if stale {
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

/// Whether `krate` is one of Rawr's crates, whose units are Rawr's to remove.
fn owned(krate: &str) -> bool {
    krate == "rawr" || krate.starts_with("rawr_")
}

/// Record the Rawr units in `live` as the ones the workspace `work` last used
/// in `shared`, and return those every other workspace there last recorded.
/// Workspaces share a build directory (every script builds in rawr's, and
/// projects may share a `CARGO_TARGET_DIR`), and another profile or feature
/// set gives Rawr's crates another hash: such a unit is another workspace's,
/// not superseded. The record of a workspace that is gone goes too.
fn shared_units(shared: &Path, work: &Path, live: &BTreeSet<String>) -> BTreeSet<String> {
    use sha2::{Digest as _, Sha256};
    let records = shared.join("rawr-units");
    let digest = Sha256::digest(work.as_os_str().as_encoded_bytes());
    let name = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut others = BTreeSet::new();
    for entry in fs::read_dir(&records).into_iter().flatten().flatten() {
        if entry.file_name().to_str() == Some(name.as_str()) {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let mut lines = text.lines();
        if lines
            .next()
            .is_some_and(|workspace| Path::new(workspace).join("Cargo.toml").is_file())
        {
            others.extend(lines.map(str::to_owned));
        } else {
            let _ = fs::remove_file(entry.path());
        }
    }
    let mut text = format!("{}\n", work.to_string_lossy());
    for key in live {
        if key.rsplit_once('-').is_some_and(|(krate, _)| owned(krate)) {
            text.push_str(key);
            text.push('\n');
        }
    }
    let _ = fs::create_dir_all(&records).and_then(|()| fs::write(records.join(name), text));
    others
}

/// Remove the runner's incremental directories that `record` lists and this
/// rebuilt runner did not touch, then record the ones it did. A directory is
/// `<crate>-<stable crate id>`, so the project's own binary of the runner's
/// crate name has one of the same shape; only a recorded one is the runner's.
fn prune_incremental(
    profiles: &[PathBuf],
    runner: &str,
    record: &Path,
    before: &dyn Fn(&Path) -> bool,
) {
    let prefix = format!("{runner}-");
    let mut current = Vec::new();
    for profile in profiles {
        for entry in fs::read_dir(profile.join("incremental"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with(&prefix) && !before(&path) {
                current.push(path);
            }
        }
    }
    for recorded in fs::read_to_string(record).unwrap_or_default().lines() {
        let recorded = Path::new(recorded);
        if !current.iter().any(|path| path == recorded)
            && profiles.iter().any(|profile| recorded.starts_with(profile))
            && before(recorded)
        {
            let _ = fs::remove_dir_all(recorded);
        }
    }
    let mut text = String::new();
    for path in &current {
        text.push_str(&path.to_string_lossy());
        text.push('\n');
    }
    let _ = fs::write(record, text);
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

    #[test]
    fn prune_keeps_what_the_build_named_and_removes_what_it_superseded() {
        let project = crate::store::TestProject::new("rawr-deps-superseded");
        let shared = project.path().join("shared");
        let target = shared.join("aarch64-apple-darwin/debug");
        let host = shared.join("debug");
        for profile in [&target, &host] {
            for directory in ["deps", ".fingerprint", "build", "incremental"] {
                fs::create_dir_all(profile.join(directory)).unwrap();
            }
        }
        let hash = |digit: char| std::iter::repeat_n(digit, 16).collect::<String>();
        let write = |path: std::path::PathBuf| {
            fs::write(&path, b"").unwrap();
            path
        };
        let iced = write(target.join(format!("deps/librawr_iced-{}.rlib", hash('a'))));
        let iced_old = write(target.join(format!("deps/librawr_iced-{}.rlib", hash('b'))));
        let iced_host = write(host.join(format!("deps/librawr_iced-{}.rlib", hash('c'))));
        let serde = write(target.join(format!("deps/libserde-{}.rlib", hash('d'))));
        let serde_other = write(target.join(format!("deps/libserde-{}.rlib", hash('9'))));
        // The runner takes the script crate's name, `app` (D594), and the
        // project's own binary of that name shares the build directory.
        let runner_sources = project.path().join("runner/src");
        let project_binary = write(target.join(format!("deps/app-{}", hash('3'))));
        fs::write(
            target.join(format!("deps/app-{}.d", hash('3'))),
            format!(
                "{}: {}\n",
                project_binary.display(),
                project.path().join("app/src/main.rs").display()
            ),
        )
        .unwrap();
        let project_incremental = target.join("incremental/app-project");
        fs::create_dir_all(&project_incremental).unwrap();
        let derive = write(host.join(format!("deps/libserde_derive-{}.dylib", hash('e'))));
        let harness = write(target.join(format!(
            "deps/librawr_resolution_harness-{}.rlib",
            hash('f')
        )));
        let old_objects = write(target.join(format!("deps/app-{}.cgu.old.rcgu.o", hash('1'))));
        let other_runner = write(host.join(format!("deps/app-{}", hash('2'))));
        for (profile, digit) in [(&target, '1'), (&host, '2')] {
            fs::write(
                profile.join(format!("deps/app-{}.d", hash(digit))),
                format!("app: {}\n", runner_sources.join("main.rs").display()),
            )
            .unwrap();
        }
        let old_incremental = target.join("incremental/app-old");
        fs::create_dir_all(&old_incremental).unwrap();
        let record = project.path().join("incremental-directories");
        fs::write(&record, format!("{}\n", old_incremental.display())).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let started = std::time::SystemTime::now();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let new_objects = write(target.join(format!("deps/app-{}.cgu.new.rcgu.o", hash('1'))));
        // A linked runner whose debug map (one `N_OSO` stab) names the new set.
        let binary = target.join(format!("deps/app-{}", hash('1')));
        let oso = format!("\0{}\0", new_objects.display()).into_bytes();
        let mut macho = Vec::new();
        for word in [0xfeed_facf_u32, 0x0100_000c, 0, 2, 1, 24, 0, 0] {
            macho.extend_from_slice(&word.to_le_bytes());
        }
        let strings = 56 + 16;
        for word in [2_u32, 24, 56, 1, strings, u32::try_from(oso.len()).unwrap()] {
            macho.extend_from_slice(&word.to_le_bytes());
        }
        macho.extend_from_slice(&1_u32.to_le_bytes());
        macho.extend_from_slice(&[0x66, 0, 0, 0]);
        macho.extend_from_slice(&0_u64.to_le_bytes());
        macho.extend_from_slice(&oso);
        fs::write(&binary, macho).unwrap();
        let new_incremental = target.join("incremental/app-new");
        fs::create_dir_all(&new_incremental).unwrap();
        let artifact = |file: &std::path::Path, name: &str, kind: &str, fresh: bool| {
            format!(
                r#"{{"reason":"compiler-artifact","target":{{"name":"{name}","kind":["{kind}"]}},"fresh":{fresh},"filenames":["{}"],"executable":null}}"#,
                file.display()
            )
        };
        let runner_fresh = |fresh: bool| {
            [
                artifact(&iced, "rawr_iced", "lib", true),
                artifact(&serde, "serde", "lib", true),
                artifact(&derive, "serde_derive", "proc-macro", true),
                format!(
                    r#"{{"reason":"compiler-artifact","package_id":"path+file:///runner#rawr-generated-runner@0.0.0","target":{{"name":"app","kind":["bin"],"src_path":"{}"}},"fresh":{fresh},"filenames":["{}"],"executable":null}}"#,
                    runner_sources.join("main.rs").display(),
                    project.path().join("runner/target/app").display()
                ),
            ]
            .join("\n")
        };
        // A fresh runner build keeps the runner's outputs it did not rewrite.
        super::prune_superseded(
            &shared,
            runner_fresh(true).as_bytes(),
            started,
            project.path(),
        );
        assert!(old_objects.exists() && old_incremental.exists() && other_runner.exists());
        assert!(
            !iced_old.exists() && !iced_host.exists(),
            "superseded rawr_iced units"
        );
        super::prune_superseded(
            &shared,
            runner_fresh(false).as_bytes(),
            started,
            project.path(),
        );
        for kept in [
            &iced,
            &serde,
            &serde_other,
            &derive,
            &harness,
            &new_objects,
            &binary,
            &new_incremental,
            &project_incremental,
            &project_binary,
        ] {
            assert!(kept.exists(), "{} is kept", kept.display());
        }
        assert!(
            !old_objects.exists(),
            "the runner's earlier codegen objects"
        );
        assert!(!other_runner.exists(), "another runner unit hash");
        assert!(
            !old_incremental.exists(),
            "incremental state the build did not touch"
        );
        assert_eq!(
            fs::read_to_string(&record).unwrap(),
            format!("{}\n", new_incremental.display())
        );
        assert_eq!(
            super::unit_key("librawr_iced-0123456789abcdef.rlib"),
            Some(("rawr_iced".to_owned(), "0123456789abcdef"))
        );
        assert_eq!(
            super::unit_key("rawr-generated-runner-0123456789abcdef"),
            Some(("rawr_generated_runner".to_owned(), "0123456789abcdef"))
        );
        assert_eq!(
            super::unit_key("libc-0123456789abcdef.d").map(|key| key.0),
            Some("libc".to_owned())
        );
        assert_eq!(
            super::unit_key("liblibc-0123456789abcdef.rlib").map(|key| key.0),
            Some("libc".to_owned())
        );
        assert_eq!(super::unit_key("invoked.timestamp"), None);
    }

    /// Two scripts build in one directory, one under a profile of its own,
    /// so `rawr` has a unit of each hash, and neither prunes the other's.
    #[test]
    fn prune_keeps_the_units_another_workspace_last_used() {
        let project = crate::store::TestProject::new("rawr-deps-workspaces");
        let shared = project.path().join("build");
        let deps = shared.join("debug/deps");
        fs::create_dir_all(&deps).unwrap();
        fs::create_dir_all(shared.join("debug/.fingerprint")).unwrap();
        let (first, second) = (project.path().join("a"), project.path().join("b"));
        for work in [&first, &second] {
            fs::create_dir_all(work).unwrap();
            fs::write(work.join("Cargo.toml"), b"").unwrap();
        }
        let unit = |digit: char| {
            let path = deps.join(format!(
                "librawr-{}.rlib",
                std::iter::repeat_n(digit, 16).collect::<String>()
            ));
            fs::write(&path, b"").unwrap();
            path
        };
        let (a, b, old) = (unit('a'), unit('b'), unit('c'));
        let used = |path: &std::path::Path| {
            format!(
                r#"{{"reason":"compiler-artifact","target":{{"name":"rawr","kind":["lib"]}},"fresh":true,"filenames":["{}"]}}"#,
                path.display()
            )
        };
        let started = || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            std::time::SystemTime::now()
        };
        super::prune_superseded(&shared, used(&a).as_bytes(), started(), &first);
        assert!(a.exists() && !b.exists() && !old.exists());
        let b = unit('b');
        super::prune_superseded(&shared, used(&b).as_bytes(), started(), &second);
        assert!(a.exists() && b.exists(), "each script's unit stays");
        // A workspace that is gone keeps nothing.
        fs::remove_dir_all(&first).unwrap();
        super::prune_superseded(&shared, used(&b).as_bytes(), started(), &second);
        assert!(!a.exists() && b.exists());
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
