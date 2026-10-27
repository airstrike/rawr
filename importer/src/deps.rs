//! The project's Cargo build directory, the one `cargo build` uses
//! (`build.build-dir`, else the target directory). Runner and resolution
//! builds compile into it, so a unit `cargo build` compiled is reused, and
//! the reverse. Rawr removes only its own units there.

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
    let own = |name: &str| {
        name == "rawr"
            || (name.starts_with("rawr_")
                && !matches!(name, "rawr_generated_runner" | "rawr_resolution_harness"))
    };
    let contains = |needle: &str| {
        used.windows(needle.len())
            .any(|window| window == needle.as_bytes())
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
    // Each Rawr unit's hash and its first source file, from `deps/<crate>-<hash>.d`.
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
    if stale.is_empty() {
        return;
    }
    for profile in &profiles {
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

/// The newest linked runner binary in `shared`'s deps directories and its
/// unit hash.
fn current_runner(shared: &Path, runner: &str) -> Option<(String, PathBuf)> {
    let mut newest: Option<(std::time::SystemTime, String, PathBuf)> = None;
    for profile in fs::read_dir(shared).into_iter().flatten().flatten() {
        for deps in [
            profile.path().join("deps"),
            profile.path().join("debug/deps"),
        ] {
            for entry in fs::read_dir(&deps).into_iter().flatten().flatten() {
                let file = entry.file_name();
                let Some(file) = file.to_str() else { continue };
                let Some(hash) = file
                    .strip_prefix(runner)
                    .and_then(|rest| rest.strip_prefix('-'))
                    .filter(|hash| {
                        hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                else {
                    continue;
                };
                let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
                    continue;
                };
                if newest.as_ref().is_none_or(|(time, _, _)| modified > *time) {
                    newest = Some((modified, hash.to_owned(), entry.path()));
                }
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
/// install's `rawr_iced`), the
/// runner's own outputs this build did not write (each incremental build
/// leaves a new set of codegen objects beside the binary), and incremental
/// state this build did not touch (only the runner is incremental).
/// Crates this build did not use (the resolution harness) are kept, and so is
/// every crate that is not Rawr's, since `cargo build` shares the directory.
/// Best effort; the caller holds the build lock.
pub(crate) fn prune_superseded(shared: &Path, messages: &[u8], started: std::time::SystemTime) {
    let owned = |krate: &str| krate == "rawr" || krate.starts_with("rawr_");
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
    let runner = runner.as_str();
    // The runner this build linked, and the object files its debug map names
    // (macOS keeps debug info in them, by path): a rebuilt runner's earlier
    // objects, and every output of its other unit hashes, are superseded.
    // Without a Mach-O debug map (an ELF runner on Linux) the other hashes
    // still go, and every object of the current one stays.
    let linked = rebuilt
        .then(|| current_runner(shared, runner))
        .flatten()
        .map(|(hash, binary)| (hash, debug_map_objects(&binary)));
    let before = |entry: &fs::DirEntry| {
        entry
            .metadata()
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
    for profile in &profiles {
        for directory in ["deps", ".fingerprint", "build", "incremental"] {
            for entry in fs::read_dir(profile.join(directory))
                .into_iter()
                .flatten()
                .flatten()
            {
                let file = entry.file_name();
                let Some(file) = file.to_str() else { continue };
                let stale = if directory == "incremental" {
                    // `<crate>-<session base>`: the runner is Rawr's one
                    // incremental unit, and a rebuilt runner's directory took
                    // a new session. The project's own are Cargo's.
                    rebuilt && file.starts_with(runner) && before(&entry)
                } else if let Some((krate, hash)) = unit_key(file) {
                    // The build directory is the project's: only Rawr's own
                    // crates are Rawr's to remove.
                    let superseded = owned(&krate)
                        && crates.contains(&krate)
                        && !live.contains(&format!("{krate}-{hash}"));
                    if krate == runner {
                        // A binary unit's messages name its uplifted copy, not
                        // its hash.
                        directory == "deps"
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
        let project_incremental = target.join("incremental/app-old");
        fs::create_dir_all(&project_incremental).unwrap();
        let derive = write(host.join(format!("deps/libserde_derive-{}.dylib", hash('e'))));
        let harness = write(target.join(format!(
            "deps/librawr_resolution_harness-{}.rlib",
            hash('f')
        )));
        let old_objects = write(target.join(format!(
            "deps/rawr_generated_runner-{}.cgu.old.rcgu.o",
            hash('1')
        )));
        let other_runner = write(host.join(format!("deps/rawr_generated_runner-{}", hash('2'))));
        let old_incremental = target.join("incremental/rawr_generated_runner-old");
        fs::create_dir_all(&old_incremental).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let started = std::time::SystemTime::now();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let new_objects = write(target.join(format!(
            "deps/rawr_generated_runner-{}.cgu.new.rcgu.o",
            hash('1')
        )));
        // A linked runner whose debug map (one `N_OSO` stab) names the new set.
        let binary = target.join(format!("deps/rawr_generated_runner-{}", hash('1')));
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
        let new_incremental = target.join("incremental/rawr_generated_runner-new");
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
                artifact(
                    &project.path().join("runner/target/rawr-generated-runner"),
                    "rawr-generated-runner",
                    "bin",
                    fresh,
                ),
            ]
            .join("\n")
        };
        // A fresh runner build keeps the runner's outputs it did not rewrite.
        super::prune_superseded(&shared, runner_fresh(true).as_bytes(), started);
        assert!(old_objects.exists() && old_incremental.exists() && other_runner.exists());
        assert!(
            !iced_old.exists() && !iced_host.exists(),
            "superseded rawr_iced units"
        );
        super::prune_superseded(&shared, runner_fresh(false).as_bytes(), started);
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
