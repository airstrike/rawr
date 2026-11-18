//! Conservative manifest determinants, captured before Cargo consumes them.
//!
//! This is deliberately not a Cargo workspace selector. Every owner/ancestor
//! candidate and recursively named workspace is observed, including unrelated
//! malformed manifests. Cargo's own metadata and Build supply semantics; this
//! image only proves that the manifests they read did not change. The one
//! rule of Cargo's it keeps is that the search for a workspace ends at a
//! manifest declaring `[workspace]`, so a change above a workspace's root
//! is no drift.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use rawr_toml::document;
use sha2::{Digest as _, Sha256};

use super::{proof::Entry, Allocation};
use crate::{cargo, node, Error, Result};

pub(crate) struct Image {
    pub(crate) roots: Box<[PathBuf]>,
    pub(crate) entry: Entry,
    // Live derivation proof only. Persisted records retain roots and regenerate
    // complete coverage from current owners; this list is never serialized.
    candidates: Box<[PathBuf]>,
    // The observation's digest before each candidate and after the last, so
    // a changed image names the first candidate it differs at. Live only.
    prefixes: Box<[[u8; 32]]>,
}

pub(crate) struct Account<'a> {
    storage: Option<&'a mut Allocation>,
    limits: &'a cargo::Limits,
}

struct Slots<T> {
    values: Vec<T>,
    admitted: usize,
}

impl<T> Slots<T> {
    fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: 0,
        }
    }
}

impl<'a> Account<'a> {
    pub(crate) fn borrow(storage: &'a mut Allocation, limits: &'a cargo::Limits) -> Self {
        Self {
            storage: Some(storage),
            limits,
        }
    }

    /// An account that retains into no input allocation: Cargo resolution's
    /// own reads keep only the physical bounds below.
    pub(crate) fn bounded(limits: &'a cargo::Limits) -> Self {
        Self {
            storage: None,
            limits,
        }
    }

    // Copies of admitted bytes are owned once by the original storage
    // account; probes and hashing keep their own graph axes below.
    fn retain(&mut self, items: usize, bytes: usize) -> Result<()> {
        match self.storage.as_mut() {
            Some(storage) => {
                storage.items(items)?;
                storage.bytes(bytes)
            }
            None => Ok(()),
        }
    }

    fn copy(&mut self, path: &Path) -> Result<PathBuf> {
        self.retain(1, path.as_os_str().len())?;
        let mut value = PathBuf::new();
        value
            .try_reserve_exact(path.as_os_str().len())
            .map_err(allocation)?;
        value.push(path);
        Ok(value)
    }

    fn push<T>(&mut self, slots: &mut Slots<T>, value: T) -> Result<()> {
        if slots.values.len() == slots.admitted {
            let count = slots.admitted.checked_mul(2).ok_or_else(overflow)?.max(1);
            let bytes = count
                .checked_mul(std::mem::size_of::<T>())
                .ok_or_else(overflow)?;
            // Full replacement ownership, independent of allocator surplus.
            self.retain(count, bytes)?;
            slots
                .values
                .try_reserve_exact(count - slots.values.len())
                .map_err(allocation)?;
            slots.admitted = count;
        }
        slots.values.push(value);
        Ok(())
    }

    fn boxed<T>(&mut self, slots: Slots<T>) -> Result<Box<[T]>> {
        let values = slots.values;
        if values.capacity() != values.len() {
            self.retain(
                values.len(),
                values
                    .len()
                    .checked_mul(std::mem::size_of::<T>())
                    .ok_or_else(overflow)?,
            )?;
        }
        Ok(values.into_boxed_slice())
    }

    fn probe(&mut self, path: &Path) -> Result<()> {
        let bytes = crate::store::path::input_allocation(path).map_err(graph_error)?;
        self.retain(usize::from(bytes != 0), bytes)
    }

    fn canonical(&mut self, path: &Path) -> Result<PathBuf> {
        self.probe(path)?;
        self.retain(2, crate::store::path::canonical_allocation())?;
        fs::canonicalize(path)
            .map_err(|source| io("canonicalize workspace candidate", path, source))
    }

    fn frame(&self, hash: &mut Sha256, bytes: &[u8]) -> Result<()> {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
        Ok(())
    }

