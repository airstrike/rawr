//! Cargo's view of one request, remembered across the edits that leave it
//! unchanged.
//!
//! `cargo metadata` (provisional and candidate), the lock it writes and the
//! unit graph are functions of the request (its manifests, lock seed, config,
//! command cohort and toolchain) and of what Cargo reads from the packages:
//! their manifests, the manifests above them, Cargo config files, and which
//! target files exist for auto-discovery. None of them reads a Rust source's
//! bytes, so a `.rs` edit leaves all four outputs unchanged. They name the
//! bundle they were produced in, so a reuse rewrites that bundle's root to the
//! new one. Anything else (a new request, a changed manifest, an added target,
//! an unusual root path) runs Cargo as before.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest as _, Sha256};

/// The four Cargo outputs of one request.
#[derive(Clone)]
pub(in crate::cargo) struct Outputs {
    pub(in crate::cargo) provisional: Box<[u8]>,
    pub(in crate::cargo) candidate: Box<[u8]>,
    pub(in crate::cargo) lock: Box<[u8]>,
    pub(in crate::cargo) units: Box<[u8]>,
}

struct Entry {
    base: [u8; 32],
    root: String,
    roots: Box<[PathBuf]>,
    determinants: [u8; 32],
    outputs: Outputs,
}

static REMEMBERED: Mutex<Vec<Arc<Entry>>> = Mutex::new(Vec::new());

/// Requests remembered: the current one and a few it may return to.
const ENTRIES: usize = 4;

/// The request's own identity: every Cargo input it names.
pub(in crate::cargo) fn base(parts: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.cargo.remembered/v1");
    for part in parts {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part);
    }
    digest.finalize().into()
}

/// The remembered outputs for this request when Cargo's view is unchanged,
/// rewritten to name `root`.
pub(in crate::cargo) fn lookup(base: &[u8; 32], root: &Path) -> Option<Outputs> {
    let new = plain(root)?;
    let entries = REMEMBERED.lock().ok()?.clone();
    let entry = entries.iter().find(|entry| entry.base == *base)?;
    if determinants(&entry.roots, root)? != entry.determinants {
        return None;
    }
    let rewrite = |bytes: &[u8]| replace(bytes, entry.root.as_bytes(), new.as_bytes());
    Some(Outputs {
        provisional: rewrite(&entry.outputs.provisional),
        candidate: rewrite(&entry.outputs.candidate),
        lock: entry.outputs.lock.clone(),
        units: rewrite(&entry.outputs.units),
    })
}

/// Remember what Cargo produced for this request in the bundle at `root`.
pub(in crate::cargo) fn remember(base: [u8; 32], root: &Path, roots: &[PathBuf], outputs: Outputs) {
    let Some(text) = plain(root) else { return };
    let Some(determinants) = determinants(roots, root) else {
        return;
    };
    let entry = Arc::new(Entry {
        base,
        root: text.to_owned(),
        roots: roots.into(),
        determinants,
        outputs,
    });
    if let Ok(mut remembered) = REMEMBERED.lock() {
        remembered.retain(|known| known.base != base);
        remembered.insert(0, entry);
        remembered.truncate(ENTRIES);
    }
}

/// A root whose text appears verbatim in Cargo's JSON and package ids.
fn plain(root: &Path) -> Option<&str> {
    let text = root.to_str()?;
    (text.len() > 1
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-')))
    .then_some(text)
}

fn replace(bytes: &[u8], old: &[u8], new: &[u8]) -> Box<[u8]> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(index) = rest.windows(old.len()).position(|window| window == old) {
        output.extend_from_slice(&rest[..index]);
        output.extend_from_slice(new);
        rest = &rest[index + old.len()..];
    }
    output.extend_from_slice(rest);
    output.into_boxed_slice()
}

