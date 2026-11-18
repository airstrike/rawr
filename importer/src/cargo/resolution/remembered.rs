//! Cargo's view of one request, remembered across the edits that leave it
//! unchanged.
//!
//! `cargo metadata` (provisional and candidate), the lock it writes and the
//! unit graph are functions of the request (its manifests, lock seed, config,
//! command cohort and toolchain) and of what Cargo reads from the packages:
//! their manifests, the manifests above them, Cargo config files, and which
//! target files exist for auto-discovery. None of them reads a Rust source's
//! bytes, so a `.rs` edit leaves them all unchanged. The provisional metadata
//! is read only for the internal recipe it seals, so the recipe is kept in its
//! place (#1837). The others name the bundle they were produced in, so a reuse
//! rewrites that bundle's root to the new one, in each spelling Cargo gives
//! it: a JSON string's text for a path, and a percent-encoded `file://` URL
//! inside a package id. Anything else (a new request, a changed manifest, an
//! added target) runs Cargo as before.
//!
//! The demand is not part of the request here: it changes what the probe
//! extracts, never what Cargo sees. The outputs are also kept in the
//! project's Rawr directory, so the next process starts from them too.
//!
//! The files are read when the outputs are saved, after Cargo ran. A file
//! modified since Cargo started reading may hold bytes Cargo never saw, so
//! the outputs are then not saved, and the next run asks Cargo again. As in
//! Cargo's own freshness check, a modification time at or after the start
//! counts. Only manifests, configs and the directories listed for targets
//! are compared; a source's bytes are never part of Cargo's view.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest as _, Sha256};

/// The Cargo outputs of one request.
#[derive(Clone)]
pub(in crate::cargo) struct Outputs {
    /// The internal recipe the provisional metadata sealed, as
    /// `internal::Recipe::encode` wrote it.
    pub(in crate::cargo) internal: Box<[u8]>,
    pub(in crate::cargo) candidate: Box<[u8]>,
    pub(in crate::cargo) lock: Box<[u8]>,
    pub(in crate::cargo) units: Box<[u8]>,
}

struct Entry {
    base: [u8; 32],
    root: Spelled,
    roots: Box<[PathBuf]>,
    determinants: [u8; 32],
    outputs: Outputs,
}

static REMEMBERED: Mutex<Vec<Arc<Entry>>> = Mutex::new(Vec::new());

/// Requests remembered: the current one and a few it may return to.
const ENTRIES: usize = 4;

/// Where a project keeps them, under its Rawr directory.
const DIRECTORY: &str = "cargo";

fn file(rawr: &Path, base: &[u8; 32]) -> PathBuf {
    rawr.join(DIRECTORY).join(crate::extract::engine::hex(base))
}

/// A kept entry's format: each field framed by its length.
const FORMAT: &[u8] = b"rawr.cargo.remembered/v2";

fn encode(entry: &Entry) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut frame = |field: &[u8]| {
        bytes.extend_from_slice(&(field.len() as u64).to_le_bytes());
        bytes.extend_from_slice(field);
    };
    frame(FORMAT);
    frame(&entry.base);
    frame(entry.root.text.as_bytes());
    frame(&(entry.roots.len() as u64).to_le_bytes());
    for root in &entry.roots {
        frame(root.to_str()?.as_bytes());
    }
    frame(&entry.determinants);
    let outputs = &entry.outputs;
    for field in [
        &outputs.internal,
        &outputs.candidate,
        &outputs.lock,
        &outputs.units,
    ] {
        frame(field);
    }
    Some(bytes)
}

fn decode(mut bytes: &[u8]) -> Option<Entry> {
    let mut field = || -> Option<&[u8]> {
        let length = usize::try_from(u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?)).ok()?;
        let field = bytes.get(8..8usize.checked_add(length)?)?;
        bytes = &bytes[8 + length..];
        Some(field)
    };
    if field()? != FORMAT {
        return None;
    }
    let base = field()?.try_into().ok()?;
    let root = Spelled::of(Path::new(std::str::from_utf8(field()?).ok()?))?;
    let count = u64::from_le_bytes(field()?.try_into().ok()?);
    let roots = (0..count)
        .map(|_| Some(PathBuf::from(std::str::from_utf8(field()?).ok()?)))
        .collect::<Option<Box<[PathBuf]>>>()?;
    let determinants = field()?.try_into().ok()?;
    let mut output = || field().map(Box::from);
    Some(Entry {
        base,
        root,
        roots,
        determinants,
        outputs: Outputs {
            internal: output()?,
            candidate: output()?,
            lock: output()?,
            units: output()?,
        },
    })
}