    fn join(&mut self, parent: &Path, child: &Path) -> Result<PathBuf> {
        let length = parent
            .as_os_str()
            .len()
            .checked_add(child.as_os_str().len())
            .and_then(|v| v.checked_add(1))
            .ok_or_else(overflow)?;
        self.retain(1, length)?;
        let mut path = PathBuf::new();
        path.try_reserve_exact(length).map_err(allocation)?;
        path.push(parent);
        path.push(child);
        Ok(path)
    }

    fn normalize(&mut self, path: &Path) -> Result<PathBuf> {
        if !path.is_absolute() {
            return Err(invariant("absolute workspace candidate"));
        }
        self.retain(1, path.as_os_str().len())?;
        let mut result = PathBuf::new();
        result
            .try_reserve_exact(path.as_os_str().len())
            .map_err(allocation)?;
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    // Cargo's lexical normalization floors absolute parents at
                    // the root. The unnormalized query is observed separately.
                    result.pop();
                }
                _ => result.push(component.as_os_str()),
            }
        }
        Ok(result)
    }
}

impl document::Admission for Account<'_> {
    type Error = Error;
    fn admit(&mut self, request: document::Request) -> Result<()> {
        match request {
            document::Request::Items(items) => self.retain(items, 0),
            document::Request::String(_) => Ok(()),
            document::Request::Allocation { bytes, alignment } => {
                std::alloc::Layout::from_size_align(bytes, alignment).map_err(|_| overflow())?;
                self.retain(0, bytes)
            }
        }
    }
}

/// Rebuild the root set from supplied owner associations, never from a stored
/// candidate list. The generated harness already has its separate exact proof.
pub(crate) fn roots(
    authority: &[cargo::Input],
    metadata: &cargo::unit::Metadata,
    account: &mut Account<'_>,
) -> Result<Box<[PathBuf]>> {
    sorted_roots(authority, metadata.workspace_manifests()?, account)
}

fn sorted_roots<'path>(
    authority: &'path [cargo::Input],
    named: impl Iterator<Item = &'path Path>,
    account: &mut Account<'_>,
) -> Result<Box<[PathBuf]>> {
    let mut roots = Slots::new();
    let owned = authority.iter().filter_map(cargo::Input::manifest);
    for manifest in owned.chain(named) {
        insert(&mut roots, manifest, account)?;
    }
    roots.values.sort_unstable();
    account.boxed(roots)
}

/// The image of the request's own manifests, captured before Cargo reads
/// them: an edit Cargo's metadata could miss is then drift when the image is
/// [extended](Image::extend) with the manifests that metadata names.
pub(crate) fn before_metadata(
    authority: &[cargo::Input],
    account: &mut Account<'_>,
) -> Result<Image> {
    capture(
        sorted_roots(authority, std::iter::empty(), account)?,
        account,
    )
}

fn insert(paths: &mut Slots<PathBuf>, path: &Path, account: &mut Account<'_>) -> Result<()> {
    if !paths.values.iter().any(|existing| existing == path) {
        let path = account.copy(path)?;
        account.push(paths, path)?;
    }
    Ok(())
}

/// The candidate one directory above `manifest`'s, as spelled and
/// normalized: where Cargo looks next for the workspace `manifest` may
/// belong to. Each candidate that is not a workspace's root leads on to the
/// one above it once it is observed, so the search ends where Cargo's does.
fn above(paths: &mut Slots<PathBuf>, manifest: &Path, account: &mut Account<'_>) -> Result<()> {
    let Some(directory) = manifest.parent().and_then(Path::parent) else {
        return Ok(());
    };
    let candidate = account.join(directory, Path::new("Cargo.toml"))?;
    insert(paths, &candidate, account)?;
    let normalized = account.normalize(&candidate)?;
    insert(paths, &normalized, account)
}

pub(crate) fn capture(roots: Box<[PathBuf]>, account: &mut Account<'_>) -> Result<Image> {
    let scanned = scan(&roots, None, account)?;
    let candidates = account.boxed(scanned.candidates)?;
    let prefixes = account.boxed(scanned.prefixes)?;
    Ok(Image {
        roots,
        entry: scanned.entry,
        candidates,
        prefixes,
    })
}

/// The format of [`Image::record`].
const RECORD: &str = "rawr.cargo.workspace-record/v1";

