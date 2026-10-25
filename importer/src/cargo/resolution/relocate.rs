//! A resolution compiles in the toolchain's shared Cargo build directory and
//! keeps, under its bundle's own build root, only what its record and metadata
//! probe read: each unit's metadata (`.rmeta`, or a proc-macro's dylib), build
//! script executables, rustc dep-info and build-script run directories — not
//! rlibs, objects, fingerprints or incremental state. Cargo's messages and the
//! copied dep-info and script outputs name the copies, so the record, the probe
//! and every later replay read sealed bundle files, never the live shared ones.
//!
//! A copy is a clone where the filesystem has them (`fs::copy` is `clonefile`
//! on APFS, a reflink on Linux), so an unchanged unit costs no data blocks in
//! another bundle. Each copy is written frozen and its digest memoized for its
//! identity, taken from the memo of the unchanged shared file: sealing,
//! validation and replay then hash only the units this build changed.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::file_digest::{self, Domain, Identity};
use crate::input::{escape_environment, escape_path};

use crate::{Error, Result};

/// Copy what this build's messages name from `shared` to `bundle` and return the
/// messages naming the copies. The caller holds the shared directory's lock.
pub(in crate::cargo) fn relocate(
    messages: &[u8],
    shared: &Path,
    bundle: &Path,
) -> Result<Box<[u8]>> {
    let (from, to) = (spelled(shared)?, spelled(bundle)?);
    let copy = |path: &str| -> Result<()> {
        let Ok(relative) = Path::new(path).strip_prefix(shared) else {
            return Ok(());
        };
        if relative.starts_with(RAWR) {
            return Ok(());
        }
        let destination = bundle.join(relative);
        copy_tree(Path::new(path), &destination, from, to)?;
        // rustc names dep-info `<stem without lib>.d`; a build script's is
        // `build_script_build-<hash>.d` beside it.
        let Some(name) = destination.file_name().and_then(|name| name.to_str()) else {
            return Ok(());
        };
        let dep_info = if name == "build-script-build" {
            destination
                .parent()
                .and_then(|parent| parent.file_name()?.to_str()?.rsplit_once('-'))
                .map(|(_, hash)| format!("build_script_build-{hash}.d"))
        } else {
            name.strip_prefix("lib")
                .and_then(|stem| stem.rsplit_once('.'))
                .map(|(stem, _)| format!("{stem}.d"))
        };
        if let Some(dep_info) = dep_info {
            let source = Path::new(path).with_file_name(&dep_info);
            if source.is_file() {
                copy_tree(&source, &destination.with_file_name(dep_info), from, to)?;
            }
        }
        Ok(())
    };
    let mut relocated = Vec::with_capacity(messages.len());
    for chunk in messages.split_inclusive(|byte| *byte == b'\n') {
        let line = chunk.strip_suffix(b"\n").unwrap_or(chunk);
        let Ok(mut message) = serde_json::from_slice::<Value>(line) else {
            relocated.extend_from_slice(chunk);
            continue;
        };
        match message.get("reason").and_then(Value::as_str) {
            Some("compiler-artifact") => {
                if let Some(files) = message.get_mut("filenames").and_then(Value::as_array_mut) {
                    let shared_rlib = |file: &Value| {
                        file.as_str().is_some_and(|file| {
                            file.ends_with(".rlib") && Path::new(file).starts_with(shared)
                        })
                    };
                    // Metadata is what the probe reads; the rlib only links.
                    if files
                        .iter()
                        .any(|file| file.as_str().is_some_and(|file| file.ends_with(".rmeta")))
                    {
                        files.retain(|file| !shared_rlib(file));
                    }
                    for file in files.iter().filter_map(Value::as_str) {
                        copy(file)?;
                    }
                }
                if let Some(executable) = message.get("executable").and_then(Value::as_str) {
                    copy(executable)?;
                }
            }
            Some("build-script-executed") => {
                if let Some(run) = message
                    .get("out_dir")
                    .and_then(Value::as_str)
                    .and_then(|out_dir| Path::new(out_dir).parent())
                {
                    copy(&run.to_string_lossy())?;
                }
            }
            _ => {}
        }
        relocate_strings(&mut message, from, to);
        let line = serde_json::to_string(&message).map_err(|error| Error::Input {
            code: "relocated-cargo-message",
            reason: error.to_string().into_boxed_str(),
        })?;
        relocated.extend_from_slice(line.as_bytes());
        if chunk.ends_with(b"\n") {
            relocated.push(b'\n');
        }
    }
    Ok(relocated.into_boxed_slice())
}

