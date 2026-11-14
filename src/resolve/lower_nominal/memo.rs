//! What a compiler's interfaces made, each by the digest of what it was
//! made from, so the next interface makes only what reads something new:
//! this compile's, the compile before's, and, on the compiler's first, what
//! an earlier runner kept on disk (#1755, #1899).

use super::disk::{self, Disk};
use crate::Engine;
use std::collections::BTreeMap;

/// The file of a memo holds each value's own key.
const KEY: [u8; 32] = [0; 32];

/// Values by the digest of what each was made from.
pub(super) struct Memo<V> {
    /// The runner's file that holds them.
    kind: disk::Kind,
    /// What this compile made or reused.
    kept: BTreeMap<[u8; 32], V>,
    /// What the compile before it made or reused.
    earlier: BTreeMap<[u8; 32], V>,
    /// Whether `earlier` holds what the disk held.
    loaded: bool,
    /// Whether this compile made a value the disk doesn't hold.
    made: bool,
}

impl<V: Clone + serde::Serialize + serde::de::DeserializeOwned> Memo<V> {
    /// None made yet; a runner keeps them in its file of `kind`.
    pub(super) const fn new(kind: disk::Kind) -> Self {
        Self {
            kind,
            kept: BTreeMap::new(),
            earlier: BTreeMap::new(),
            loaded: false,
            made: false,
        }
    }

    /// What was made at `key`: by this compile, by the compile before, or,
    /// on the compiler's first, by an earlier runner that kept it in
    /// `disk`.
    fn find(&mut self, engine: &Engine, disk: Option<&mut Disk>, key: &[u8; 32]) -> Option<V> {
        if let Some(value) = self.kept.get(key) {
            return Some(value.clone());
        }
        if !self.loaded
            && let Some(disk) = disk
        {
            self.loaded = true;
            let stored = disk.read::<BTreeMap<[u8; 32], V>>(engine, self.entry());
            for (key, value) in stored.into_iter().flatten() {
                self.earlier.entry(key).or_insert(value);
            }
        }
        let value = self.earlier.remove(key)?;
        self.kept.insert(*key, value.clone());
        Some(value)
    }

    /// What was made at `key`, else `make`'s, kept at `key`.
    pub(super) fn get_or_make(
        &mut self,
        engine: &Engine,
        disk: Option<&mut Disk>,
        key: &[u8; 32],
        make: impl FnOnce() -> Option<V>,
    ) -> Option<V> {
        if let Some(value) = self.find(engine, disk, key) {
            return Some(value);
        }
        let value = make()?;
        self.keep(*key, value.clone());
        Some(value)
    }

    /// Keep `value`, made at `key`.
    fn keep(&mut self, key: [u8; 32], value: V) {
        self.kept.insert(key, value);
        self.made = true;
    }

    /// A compile begins: what the last one kept is its earlier.
    pub(super) fn begin(&mut self) {
        if !self.kept.is_empty() {
            self.earlier = std::mem::take(&mut self.kept);
        }
    }

    /// The compile succeeded: `disk` holds what it kept, when it made
    /// something.
    pub(super) fn succeeded(&mut self, engine: &Engine, disk: Option<&mut Disk>) {
        if let Some(disk) = disk
            && std::mem::take(&mut self.made)
        {
            disk.write(engine, self.entry(), &self.kept);
        }
    }

    /// What this compile kept.
    #[cfg(test)]
    pub(super) fn kept(&self) -> &BTreeMap<[u8; 32], V> {
        &self.kept
    }

    /// What `disk` holds of them.
    #[cfg(test)]
    pub(super) fn stored(&self, engine: &Engine, disk: &mut Disk) -> Option<BTreeMap<[u8; 32], V>> {
        disk.read(engine, self.entry())
    }

    const fn entry(&self) -> disk::Entry<'static> {
        disk::Entry {
            identity: "",
            kind: self.kind,
            key: &KEY,
        }
    }
}