/// Whether the manifests an [`Image::record`] observed are as it observed
/// them: an edit to any manifest of the workspace, below the request's own
/// path dependencies too, is a resolution kept for other files. A record
/// that doesn't decode, or can't be observed again, holds nothing.
pub(crate) fn holds(record: &[u8], limits: &cargo::Limits) -> bool {
    let Ok((format, roots, digest)) =
        serde_json::from_slice::<(Box<str>, Box<[PathBuf]>, [u8; 32])>(record)
    else {
        return false;
    };
    *format == *RECORD
        && matches!(
            observe(&roots, &mut Account::bounded(limits)),
            Ok(Entry::Workspace { digest: current, .. }) if current == digest
        )
}

pub(crate) fn observe(roots: &[PathBuf], account: &mut Account<'_>) -> Result<Entry> {
    scan(roots, None, account).map(|scanned| scanned.entry)
}

/// One observation of a workspace's manifests: its entry, the candidates
/// it read in order, and its digest before each and after the last.
struct Scanned {
    entry: Entry,
    candidates: Slots<PathBuf>,
    prefixes: Slots<[u8; 32]>,
}

fn scan(
    roots: &[PathBuf],
    captured: Option<&[PathBuf]>,
    account: &mut Account<'_>,
) -> Result<Scanned> {
    let mut hash = Sha256::new();
    account.frame(&mut hash, b"rawr.cargo.workspace-observations/v2")?;
    account.frame(&mut hash, &(roots.len() as u64).to_le_bytes())?;
    let mut candidates = Slots::new();
    for root in roots {
        account.frame(&mut hash, root.as_os_str().as_encoded_bytes())?;
        if captured.is_none() {
            insert(&mut candidates, root, account)?;
            let normalized = account.normalize(root)?;
            insert(&mut candidates, &normalized, account)?;
        }
    }
    if let Some(captured) = captured {
        for path in captured {
            let path = account.copy(path)?;
            account.push(&mut candidates, path)?;
        }
    }
    let mut watched = Slots::new();
    let mut expanded = Slots::new();
    let mut prefixes = Slots::new();
    let mut cursor = 0;
    while cursor < candidates.values.len() {
        account.push(&mut prefixes, hash.clone().finalize().into())?;
        let path = account.copy(&candidates.values[cursor])?;
        cursor += 1;
        account.frame(&mut hash, b"candidate")?;
        account.frame(&mut hash, path.as_os_str().as_encoded_bytes())?;
        insert(&mut watched, &path, account)?;
        // Include each association along the queried spelling, not merely the
        // old resolved target. No directory contents are traversed.
        for component in path.ancestors() {
            account.probe(component)?;
            match fs::symlink_metadata(component) {
                Ok(metadata) => {
                    account.frame(&mut hash, &[kind(&metadata)])?;
                    if metadata.file_type().is_symlink() {
                        insert(&mut watched, component, account)?;
                        match account.canonical(component) {
                            Ok(canonical) => {
                                account
                                    .frame(&mut hash, canonical.as_os_str().as_encoded_bytes())?;
                                insert(&mut watched, &canonical, account)?;
                            }
                            Err(Error::Io { source, .. }) if absent(&source).is_some() => {
                                account
                                    .frame(&mut hash, absent(&source).expect("matched absence"))?
                            }
                            Err(error) => return Err(error),
                        }
                    }
                }
                Err(source) if absent(&source).is_some() => {
                    insert(&mut watched, component, account)?;
                    account.frame(&mut hash, absent(&source).expect("matched absence"))?
                }
                Err(source) => {
                    return Err(io(
                        "inspect workspace candidate association",
                        component,
                        source,
                    ))
                }
            }
        }
        account.probe(&path)?;
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(source) if absent(&source).is_some() => {
                account.frame(&mut hash, absent(&source).expect("matched absence"))?;
                if captured.is_none() {
                    above(&mut candidates, &path, account)?;
                }
                continue;
            }
            Err(source) => return Err(io("inspect workspace candidate", &path, source)),
        };
        account.frame(&mut hash, &[kind(&metadata)])?;
        let canonical = account.canonical(&path)?;
        account.frame(&mut hash, canonical.as_os_str().as_encoded_bytes())?;
        insert(&mut watched, &canonical, account)?;
        if !metadata.is_file() {
            if captured.is_none() {
                above(&mut candidates, &path, account)?;
                above(&mut candidates, &canonical, account)?;
            }
            continue;
        }
        let bytes = read(&canonical, account)?;
        account.frame(&mut hash, &bytes)?;
        // Identical bytes and every queried/canonical association imply the
        // original derived closure still applies. Any mutation changes this
        // observation and refuses the original Image; only initial derivation
        // and full replay interpret redirects and discover additional roots.
        if captured.is_some() {
            continue;
        }
        let document = match std::str::from_utf8(&bytes) {
            Ok(source) => match document::parse(
                source,
                u32::try_from(account.limits.depth).unwrap_or(u32::MAX),
                account,
            ) {
                Ok(document) => Some(document),
                Err(document::Error::Invalid(diagnostic)) => match diagnostic.tag {
                    document::Tag::Depth => {
                        return Err(Error::Limit {
                            resource: "workspace TOML depth",
                            limit: account.limits.depth.min(80),
                            actual: account.limits.depth.min(80) + 1,
                        })
                    }
                    document::Tag::Internal => {
                        return Err(graph_error(node::Error::State(
                            "workspace TOML parser state",
                        )))
                    }
                    _ => None,
                },
                Err(document::Error::Admission(error)) => return Err(error),
                Err(document::Error::Allocation | document::Error::Overflow) => {
                    return Err(overflow())
                }
            },
            Err(_) => None,
        };
        // A manifest that declares `[workspace]` is a workspace's root: Cargo
        // searches no directory above it, for itself or for a member below
        // it. One that excludes paths may not hold the member, so the search
        // goes on above it, as it does above any other manifest (#894).
        let root = document.as_ref().is_some_and(|document| {
            match document
                .table()
                .get("workspace")
                .map(document::Spanned::value)
            {
                Some(document::Value::Table(workspace)) => workspace.get("exclude").is_none(),
                Some(_) => true,
                None => false,
            }
        });
        if !root {
            above(&mut candidates, &path, account)?;
            above(&mut candidates, &canonical, account)?;
        }
        let Some(document) = document else {
            continue;
        };
        let package = document
            .table()
            .get("package")
            .or_else(|| document.table().get("project"));
        let Some(document::Value::Table(package)) = package.map(document::Spanned::value) else {
            continue;
        };
        let Some(document::Value::String(redirect)) =
            package.get("workspace").map(document::Spanned::value)
        else {
            continue;
        };
        // Observe every queried spelling above, then suppress only an already
        // expanded association. Raw redirects containing symlink/.. must reach
        // their physical target before lexical aliases can deduplicate them.
        // Normalized query + physical parent + target bounds spelling cycles
        // without conflating manifest symlinks with directory symlinks.
        let normalized = account.normalize(&path)?;
        let parent = path
            .parent()
            .expect("a workspace candidate names a `Cargo.toml` in a directory");
        let parent = account.canonical(parent)?;
        if expanded.values.iter().any(|(query, directory, target)| {
            query == &normalized && directory == &parent && target == &canonical
        }) {
            continue;
        }
        let target = account.copy(&canonical)?;
        account.push(&mut expanded, (normalized, parent, target))?;
        for spelling in [&path, &canonical] {
            let parent = spelling
                .parent()
                .expect("a workspace candidate names a `Cargo.toml` in a directory");
            let directory = account.join(parent, Path::new(redirect.as_str()))?;
            let manifest = account.join(&directory, Path::new("Cargo.toml"))?;
            // Its ancestors are observed once it is read, unless it is a
            // workspace's root.
            insert(&mut candidates, &manifest, account)?;
            let normalized = account.normalize(&manifest)?;
            insert(&mut candidates, &normalized, account)?;
        }
    }
    account.push(&mut prefixes, hash.clone().finalize().into())?;
    // In the bytes' order, as the stored entry reads them back: a path's
    // own order compares components, so `a/rawr/x` comes before
    // `a/rawr-home` there and after it as text.
    watched.values.sort_unstable_by(|left, right| {
        left.as_os_str()
            .as_encoded_bytes()
            .cmp(right.as_os_str().as_encoded_bytes())
    });
    let mut paths = Slots::new();
    for path in watched.values {
        let text = path.to_str().ok_or_else(|| Error::Input {
            code: "workspace-path-utf8",
            reason: path.display().to_string().into_boxed_str(),
        })?;
        account.retain(1, text.len())?;
        let mut value = String::new();
        value.try_reserve_exact(text.len()).map_err(allocation)?;
        value.push_str(text);
        account.push(&mut paths, value.into_boxed_str())?;
    }
    Ok(Scanned {
        entry: Entry::Workspace {
            paths: account.boxed(paths)?,
            digest: hash.finalize().into(),
        },
        candidates,
        prefixes,
    })
}