/// The entry an earlier process kept, read back. A file that does not
/// decode is a miss.
fn kept(rawr: &Path, base: &[u8; 32]) -> Option<Arc<Entry>> {
    let entry = decode(&std::fs::read(file(rawr, base)).ok()?)?;
    (entry.base == *base).then(|| Arc::new(entry))
}

/// Keep `entry` for the next process, and only the few newest beside it.
fn keep(rawr: &Path, entry: &Entry) {
    let directory = rawr.join(DIRECTORY);
    let Some(bytes) = encode(entry) else {
        return;
    };
    let path = file(rawr, &entry.base);
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    if std::fs::create_dir_all(&directory).is_err()
        || std::fs::write(&temporary, bytes).is_err()
        || std::fs::rename(&temporary, &path).is_err()
    {
        let _ = std::fs::remove_file(&temporary);
        return;
    }
    let Ok(listed) = std::fs::read_dir(&directory) else {
        return;
    };
    let mut files = listed
        .flatten()
        .filter_map(|item| Some((item.metadata().ok()?.modified().ok()?, item.path())))
        .collect::<Vec<_>>();
    files.sort_unstable_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, stale) in files.into_iter().skip(ENTRIES) {
        let _ = std::fs::remove_file(stale);
    }
}

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
/// rewritten to name `root`: this process's, else the one kept under the
/// project's Rawr directory `rawr`.
pub(in crate::cargo) fn lookup(base: &[u8; 32], root: &Path, rawr: &Path) -> Option<Outputs> {
    let new = Spelled::of(root)?;
    let known = REMEMBERED
        .lock()
        .ok()?
        .iter()
        .find(|entry| entry.base == *base)
        .cloned();
    let entry = match known {
        Some(entry) => entry,
        None => {
            let entry = kept(rawr, base)?;
            if let Ok(mut remembered) = REMEMBERED.lock() {
                remembered.insert(0, Arc::clone(&entry));
                remembered.truncate(ENTRIES);
            }
            entry
        }
    };
    if determinants(&entry.roots, root, None)? != entry.determinants {
        return None;
    }
    let rewrite = |bytes: &[u8]| entry.root.rewrite(bytes, &new);
    Some(Outputs {
        internal: entry.outputs.internal.clone(),
        candidate: rewrite(&entry.outputs.candidate),
        lock: entry.outputs.lock.clone(),
        units: rewrite(&entry.outputs.units),
    })
}

/// Forget every remembered request, in this process and under the
/// project's Rawr directory `rawr`, so the next preparation asks Cargo.
#[cfg(test)]
pub(in crate::cargo) fn forget(rawr: &Path) {
    if let Ok(mut remembered) = REMEMBERED.lock() {
        remembered.clear();
    }
    match std::fs::remove_dir_all(rawr.join(DIRECTORY)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            panic!("the remembered outputs are removed: {error}")
        }
        _ => {}
    }
}

/// When Cargo started reading the files of one request.
#[derive(Clone, Copy)]
pub(in crate::cargo) struct Started(Duration);

impl Started {
    /// Taken before Cargo reads anything for the request.
    pub(in crate::cargo) fn now() -> Self {
        Self(since_epoch(SystemTime::now()).unwrap_or_default())
    }

    /// Whether a file modified at `modified` was modified before this
    /// start. A time with no fraction of a second may come from a
    /// filesystem that keeps whole seconds, so it is compared at the start's
    /// whole second.
    fn precedes(self, modified: SystemTime) -> bool {
        let Some(modified) = since_epoch(modified) else {
            return false;
        };
        if modified.subsec_nanos() == 0 {
            modified.as_secs() < self.0.as_secs()
        } else {
            modified < self.0
        }
    }
}

fn since_epoch(time: SystemTime) -> Option<Duration> {
    time.duration_since(UNIX_EPOCH).ok()
}

