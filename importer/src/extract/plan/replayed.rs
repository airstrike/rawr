//! Each shared table's replay for one demand, kept for the next round that
//! reads the same table for the same demand (#1845).
//!
//! A table replays to the same body whenever its bytes, its crate's unit
//! and aliases and its part of the demand are the same. A round or a save
//! that changes none of them takes the body the last one replayed: it reads,
//! hashes and parses nothing of the table. Inputs are still checked first,
//! each round, as for any replay.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest as _, Sha256};

use crate::extract::engine;

/// The replays an owner keeps, by shared key: the latest of each.
///
/// A replay is shared, not copied, between the owner that keeps it and the
/// round that reads it, as a body can be megabytes.
#[derive(Clone, Default)]
pub(crate) struct Tables(BTreeMap<Box<str>, Arc<Table>>);

impl Tables {
    /// The replay kept for `key` that answers `question`.
    pub(super) fn answer(&self, key: &str, question: &Question) -> Option<Arc<Table>> {
        self.0
            .get(key)
            .filter(|table| table.question == *question)
            .map(Arc::clone)
    }

    /// Keep `table` for `key`, in place of any earlier one.
    pub(super) fn keep(&mut self, key: &str, table: Arc<Table>) {
        self.0.insert(key.into(), table);
    }

    /// Keep only the replays of `keys`: a table no plan names again is
    /// not held.
    pub(super) fn retain(&mut self, keys: &[&str]) {
        self.0.retain(|key, _| keys.contains(&&**key));
    }
}

/// What a table's replay is a function of: the table, by its directory and
/// stamp; its crate's unit and aliases; and its part of the demand, none
/// for a whole walk.
#[derive(Eq, PartialEq)]
pub(super) struct Question {
    directory: PathBuf,
    stamp: crate::bindings::Stamp,
    unit_key: Box<str>,
    aliases: Vec<String>,
    seed: Option<Vec<Vec<String>>>,
}

impl Question {
    pub(super) fn new(
        directory: &Path,
        stamp: crate::bindings::Stamp,
        unit_key: &str,
        aliases: &[String],
        seed: engine::Seed<'_>,
    ) -> Self {
        Self {
            directory: directory.to_path_buf(),
            stamp,
            unit_key: unit_key.into(),
            aliases: aliases.to_vec(),
            seed: match seed {
                engine::Seed::Whole => None,
                engine::Seed::Paths(paths) => Some(paths.to_vec()),
            },
        }
    }
}

/// One table replayed for one demand.
pub(super) struct Table {
    question: Question,
    /// The protocol body the replay gave.
    body: Vec<u8>,
    body_digest: [u8; 32],
    /// The crates it reached, sysroot and external, in order.
    crates: Vec<engine::Crate>,
    /// The table's length and hex digest.
    length: usize,
    digest: String,
}

impl Table {
    /// The replay of `question`'s table, whose bytes are `table` with hex
    /// `digest`, over `body` reaching `crates`.
    pub(super) fn new(
        question: Question,
        table: &[u8],
        digest: String,
        body: Vec<u8>,
        crates: Vec<engine::Crate>,
    ) -> Self {
        Self {
            question,
            body_digest: Sha256::digest(&body).into(),
            body,
            crates,
            length: table.len(),
            digest,
        }
    }

    pub(super) fn body(&self) -> &[u8] {
        &self.body
    }

    pub(super) const fn body_digest(&self) -> &[u8; 32] {
        &self.body_digest
    }

    pub(super) fn crates(&self) -> &[engine::Crate] {
        &self.crates
    }

    pub(super) const fn length(&self) -> usize {
        self.length
    }

    pub(super) fn digest(&self) -> &str {
        &self.digest
    }
}
