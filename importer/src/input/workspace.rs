//! Conservative manifest determinants, captured before Cargo consumes them.
//!
//! This is deliberately not a Cargo workspace selector. Every owner/ancestor
//! candidate and recursively named workspace is observed, including unrelated
//! malformed manifests. Cargo's actual successful postcheck supplies semantics.

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
}

pub(crate) trait Storage {
    fn retain(&mut self, items: usize, bytes: usize) -> Result<()>;
    /// Record a measured amount against an accounting target on the
    /// original account; never refuses.
    fn exceeded(&mut self, resource: &'static str, target: usize, actual: usize);
}

impl Storage for cargo::RequestAllocation {
    fn retain(&mut self, items: usize, bytes: usize) -> Result<()> {
        self.retained(items, bytes)
    }
    fn exceeded(&mut self, resource: &'static str, target: usize, actual: usize) {
        cargo::RequestAllocation::exceeded(self, resource, target, actual);
    }
}

impl Storage for Allocation {
    fn retain(&mut self, items: usize, bytes: usize) -> Result<()> {
        self.items(items)?;
        self.bytes(bytes)
    }
    fn exceeded(&mut self, resource: &'static str, target: usize, actual: usize) {
        Allocation::exceeded(self, resource, target, actual);
    }
}

pub(crate) struct Account<'a> {
    storage: &'a mut dyn Storage,
    graph: &'a node::GraphBudget,
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
    pub(crate) fn borrow(
        storage: &'a mut dyn Storage,
        graph: &'a node::GraphBudget,
        limits: &'a cargo::Limits,
    ) -> Self {
        Self {
            storage,
            graph,
            limits,
        }
    }

