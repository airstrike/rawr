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

use crate::{Error, Result};

/// Copy what this build's messages name from `shared` to `bundle` and return the
/// messages naming the copies. The caller holds the shared directory's lock.
pub(in crate::cargo) fn relocate(
    messages: &[u8],
    shared: &Path,
    bundle: &Path,
) -> Result<Box<[u8]>> {
    let (from, to) = (plain(shared)?, plain(bundle)?);
    let copy = |path: &str| -> Result<()> {
        let Ok(relative) = Path::new(path).strip_prefix(shared) else {
            return Ok(());
        };
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
        let line = serde_json::to_string(&message).map_err(|error| Error::Input {
            code: "relocated-cargo-message",
            reason: error.to_string().into_boxed_str(),
        })?;
        relocated.extend_from_slice(line.replace(from, to).as_bytes());
        if chunk.ends_with(b"\n") {
            relocated.push(b'\n');
        }
    }
    Ok(relocated.into_boxed_slice())
}

/// A path spelled identically in JSON, dep-info and script output.
fn plain(path: &Path) -> Result<&str> {
    path.to_str()
        .filter(|text| {
            !text.is_empty()
                && !text.chars().any(|c| {
                    c.is_control() || c.is_whitespace() || matches!(c, '"' | '\\' | ':' | '#')
                })
        })
        .ok_or_else(|| Error::Path {
            field: "shared Cargo build directory",
            path: path.to_path_buf(),
            reason: "must be plain UTF-8 to relocate".into(),
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
        let digest = if matches!(
            destination
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("d")
        ) || matches!(
            destination.file_name().and_then(|name| name.to_str()),
            Some("output" | "root-output")
        ) {
            let text = fs::read(source).map_err(io("read shared build text", source))?;
            let text = String::from_utf8_lossy(&text).replace(from, to);
            fs::write(destination, &text).map_err(io("write relocated build text", destination))?;
            Some(Sha256::digest(text).into())
        } else {
            let identity = Identity::of(&metadata);
            let digest = match file_digest::get(identity, Domain::Sha256) {
                Some(digest) => digest,
                None => {
                    let mut digest = Sha256::new();
                    let mut file = fs::File::open(source).map_err(io("open shared build output", source))?;
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
    use std::fs;

    #[test]
    fn relocation_keeps_metadata_dep_info_and_script_runs_and_names_the_copies() {
        let root = std::env::temp_dir().join(format!("rawr-relocate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (shared, bundle) = (root.join("shared"), root.join("bundle/build/final"));
        let deps = shared.join("t/debug/deps");
        let run = shared.join("t/debug/build/s-1");
        fs::create_dir_all(&deps).unwrap();
        fs::create_dir_all(run.join("out")).unwrap();
        fs::write(deps.join("liba-1.rlib"), b"link").unwrap();
        fs::write(deps.join("liba-1.rmeta"), b"meta").unwrap();
        fs::write(deps.join("liba-1.o"), b"object").unwrap();
        let s = shared.display();
        fs::write(
            deps.join("a-1.d"),
            format!("{s}/t/debug/deps/liba-1.rmeta: src/lib.rs {s}/t/debug/build/s-1/out/g.rs\n"),
        )
        .unwrap();
        fs::write(run.join("out/g.rs"), b"pub fn g() {}").unwrap();
        fs::write(
            run.join("output"),
            format!("cargo:rustc-link-search={s}/t/debug/build/s-1/out\n"),
        )
        .unwrap();
        let messages = format!(
            "{{\"reason\":\"compiler-artifact\",\"filenames\":[\"{s}/t/debug/deps/liba-1.rlib\",\"{s}/t/debug/deps/liba-1.rmeta\"],\"executable\":null}}\n\
             {{\"reason\":\"build-script-executed\",\"out_dir\":\"{s}/t/debug/build/s-1/out\"}}\n\
             not json\n\
             {{\"reason\":\"build-finished\",\"success\":true}}"
        );
        let relocated = super::relocate(messages.as_bytes(), &shared, &bundle).unwrap();
        let relocated = std::str::from_utf8(&relocated).unwrap();
        let b = bundle.display();
        assert!(!relocated.contains(&s.to_string()));
        assert!(relocated.contains(&format!(
            "\"filenames\":[\"{b}/t/debug/deps/liba-1.rmeta\"]"
        )));
        assert!(relocated.contains(&format!("\"out_dir\":\"{b}/t/debug/build/s-1/out\"")));
        assert!(relocated.ends_with("\nnot json\n{\"reason\":\"build-finished\",\"success\":true}"));
        for line in relocated.lines().filter(|line| *line != "not json") {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
        let copies = bundle.join("t/debug/deps");
        assert_eq!(fs::read(copies.join("liba-1.rmeta")).unwrap(), b"meta");
        assert!(!copies.join("liba-1.rlib").exists() && !copies.join("liba-1.o").exists());
        assert_eq!(
            fs::read_to_string(copies.join("a-1.d")).unwrap(),
            format!("{b}/t/debug/deps/liba-1.rmeta: src/lib.rs {b}/t/debug/build/s-1/out/g.rs\n")
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
        fs::write(deps.join("a-1.d"), format!("{}/t/debug/deps/liba-1.rmeta:\n", shared.display()))
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
            assert_eq!(digest, <[u8; 32]>::from(Sha256::digest(fs::read(&path).unwrap())));
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