impl Image {
    /// The image of every manifest behind `metadata`, once this one, taken
    /// before Cargo read them, still holds: a manifest edited while Cargo
    /// read it is drift, never metadata paired with the edited bytes.
    pub(crate) fn extend(
        self,
        authority: &[cargo::Input],
        metadata: &cargo::unit::Metadata,
        account: &mut Account<'_>,
    ) -> Result<Self> {
        self.require(account)?;
        capture(roots(authority, metadata, account)?, account)
    }

    /// Whether the manifests this image read are as it read them: the
    /// first that differs is [`Error::Changed`].
    pub(crate) fn require(&self, account: &mut Account<'_>) -> Result<()> {
        let current = scan(&self.roots, Some(&self.candidates), account)?;
        if current.entry == self.entry {
            return Ok(());
        }
        // The digest before candidate `n + 1` is the first to differ when
        // candidate `n` changed.
        let changed = current
            .prefixes
            .values
            .iter()
            .zip(&self.prefixes)
            .position(|(current, captured)| current != captured)
            .and_then(|first| first.checked_sub(1))
            .and_then(|candidate| self.candidates.get(candidate))
            .or_else(|| self.roots.first())
            .expect("a workspace image has the request's own manifest as a root");
        Err(Error::Changed {
            path: changed.clone(),
        })
    }