    // Copies of admitted bytes are owned once by the original storage
    // account; probes and hashing keep their own graph axes below.
    fn retain(&mut self, items: usize, bytes: usize) -> Result<()> {
        self.storage.retain(items, bytes)
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
        self.graph.paths(1).map_err(graph_error)?;
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
        self.graph
            .hash(bytes.len().checked_add(8).ok_or_else(overflow)?)
            .map_err(graph_error)?;
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
            return Err(drift("workspace root is not absolute"));
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
            document::Request::String(bytes) => {
                self.storage.exceeded(
                    "workspace TOML string bytes",
                    self.limits.string_bytes,
                    bytes,
                );
                Ok(())
            }
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
    let mut roots = Slots::new();
    for input in authority {
        if let Some(manifest) = input.manifest() {
            insert(&mut roots, manifest, account)?;
        }
    }
    for manifest in metadata.workspace_manifests()? {
        insert(&mut roots, manifest, account)?;
    }
    roots.values.sort_unstable();
    account.boxed(roots)
}

fn insert(paths: &mut Slots<PathBuf>, path: &Path, account: &mut Account<'_>) -> Result<()> {
    if !paths.values.iter().any(|existing| existing == path) {
        let path = account.copy(path)?;
        account.push(paths, path)?;
    }
    Ok(())
}

fn ancestors(paths: &mut Slots<PathBuf>, manifest: &Path, account: &mut Account<'_>) -> Result<()> {
    insert(paths, manifest, account)?;
    let normalized = account.normalize(manifest)?;
    insert(paths, &normalized, account)?;
    for directory in manifest.ancestors().skip(2) {
        let candidate = account.join(directory, Path::new("Cargo.toml"))?;
        insert(paths, &candidate, account)?;
        let normalized = account.normalize(&candidate)?;
        insert(paths, &normalized, account)?;
    }
    Ok(())
}

pub(crate) fn capture(roots: Box<[PathBuf]>, account: &mut Account<'_>) -> Result<Image> {
    let (entry, candidates) = scan(&roots, None, account)?;
    let candidates = account.boxed(candidates)?;
    Ok(Image {
        roots,
        entry,
        candidates,
    })
}

pub(crate) fn observe(roots: &[PathBuf], account: &mut Account<'_>) -> Result<Entry> {
    scan(roots, None, account).map(|(entry, _)| entry)
}

fn scan(
    roots: &[PathBuf],
    captured: Option<&[PathBuf]>,
    account: &mut Account<'_>,
) -> Result<(Entry, Slots<PathBuf>)> {
    let mut hash = Sha256::new();
    account.frame(&mut hash, b"rawr.cargo.workspace-observations/v1")?;
    account.frame(&mut hash, &(roots.len() as u64).to_le_bytes())?;
    let mut candidates = Slots::new();
    for root in roots {
        account.frame(&mut hash, root.as_os_str().as_encoded_bytes())?;
        if captured.is_none() {
            ancestors(&mut candidates, root, account)?;
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
    let mut cursor = 0;
    while cursor < candidates.values.len() {
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
                continue;
            }
            Err(source) => return Err(io("inspect workspace candidate", &path, source)),
        };
        account.frame(&mut hash, &[kind(&metadata)])?;
        let canonical = account.canonical(&path)?;
        account.frame(&mut hash, canonical.as_os_str().as_encoded_bytes())?;
        insert(&mut watched, &canonical, account)?;
        if !metadata.is_file() {
            continue;
        }
        if captured.is_none() {
            ancestors(&mut candidates, &canonical, account)?;
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
        let Ok(source) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let document = match document::parse(
            source,
            u32::try_from(account.limits.depth).unwrap_or(u32::MAX),
            account,
        ) {
            Ok(document) => document,
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
                _ => continue,
            },
            Err(document::Error::Admission(error)) => return Err(error),
            Err(document::Error::Allocation | document::Error::Overflow) => return Err(overflow()),
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
            .ok_or_else(|| drift("workspace manifest lacks parent"))?;
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
                .ok_or_else(|| drift("workspace manifest lacks parent"))?;
            let directory = account.join(parent, Path::new(redirect.as_str()))?;
            let manifest = account.join(&directory, Path::new("Cargo.toml"))?;
            ancestors(&mut candidates, &manifest, account)?;
        }
    }
    watched.values.sort_unstable();
    let mut paths = Slots::new();
    for path in watched.values {
        let text = path
            .to_str()
            .ok_or_else(|| drift("workspace path is not UTF-8"))?;
        account.storage.exceeded(
            "workspace path bytes",
            account.limits.string_bytes,
            text.len(),
        );
        account.retain(1, text.len())?;
        let mut value = String::new();
        value.try_reserve_exact(text.len()).map_err(allocation)?;
        value.push_str(text);
        account.push(&mut paths, value.into_boxed_str())?;
    }
    Ok((
        Entry::Workspace {
            paths: account.boxed(paths)?,
            digest: hash.finalize().into(),
        },
        candidates,
    ))
}

impl Image {
    pub(crate) fn require(&self, account: &mut Account<'_>) -> Result<()> {
        if scan(&self.roots, Some(&self.candidates), account)?.0 != self.entry {
            return Err(drift("Cargo workspace manifest closure changed"));
        }
        Ok(())
    }

    pub(crate) fn digest(&self) -> Result<&[u8; 32]> {
        match &self.entry {
            Entry::Workspace { digest, .. } => Ok(digest),
            _ => Err(drift("invalid workspace image")),
        }
    }
}

fn read(path: &Path, account: &mut Account<'_>) -> Result<Vec<u8>> {
    account.probe(path)?;
    let mut file =
        File::open(path).map_err(|source| io("open workspace manifest", path, source))?;
    account.graph.paths(1).map_err(graph_error)?;
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
    account.storage.exceeded(
        "workspace manifest bytes",
        account.limits.manifest_bytes,
        length,
    );
    account.retain(1, length)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(allocation)?;
    bytes.resize(length, 0);
    let mut offset = 0;
    while offset < bytes.len() {
        account.graph.paths(1).map_err(graph_error)?;
        match file.read(&mut bytes[offset..]) {
            Ok(0) => return Err(drift("workspace manifest shortened during read")),
            Ok(amount) => offset += amount,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(source) => return Err(io("read workspace manifest", path, source)),
        }
    }
    let mut extra = [0];
    let extra = loop {
        account.graph.paths(1).map_err(graph_error)?;
        match file.read(&mut extra) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            result => {
                break result
                    .map_err(|source| io("observe workspace manifest EOF", path, source))?
            }
        }
    };
    if extra != 0 {
        return Err(drift("workspace manifest changed during read"));
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
fn drift(field: &'static str) -> Error {
    Error::ResolutionDrift {
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