/// What Cargo reads besides the request: each package manifest, every
/// `Cargo.toml` and Cargo config above it and above the harness, and which
/// target files exist for auto-discovery. Source bytes are not read.
fn determinants(roots: &[PathBuf], harness: &Path) -> Option<[u8; 32]> {
    let mut files = std::collections::BTreeSet::new();
    let mut directories = std::collections::BTreeSet::new();
    let ancestors = |files: &mut std::collections::BTreeSet<PathBuf>, start: &Path| {
        for directory in start.ancestors() {
            files.insert(directory.join("Cargo.toml"));
            files.insert(directory.join(".cargo/config.toml"));
            files.insert(directory.join(".cargo/config"));
        }
    };
    // Above the bundle's two store-named directories, which differ per bundle.
    if let Some(store) = harness.ancestors().nth(2) {
        ancestors(&mut files, store);
    }
    for manifest in roots {
        files.insert(manifest.clone());
        let package = manifest.parent()?;
        ancestors(&mut files, package);
        for file in ["build.rs", "src/lib.rs", "src/main.rs"] {
            files.insert(package.join(file));
        }
        for directory in ["src/bin", "examples", "tests", "benches"] {
            directories.insert(package.join(directory));
        }
    }
    let mut digest = Sha256::new();
    for file in &files {
        digest.update(file.as_os_str().as_encoded_bytes());
        // Auto-discovery files count by presence, manifests and configs by bytes.
        let named = file.file_name().and_then(|name| name.to_str());
        if named.is_some_and(|name| name.ends_with(".rs")) {
            digest.update([u8::from(file.is_file())]);
            continue;
        }
        match std::fs::read(file) {
            Ok(bytes) => {
                digest.update([1]);
                digest.update((bytes.len() as u64).to_le_bytes());
                digest.update(&bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => digest.update([0]),
            Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => digest.update([0]),
            Err(_) => return None,
        }
    }
    for directory in &directories {
        digest.update(directory.as_os_str().as_encoded_bytes());
        let mut names = match std::fs::read_dir(directory) {
            Ok(entries) => entries
                .map(|entry| {
                    let entry = entry.ok()?;
                    let kind = entry.file_type().ok()?;
                    let main = kind.is_dir() && entry.path().join("main.rs").is_file();
                    Some((entry.file_name(), kind.is_dir(), main))
                })
                .collect::<Option<Vec<_>>>()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return None,
        };
        names.sort_unstable();
        digest.update((names.len() as u64).to_le_bytes());
        for (name, directory, main) in names {
            digest.update(name.as_encoded_bytes());
            digest.update([u8::from(directory), u8::from(main)]);
        }
    }
    Some(digest.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::{lookup, remember, Outputs};

    #[test]
    fn cargo_outputs_survive_a_source_edit_and_not_a_manifest_or_target_change() {
        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-scratch")
            .join(format!("remembered-{}", std::process::id()));
        let package = scratch.join("package");
        std::fs::create_dir_all(package.join("src")).unwrap();
        std::fs::write(package.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
        std::fs::write(package.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        let old = scratch.join("executions/a/bundle");
        let new = scratch.join("executions/b/bundle");
        let roots = [package.join("Cargo.toml")];
        let named = |root: &std::path::Path| {
            format!("{{\"workspace_root\":\"{}\"}}", root.display()).into_bytes()
        };
        let base = super::base(&[b"request"]);
        remember(
            base,
            &old,
            &roots,
            Outputs {
                provisional: named(&old).into(),
                candidate: named(&old).into(),
                lock: Box::from(&b"lock"[..]),
                units: named(&old).into(),
            },
        );
        std::fs::write(package.join("src/lib.rs"), "pub fn f() { let _ = 1; }\n").unwrap();
        let reused = lookup(&base, &new).expect("a source edit keeps Cargo's view");
        assert_eq!(&*reused.candidate, named(&new).as_slice());
        assert_eq!(&*reused.units, named(&new).as_slice());
        assert_eq!(&*reused.lock, b"lock");
        assert!(lookup(&super::base(&[b"other"]), &new).is_none());
        std::fs::create_dir_all(package.join("src/bin")).unwrap();
        std::fs::write(package.join("src/bin/tool.rs"), "fn main() {}\n").unwrap();
        assert!(lookup(&base, &new).is_none(), "a new binary target");
        std::fs::remove_dir_all(package.join("src/bin")).unwrap();
        assert!(lookup(&base, &new).is_some());
        std::fs::write(package.join("Cargo.toml"), "[package]\nname = \"q\"\n").unwrap();
        assert!(lookup(&base, &new).is_none(), "a changed manifest");
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