/// Give each file in the bundle's Cargo target directory its own inode.
/// Cargo uplifts a root unit's outputs from the shared build directory into
/// the target directory with cargo-util's `link_or_copy`. That is a copy (a
/// clone) on macOS but a hard link on Linux. A hard link would make the
/// bundle's file the live shared one: the next build would change it, and
/// freezing it would freeze the shared file too. Every bundle walk refuses a
/// hard-linked member for that reason. Each such file is replaced here by a
/// copy. The caller holds the shared directory's lock, so the shared file
/// cannot change during the copy.
pub(in crate::cargo) fn unshare(target: &Path) -> Result<()> {
    let io = |operation: &'static str, path: &Path| {
        let path: PathBuf = path.to_path_buf();
        move |source| Error::Io {
            operation,
            path,
            source,
        }
    };
    let mut pending = vec![target.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && directory == target => {
                return Ok(());
            }
            Err(error) => return Err(io("read bundle target directory", &directory)(error)),
        };
        for entry in entries {
            let path = entry
                .map_err(io("read bundle target directory", &directory))?
                .path();
            let metadata =
                fs::symlink_metadata(&path).map_err(io("inspect bundle target output", &path))?;
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() && crate::store::bundle::hard_linked(&metadata) {
                let mut name = path.file_name().unwrap_or_default().to_os_string();
                name.push(".rawr-unshare");
                let copy = path.with_file_name(name);
                fs::copy(&path, &copy).map_err(io("copy hard-linked target output", &copy))?;
                fs::rename(&copy, &path).map_err(io("replace hard-linked target output", &path))?;
            }
        }
    }
    Ok(())
}

/// `<target>/rawr`: the shared build directory is the project's target
/// directory, which also holds Rawr's store and generated sources. Those are
/// not build output and keep their paths.
const RAWR: &str = "rawr";

/// A message's strings name the copies. JSON's own escaping stays serde's.
fn relocate_strings(value: &mut Value, from: &str, to: &str) {
    match value {
        Value::String(text) if text.contains(from) => {
            *text = replace_build_paths(text, &[(from, to)]);
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| relocate_strings(value, from, to)),
        Value::Object(fields) => fields
            .values_mut()
            .for_each(|value| relocate_strings(value, from, to)),
        _ => {}
    }
}

/// `text` with every path under a spelling of the shared directory moved under
/// the same spelling of the bundle, except paths under its `rawr` directory.
/// Each pair is one spelling of `(from, to)`, and at each place the earliest,
/// then longest, spelling applies.
fn replace_build_paths(text: &str, spellings: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut next: Vec<Option<usize>> = spellings.iter().map(|(from, _)| text.find(from)).collect();
    let mut cursor = 0;
    while let Some((index, found)) = next
        .iter()
        .enumerate()
        .filter_map(|(index, found)| Some((index, (*found)?)))
        .min_by_key(|&(index, found)| (found, std::cmp::Reverse(spellings[index].0.len())))
    {
        let (from, to) = spellings[index];
        let end = found + from.len();
        let after = &text[end..];
        let separated = after.trim_start_matches(['/', '\\']);
        let reserved = separated.len() < after.len()
            && separated
                .strip_prefix(RAWR)
                .is_some_and(|after| after.is_empty() || after.starts_with(['/', '\\']));
        out.push_str(&text[cursor..found]);
        out.push_str(if reserved { from } else { to });
        cursor = end;
        for (slot, (from, _)) in next.iter_mut().zip(spellings) {
            if slot.is_some_and(|found| found < cursor) {
                *slot = text[cursor..].find(from).map(|found| cursor + found);
            }
        }
    }
    out.push_str(&text[cursor..]);
    out
}