/// Remember what Cargo produced for this request in the bundle at `root`,
/// and keep it under the project's Rawr directory `rawr`, unless a file
/// Cargo read was modified since `started`.
pub(in crate::cargo) fn remember(
    base: [u8; 32],
    root: &Path,
    roots: &[PathBuf],
    outputs: Outputs,
    rawr: &Path,
    started: Started,
) {
    let Some(spelled) = Spelled::of(root) else {
        return;
    };
    let Some(determinants) = determinants(roots, root, Some(started)) else {
        return;
    };
    let entry = Arc::new(Entry {
        base,
        root: spelled,
        roots: roots.into(),
        determinants,
        outputs,
    });
    keep(rawr, &entry);
    if let Ok(mut remembered) = REMEMBERED.lock() {
        remembered.retain(|known| known.base != base);
        remembered.insert(0, entry);
        remembered.truncate(ENTRIES);
    }
}

/// A bundle root as Cargo's JSON spells it: the text of a path string, and
/// the `file://` URL a path package's id holds, whose path segments are
/// percent-encoded.
struct Spelled {
    text: String,
    path: String,
    url: String,
}

impl Spelled {
    /// `None` for a root no rewrite names exactly: one that isn't Unicode
    /// or absolute, or the filesystem root, whose text begins every path.
    fn of(root: &Path) -> Option<Self> {
        let text = root.to_str()?;
        if text.len() <= 1 || !root.is_absolute() {
            return None;
        }
        let quoted = serde_json::to_string(text).ok()?;
        let path = quoted.get(1..quoted.len() - 1)?.to_owned();
        let mut url = String::from("file://");
        for component in root.components().skip(1) {
            url.push('/');
            for &byte in component.as_os_str().as_encoded_bytes() {
                if segment_escaped(byte) {
                    url.push_str(&format!("%{byte:02X}"));
                } else {
                    url.push(char::from(byte));
                }
            }
        }
        Some(Self {
            text: text.to_owned(),
            path,
            url,
        })
    }

    /// `bytes` with each spelling of this root replaced by `new`'s same
    /// spelling. A URL is matched with its scheme, which no path starts
    /// with, so a root both spellings write alike still takes the new
    /// root's own spelling in each place.
    fn rewrite(&self, bytes: &[u8], new: &Self) -> Box<[u8]> {
        let spellings = [(&self.url, &new.url), (&self.path, &new.path)];
        let mut output = Vec::with_capacity(bytes.len());
        let mut rest = bytes;
        'scan: while let Some((&byte, next)) = rest.split_first() {
            for (old, new) in spellings {
                if let Some(after) = rest.strip_prefix(old.as_bytes()) {
                    output.extend_from_slice(new.as_bytes());
                    rest = after;
                    continue 'scan;
                }
            }
            output.push(byte);
            rest = next;
        }
        output.into_boxed_slice()
    }
}

/// Whether a file URL's path segment percent-encodes `byte`: a control, a
/// non-ASCII byte, a space or one of the delimiters below.
fn segment_escaped(byte: u8) -> bool {
    !(0x20..0x7f).contains(&byte) || b" \"#<>?`{}/%\\".contains(&byte)
}

