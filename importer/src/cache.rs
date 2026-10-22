//! Freshness publication and conservative undeclared-external-state policy.

use crate::api;
use crate::cargo;
use crate::input;
use crate::{Error, Result};

#[cfg(test)]
std::thread_local! {
    static PUBLICATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub mod entry;
pub mod eviction;
pub mod key;
pub mod lease;
pub mod pins;
pub mod seal;

/// Fixed-path generated-runner cache authority.
pub struct Writer {
    root: std::path::PathBuf,
}

/// Exact inspectable state of one generated-runner cache directory.
#[derive(Debug, Eq, PartialEq)]
pub struct Entry {
    name: Box<str>,
    bytes: u64,
    sealed: bool,
    pinned: bool,
}

impl Writer {
    /// Bind a writer to `<project>/.rawr/cache/runners`.
    pub fn new(project: &std::path::Path) -> std::result::Result<Self, entry::Error> {
        let project = std::fs::canonicalize(project).map_err(|source| entry::Error::Io {
            operation: "canonicalize cache project",
            path: project.to_path_buf(),
            source,
        })?;
        let root = project.join(".rawr/cache/runners");
        std::fs::create_dir_all(&root).map_err(|source| entry::Error::Io {
            operation: "create runner cache",
            path: root.clone(),
            source,
        })?;
        Ok(Self { root })
    }

    /// Borrow the fixed cache root.
    #[must_use]
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub(crate) fn lease(
        &self,
    ) -> std::result::Result<fd_lock::RwLock<std::fs::File>, entry::Error> {
        lease::open(&self.root)
    }

    /// Inspect cache entries without granting lookup or deletion authority.
    pub fn inspect(&self) -> std::result::Result<Vec<Entry>, entry::Error> {
        let mut records = Vec::new();
        for path in entry::directories(&self.root)? {
            let name = path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .ok_or_else(|| entry::Error::Shape(path.clone()))?;
            let mut retained = String::new();
            retained
                .try_reserve_exact(name.len())
                .map_err(|_| entry::Error::Quota)?;
            retained.push_str(name);
            records.push(Entry {
                name: retained.into_boxed_str(),
                bytes: entry::bytes(&path)?,
                sealed: seal::exists(&path),
                pinned: pins::present(&path)?,
            });
        }
        records.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        Ok(records)
    }

    /// Delete one exact cache key if it is not pinned.
    pub fn delete(
        &self,
        key: crate::bridge::BuildRequestKey,
    ) -> std::result::Result<bool, entry::Error> {
        let mut lease = self.lease()?;
        let _guard = lease.try_write().map_err(|source| {
            if source.kind() == std::io::ErrorKind::WouldBlock {
                entry::Error::Busy
            } else {
                entry::Error::Io {
                    operation: "acquire runner cache deletion lease",
                    path: self.root.join(".writer.lock"),
                    source,
                }
            }
        })?;
        let path = entry::path(&self.root, key);
        if !path.exists() {
            return Ok(false);
        }
        if pins::present(&path)? {
            return Err(entry::Error::PinnedFull);
        }
        std::fs::remove_dir_all(&path).map_err(|source| entry::Error::Io {
            operation: "delete runner cache entry",
            path,
            source,
        })?;
        Ok(true)
    }
}

impl Entry {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    #[must_use]
    pub const fn sealed(&self) -> bool {
        self.sealed
    }

    #[must_use]
    pub const fn pinned(&self) -> bool {
        self.pinned
    }
}

/// Required warm-path handling when a build script is known to read undeclared state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Undeclared {
    /// Invoke Cargo; no Rawr input proof is claimed for the warm decision.
    InvokeCargo,
    /// Reject warm reuse instead of invoking Cargo.
    Reject,
}

/// The only non-reuse result available for explicitly undeclared external state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    /// Cargo must re-establish cold-build authority.
    InvokeCargo,
}

/// Classify explicitly undeclared external state without pretending to discover it.
pub fn undeclared_external(policy: Undeclared) -> Result<Decision> {
    match policy {
        Undeclared::InvokeCargo => Ok(Decision::InvokeCargo),
        Undeclared::Reject => Err(Error::Input {
            code: "non-hermetic-external-state",
            reason: "warm reuse was rejected because external state is undeclared".into(),
        }),
    }
}

pub(crate) fn publish(
    graph: api::Graph<api::Captured>,
    baseline: input::Baseline,
    limits: &cargo::Limits,
) -> Result<api::Graph<api::Extracted>> {
    #[cfg(test)]
    PUBLICATIONS.with(|count| count.set(count.get().saturating_add(1)));
    let proof = baseline.prove(limits)?;
    graph.publish(proof)
}

#[cfg(test)]
pub(crate) fn reset_publications() {
    PUBLICATIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn publications() -> usize {
    PUBLICATIONS.with(std::cell::Cell::get)
}