/// The shared or bundle directory as text. rustc's dep-info escapes a space
/// (`\ `) and an env-dep value escapes a backslash, so each text file is
/// rewritten in its own spellings. A control character has no spelling that
/// survives a dependency rule, so such a path is refused, as is one that is not
/// UTF-8.
fn spelled(path: &Path) -> Result<&str> {
    path.to_str()
        .filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
        .ok_or_else(|| Error::Path {
            field: "shared Cargo build directory",
            path: path.to_path_buf(),
            reason: "must be UTF-8 without control characters to relocate".into(),
        })
}

/// Copy a file or tree once. Dep-info and script control files are text that
/// names shared paths; they name the copies instead.
fn copy_tree(source: &Path, destination: &Path, from: &str, to: &str) -> Result<()> {
    let io = |operation: &'static str, path: &Path| {
        let path: PathBuf = path.to_path_buf();
        move |source| Error::Io {
            operation,
            path,
            source,
        }
    };
    let metadata =
        fs::symlink_metadata(source).map_err(io("inspect shared build output", source))?;
    if fs::symlink_metadata(destination).is_ok() {
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(io("create relocated build directory", parent))?;
    }
    if metadata.is_dir() {
        fs::create_dir(destination).map_err(io("create relocated build directory", destination))?;
        for entry in fs::read_dir(source).map_err(io("read shared build directory", source))? {
            let entry = entry.map_err(io("read shared build directory", source))?;
            copy_tree(
                &entry.path(),
                &destination.join(entry.file_name()),
                from,
                to,
            )?;
        }
    } else if metadata.file_type().is_symlink() {
        let target = fs::read_link(source).map_err(io("read shared build link", source))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, destination)
            .map_err(io("relocate shared build link", destination))?;
        #[cfg(not(unix))]
        let _ = target;
    } else {
        let dep_info = matches!(
            destination
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("d")
        );
        let digest = if dep_info
            || matches!(
                destination.file_name().and_then(|name| name.to_str()),
                Some("output" | "root-output")
            ) {
            let text = fs::read(source).map_err(io("read shared build text", source))?;
            let text = String::from_utf8_lossy(&text);
            let text = if dep_info {
                let rule = (escape_path(from), escape_path(to));
                let environment = (escape_environment(from), escape_environment(to));
                replace_build_paths(
                    &text,
                    &[(&rule.0, &rule.1), (&environment.0, &environment.1)],
                )
            } else {
                replace_build_paths(&text, &[(from, to)])
            };
            fs::write(destination, &text).map_err(io("write relocated build text", destination))?;
            Some(Sha256::digest(text).into())
        } else {
            let identity = Identity::of(&metadata);
            let digest = match file_digest::get(identity, Domain::Sha256) {
                Some(digest) => digest,
                None => {
                    let mut digest = Sha256::new();
                    let mut file =
                        fs::File::open(source).map_err(io("open shared build output", source))?;
                    std::io::copy(&mut file, &mut digest)
                        .map_err(io("hash shared build output", source))?;
                    digest.finalize().into()
                }
            };
            fs::copy(source, destination).map_err(io("copy shared build output", destination))?;
            // Unchanged across the hash and the copy, so the copy has these bytes.
            let unchanged = fs::symlink_metadata(source)
                .ok()
                .and_then(|after| Identity::of(&after))
                == identity;
            if unchanged {
                file_digest::insert(identity, Domain::Sha256, digest);
            }
            unchanged.then_some(digest)
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let written = fs::symlink_metadata(destination)
                .map_err(io("inspect relocated build output", destination))?;
            let mode = crate::store::bundle::frozen_mode(&written, false);
            fs::set_permissions(destination, fs::Permissions::from_mode(mode))
                .map_err(io("freeze relocated build output", destination))?;
        }
        if let Some(digest) = digest {
            let frozen = fs::symlink_metadata(destination)
                .map_err(io("inspect relocated build output", destination))?;
            file_digest::insert(Identity::of(&frozen), Domain::Sha256, digest);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn paths_under_the_rawr_directory_keep_their_place() {
        let text = "/p/target/debug/deps/a.d /p/target/rawr/store/x.rs /p/target/rawr";
        assert_eq!(
            super::replace_build_paths(text, &[("/p/target", "/b")]),
            "/b/debug/deps/a.d /p/target/rawr/store/x.rs /p/target/rawr"
        );
        assert_eq!(
            super::replace_build_paths("/p/target/rawrx", &[("/p/target", "/b")]),
            "/b/rawrx"
        );
    }

    use std::fs;

    /// Linux Cargo uplifts with a hard link into the shared build directory;
    /// the bundle keeps the bytes in a file of its own.
    #[cfg(unix)]
    #[test]
    fn unshare_gives_each_hard_linked_target_output_its_own_inode() {
        use std::os::unix::fs::MetadataExt as _;

        let root = std::env::temp_dir().join(format!("rawr-unshare-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (shared, target) = (root.join("shared"), root.join("target/final/t/debug"));
        fs::create_dir_all(&shared).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(shared.join("liba-1.rlib"), b"rlib").unwrap();
        fs::hard_link(shared.join("liba-1.rlib"), target.join("liba.rlib")).unwrap();
        fs::write(target.join("liba.d"), b"dep-info").unwrap();
        super::unshare(&root.join("target/final")).unwrap();
        for path in [shared.join("liba-1.rlib"), target.join("liba.rlib")] {
            assert_eq!(fs::metadata(&path).unwrap().nlink(), 1, "{path:?}");
        }
        assert_eq!(fs::read(target.join("liba.rlib")).unwrap(), b"rlib");
        assert_eq!(fs::read_dir(&target).unwrap().count(), 2);
        super::unshare(&root.join("missing")).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn relocation_keeps_metadata_dep_info_and_script_runs_and_names_the_copies() {
        relocates_under("rawr-relocate");
    }

    /// A project in `My Counter` (D491): JSON, rustc's dep-info and script
    /// output each spell the paths their own way.
    #[test]
    fn relocation_spells_paths_with_spaces_and_make_characters_per_file() {
        relocates_under("rawr-relocate My Counter #1 $x:y");
    }

    fn relocates_under(name: &str) {
        use crate::input::{escape_environment, escape_path};

        let root = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (shared, bundle) = (root.join("shared"), root.join("bundle/build/final"));
        let deps = shared.join("t/debug/deps");
        let run = shared.join("t/debug/build/s-1");
        fs::create_dir_all(&deps).unwrap();
        fs::create_dir_all(run.join("out")).unwrap();
        fs::write(deps.join("liba-1.rlib"), b"link").unwrap();
        fs::write(deps.join("liba-1.rmeta"), b"meta").unwrap();
        fs::write(deps.join("liba-1.o"), b"object").unwrap();
        let s = shared.to_str().unwrap();
        let dep_info = |s: &str| {
            let rule = escape_path(s);
            format!(
                "{rule}/t/debug/deps/liba-1.rmeta: src/lib.rs {rule}/t/debug/build/s-1/out/g.rs\n\n\
                 # env-dep:OUT_DIR={}/t/debug/build/s-1/out\n",
                escape_environment(s)
            )
        };
        fs::write(deps.join("a-1.d"), dep_info(s)).unwrap();
        fs::write(run.join("out/g.rs"), b"pub fn g() {}").unwrap();
        fs::write(
            run.join("output"),
            format!("cargo:rustc-link-search={s}/t/debug/build/s-1/out\n"),
        )
        .unwrap();
        let json = |path: String| serde_json::to_string(&path).unwrap();
        let messages = format!(
            "{{\"reason\":\"compiler-artifact\",\"filenames\":[{},{}],\"executable\":null}}\n\
             {{\"reason\":\"build-script-executed\",\"out_dir\":{}}}\n\
             not json\n\
             {{\"reason\":\"build-finished\",\"success\":true}}",
            json(format!("{s}/t/debug/deps/liba-1.rlib")),
            json(format!("{s}/t/debug/deps/liba-1.rmeta")),
            json(format!("{s}/t/debug/build/s-1/out")),
        );
        let relocated = super::relocate(messages.as_bytes(), &shared, &bundle).unwrap();
        let relocated = std::str::from_utf8(&relocated).unwrap();
        let b = bundle.to_str().unwrap();
        assert!(!relocated.contains(json(s.into()).trim_matches('"')));
        assert!(relocated.contains(&format!(
            "\"filenames\":[{}]",
            json(format!("{b}/t/debug/deps/liba-1.rmeta"))
        )));
        assert!(relocated.contains(&format!(
            "\"out_dir\":{}",
            json(format!("{b}/t/debug/build/s-1/out"))
        )));
        assert!(relocated.ends_with("\nnot json\n{\"reason\":\"build-finished\",\"success\":true}"));
        for line in relocated.lines().filter(|line| *line != "not json") {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
        let copies = bundle.join("t/debug/deps");
        assert_eq!(fs::read(copies.join("liba-1.rmeta")).unwrap(), b"meta");
        assert!(!copies.join("liba-1.rlib").exists() && !copies.join("liba-1.o").exists());
        assert_eq!(
            fs::read_to_string(copies.join("a-1.d")).unwrap(),
            dep_info(b)
        );
        let copied = bundle.join("t/debug/build/s-1");
        assert_eq!(fs::read(copied.join("out/g.rs")).unwrap(), b"pub fn g() {}");
        assert_eq!(
            fs::read_to_string(copied.join("output")).unwrap(),
            format!("cargo:rustc-link-search={b}/t/debug/build/s-1/out\n")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn relocated_copies_are_frozen_and_memoized_and_a_changed_unit_is_hashed_again() {
        use crate::file_digest::{get, Domain, Identity};
        use sha2::{Digest as _, Sha256};
        use std::os::unix::fs::PermissionsExt as _;

        let root = std::env::temp_dir().join(format!("rawr-relocate-memo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let shared = root.join("shared");
        let deps = shared.join("t/debug/deps");
        fs::create_dir_all(&deps).unwrap();
        fs::write(deps.join("liba-1.rmeta"), b"meta").unwrap();
        fs::write(
            deps.join("a-1.d"),
            format!("{}/t/debug/deps/liba-1.rmeta:\n", shared.display()),
        )
        .unwrap();
        let messages = format!(
            "{{\"reason\":\"compiler-artifact\",\"filenames\":[\"{}/t/debug/deps/liba-1.rmeta\"]}}",
            shared.display()
        );
        let relocate = |bundle: &str| {
            let bundle = root.join(bundle);
            super::relocate(messages.as_bytes(), &shared, &bundle).unwrap();
            bundle.join("t/debug/deps")
        };
        let memoized = |path: std::path::PathBuf| {
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o7777, 0o444);
            let digest = get(Identity::of(&metadata), Domain::Sha256).unwrap();
            assert_eq!(
                digest,
                <[u8; 32]>::from(Sha256::digest(fs::read(&path).unwrap()))
            );
            digest
        };
        let first = relocate("one");
        let unchanged = fs::symlink_metadata(deps.join("liba-1.rmeta")).unwrap();
        let digest = memoized(first.join("liba-1.rmeta"));
        assert_eq!(get(Identity::of(&unchanged), Domain::Sha256), Some(digest));
        memoized(first.join("a-1.d"));
        // An unchanged unit reuses the shared file's digest.
        assert_eq!(memoized(relocate("two").join("liba-1.rmeta")), digest);
        // A changed unit is a new identity, hashed from its new bytes.
        fs::write(deps.join("liba-1.rmeta"), b"changed").unwrap();
        let changed = memoized(relocate("three").join("liba-1.rmeta"));
        assert_ne!(changed, digest);
        fs::remove_dir_all(root).unwrap();
    }
}