/// What Cargo reads besides the request: each package manifest, every
/// `Cargo.toml` and Cargo config above it and above the harness, and which
/// target files exist for auto-discovery. Source bytes are not read. With
/// `since`, a manifest, config or listed directory modified at or after it
/// is `None`; a source's edit leaves Cargo's view as it was.
fn determinants(roots: &[PathBuf], harness: &Path, since: Option<Started>) -> Option<[u8; 32]> {
    let unchanged = |path: &Path| match (since, std::fs::metadata(path)) {
        (Some(since), Ok(metadata)) => metadata
            .modified()
            .is_ok_and(|modified| since.precedes(modified)),
        _ => true,
    };
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
        if !unchanged(file) {
            return None;
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
        if !unchanged(directory) {
            return None;
        }
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
    use super::{lookup, remember, Outputs, Spelled, Started};

    /// A root's path text is a JSON string's and its URL is the one a file
    /// URL gives it, so a rewrite between a plain root and one with a space,
    /// braces, a quote, a percent sign or non-ASCII names each exactly
    /// (#2249).
    #[test]
    fn a_root_is_rewritten_in_each_spelling_cargo_gives_it() {
        let plain = std::path::Path::new("/store/executions/a/bundle");
        let unusual = std::path::Path::new("/store/{home} \"x\"%/é/bundle");
        let spell = |root: &std::path::Path| {
            let url = url::Url::from_file_path(root).expect("an absolute root is a URL");
            let path = serde_json::to_string(root).expect("a Unicode root encodes");
            let path = path.trim_matches('"').to_owned();
            format!("{{\"id\":\"path+{url}#p@0.1.0\",\"path\":\"{path}/src/lib.rs\"}}")
        };
        for (old, new) in [(plain, unusual), (unusual, plain)] {
            let (spelled, renamed) = (Spelled::of(old).unwrap(), Spelled::of(new).unwrap());
            let rewritten = spelled.rewrite(spell(old).as_bytes(), &renamed);
            assert_eq!(std::str::from_utf8(&rewritten).unwrap(), spell(new));
        }
        assert!(Spelled::of(std::path::Path::new("/")).is_none());
        assert!(Spelled::of(std::path::Path::new("relative/bundle")).is_none());
    }

    /// A manifest modified after Cargo started reading leaves the outputs
    /// unsaved, so the next lookup misses; a source edited then does not
    /// (#2246).
    #[test]
    fn outputs_are_not_saved_beside_a_manifest_modified_after_cargo_started() {
        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-scratch")
            .join(format!("remembered-started-{}", std::process::id()));
        let package = scratch.join("package");
        std::fs::create_dir_all(package.join("src")).unwrap();
        std::fs::write(package.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
        std::fs::write(package.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        let bundle = scratch.join("executions/a/bundle");
        let roots = [package.join("Cargo.toml")];
        let rawr = scratch.join("rawr");
        let outputs = Outputs {
            internal: Box::from(&b"recipe"[..]),
            candidate: Box::from(&b"{}"[..]),
            lock: Box::from(&b"lock"[..]),
            units: Box::from(&b"{}"[..]),
        };
        let base = super::base(&[b"started-source"]);
        let started = Started::now();
        std::fs::write(package.join("src/lib.rs"), "pub fn g() {}\n").unwrap();
        remember(base, &bundle, &roots, outputs.clone(), &rawr, started);
        assert!(lookup(&base, &bundle, &rawr).is_some(), "a source edit");
        let base = super::base(&[b"started-manifest"]);
        let started = Started::now();
        std::fs::write(package.join("Cargo.toml"), "[package]\nname = \"q\"\n").unwrap();
        remember(base, &bundle, &roots, outputs, &rawr, started);
        assert!(lookup(&base, &bundle, &rawr).is_none(), "a manifest edit");
        std::fs::remove_dir_all(scratch).unwrap();
    }

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
        let rawr = scratch.join("rawr");
        let base = super::base(&[b"request"]);
        remember(
            base,
            &old,
            &roots,
            Outputs {
                internal: Box::from(&b"recipe"[..]),
                candidate: named(&old).into(),
                lock: Box::from(&b"lock"[..]),
                units: named(&old).into(),
            },
            &rawr,
            Started::now(),
        );
        std::fs::write(package.join("src/lib.rs"), "pub fn f() { let _ = 1; }\n").unwrap();
        let reused = lookup(&base, &new, &rawr).expect("a source edit keeps Cargo's view");
        assert_eq!(&*reused.candidate, named(&new).as_slice());
        assert_eq!(&*reused.units, named(&new).as_slice());
        assert_eq!(&*reused.lock, b"lock");
        assert_eq!(&*reused.internal, b"recipe");
        assert!(lookup(&super::base(&[b"other"]), &new, &rawr).is_none());
        // The next process starts from what this one kept.
        super::REMEMBERED.lock().unwrap().clear();
        let kept = lookup(&base, &new, &rawr).expect("kept for the next process");
        assert_eq!(&*kept.units, named(&new).as_slice());
        std::fs::create_dir_all(package.join("src/bin")).unwrap();
        std::fs::write(package.join("src/bin/tool.rs"), "fn main() {}\n").unwrap();
        assert!(lookup(&base, &new, &rawr).is_none(), "a new binary target");
        std::fs::remove_dir_all(package.join("src/bin")).unwrap();
        assert!(lookup(&base, &new, &rawr).is_some());
        std::fs::write(package.join("Cargo.toml"), "[package]\nname = \"q\"\n").unwrap();
        assert!(lookup(&base, &new, &rawr).is_none(), "a changed manifest");
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