    /// The bytes a persisted resolution keeps of this image: the manifests
    /// it starts from and its digest of what it observed, which [`holds`]
    /// reads back.
    pub(crate) fn record(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&(RECORD, &self.roots, self.digest()?)).map_err(|error| Error::Schema {
            document: "workspace record",
            reason: error.to_string().into_boxed_str(),
        })
    }

    pub(crate) fn digest(&self) -> Result<&[u8; 32]> {
        match &self.entry {
            Entry::Workspace { digest, .. } => Ok(digest),
            _ => Err(invariant("workspace image entry")),
        }
    }
}

fn read(path: &Path, account: &mut Account<'_>) -> Result<Vec<u8>> {
    account.probe(path)?;
    let mut file =
        File::open(path).map_err(|source| io("open workspace manifest", path, source))?;
    let length = file
        .metadata()
        .map_err(|source| io("inspect workspace manifest handle", path, source))?
        .len();
    let length = usize::try_from(length).map_err(|_| overflow())?;
    // A workspace manifest is a manifest-class read: the physical bound is
    // `process::TAIL_BYTES`; the manifest target only records.
    super::limited(
        "workspace manifest bytes",
        crate::process::TAIL_BYTES,
        length,
    )?;
    account.retain(1, length)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(allocation)?;
    bytes.resize(length, 0);
    let mut offset = 0;
    while offset < bytes.len() {
        match file.read(&mut bytes[offset..]) {
            Ok(0) => return Err(changed(path)),
            Ok(amount) => offset += amount,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(source) => return Err(io("read workspace manifest", path, source)),
        }
    }
    let mut extra = [0];
    let extra = loop {
        match file.read(&mut extra) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            result => {
                break result
                    .map_err(|source| io("observe workspace manifest EOF", path, source))?
            }
        }
    };
    if extra != 0 {
        return Err(changed(path));
    }
    Ok(bytes)
}

fn kind(metadata: &fs::Metadata) -> u8 {
    if metadata.file_type().is_symlink() {
        3
    } else if metadata.is_file() {
        1
    } else if metadata.is_dir() {
        2
    } else {
        4
    }
}
fn absent(source: &std::io::Error) -> Option<&'static [u8]> {
    match source.kind() {
        std::io::ErrorKind::NotFound => Some(b"missing"),
        std::io::ErrorKind::NotADirectory => Some(b"not-directory"),
        _ => None,
    }
}
fn graph_error(source: node::Error) -> Error {
    Error::Node {
        operation: "workspace input observation",
        source: Box::new(source),
    }
}
fn allocation(source: std::collections::TryReserveError) -> Error {
    Error::Allocation {
        operation: "workspace input observation",
        source,
    }
}
fn overflow() -> Error {
    graph_error(node::Error::Allocation)
}
fn changed(path: &Path) -> Error {
    Error::Changed {
        path: path.to_path_buf(),
    }
}
fn invariant(field: &'static str) -> Error {
    Error::Invariant {
        command: "workspace input observation",
        field,
    }
}
fn io(operation: &'static str, path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests;
