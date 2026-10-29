//! Recorded unit tables, one per selected crate (`engine::Table`). A table's
//! key is the crate alone: its package, its unit's key (which covers its
//! features, profile, target and its dependencies' keys), the compiler and
//! the extraction's formats. Never the demand or the probe's aliases, so
//! any program that reaches the crate reads it. A crate from a registry or
//! a git commit cannot change under its key; any other crate's key takes
//! its crate hash too, which an edit to it or to its dependencies changes.
//!
//! The importer names the key of each crate it shares per machine
//! (`extract::shared`) in `keys.json`, by unit correlation; such a crate's
//! table is kept under that key, which the importer publishes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::engine::{self, Memo, Root, Table};
use super::{Invocation, PROTOCOL_SCHEMA};

pub(super) struct Tables {
    directory: PathBuf,
    /// Shared keys by unit correlation, as the importer named them.
    shared: BTreeMap<String, String>,
}

/// What a table holds, beside it in `<key>.served`: the demands of one
/// session it was made from alone. A table with no such record, as one
/// seeded from rawr's home, holds units of demands nobody here knows.
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Served {
    session: String,
    demand: BTreeSet<Vec<String>>,
    whole: bool,
    large: bool,
}

impl Served {
    pub(super) fn new(session: &str, demand: &[Vec<String>], memo: &Memo) -> Self {
        Self {
            session: session.to_owned(),
            demand: demand.iter().cloned().collect(),
            whole: memo.whole,
            large: memo.large,
        }
    }

    /// Whether the crate's whole walk passed its bound.
    pub(super) const fn large(&self) -> bool {
        self.large
    }

    /// This record with `other`'s demands too, when both are of one
    /// session.
    pub(super) fn merged(mut self, other: &Self) -> Option<Self> {
        (self.session == other.session).then(|| {
            self.demand.extend(other.demand.iter().cloned());
            self.whole |= other.whole;
            self.large |= other.large;
            self
        })
    }
}

/// Beside a table, what it holds.
const SERVED: &str = "served";

/// The importer's shared keys, beside the tables.
const KEYS: &str = "keys.json";

impl Tables {
    pub(super) fn open(directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory)
            .map_err(|error| format!("failed to create the extraction table directory: {error}"))?;
        let shared = fs::read(directory.join(KEYS))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Ok(Self {
            directory: directory.to_path_buf(),
            shared,
        })
    }

    /// What `key`'s table holds, when it was made from known demands alone.
    pub(super) fn served(&self, key: &str) -> Option<Served> {
        let bytes = fs::read(self.directory.join(format!("{key}.{SERVED}"))).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Whether `demand` in `session` strictly grows every demand `key`'s
    /// table was made from: the table cannot answer it, and a live walk of
    /// it records every unit the table holds, so the table is neither read
    /// nor merged. The answer is that record.
    pub(super) fn outgrown(
        &self,
        key: &str,
        session: &str,
        demand: &[Vec<String>],
        used: &mut Vec<String>,
    ) -> Option<Served> {
        let served = self.served(key)?;
        let demand = demand.iter().collect::<BTreeSet<_>>();
        let outgrown = served.session == session
            && !served.whole
            && served.demand.len() < demand.len()
            && served.demand.iter().all(|path| demand.contains(path));
        outgrown.then(|| {
            used.push(key.to_owned());
            served
        })
    }

    /// The key of `invocation`'s table: the importer's shared key, or this
    /// crate's own.
    pub(super) fn key_for(&self, invocation: &Invocation, crate_hash: &str) -> String {
        match self.shared.get(&invocation.unit_correlation) {
            Some(key) => key.clone(),
            None => Self::key(invocation, crate_hash),
        }
    }

    pub(super) fn key(invocation: &Invocation, crate_hash: &str) -> String {
        let mut digest = Sha256::new();
        let mut frame = |bytes: &[u8]| {
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        };
        frame(b"rawr.extract.table/v1");
        frame(&engine::TABLE_SCHEMA.to_le_bytes());
        frame(&PROTOCOL_SCHEMA.to_le_bytes());
        let package = &invocation.package;
        for value in [
            &invocation.compiler_commit,
            &invocation.host,
            &invocation.selected_target,
            &package.cargo_id,
            &package.name,
            &package.version,
            &package.source,
            &invocation.unit_key,
            &invocation.target,
            &invocation.target_kind,
            &invocation.crate_name,
            &invocation.edition,
            &invocation.profile,
            &invocation.artifact.profile,
        ] {
            frame(value.as_bytes());
        }
        for list in [
            &invocation.dependencies,
            &invocation.target_kinds,
            &invocation.crate_types,
            &invocation.features,
        ] {
            frame(&(list.len() as u64).to_le_bytes());
            for value in list {
                frame(value.as_bytes());
            }
        }
        if !immutable(&invocation.package.source) {
            frame(crate_hash.as_bytes());
        }
        engine::hex(&digest.finalize())
    }

    /// The crate's recorded units, when a table of this format for this very
    /// crate is kept. A table that fails to read is extracted again.
    pub(super) fn load(&self, key: &str, root: &Root, used: &mut Vec<String>) -> Option<Memo> {
        used.push(key.to_owned());
        let bytes = fs::read(self.directory.join(key)).ok()?;
        let table: Table = serde_json::from_slice(&bytes).ok()?;
        (table.schema == engine::TABLE_SCHEMA && table.root == *root)
            .then(|| Memo::from_table(table))
    }

    /// Keep a crate's table, replacing the one kept before, with what it
    /// was made from when that is known. Failure only costs a later
    /// extraction.
    pub(super) fn store(&self, key: &str, root: &Root, memo: &Memo, served: Option<Served>) {
        let served_path = self.directory.join(format!("{key}.{SERVED}"));
        // The record goes first, so a table never outlives one that says
        // less than it holds.
        let _ = fs::remove_file(&served_path);
        let temporary = self
            .directory
            .join(format!(".{key}.tmp-{}", std::process::id()));
        let written = (|| -> std::io::Result<()> {
            let mut file = std::io::BufWriter::new(fs::File::create(&temporary)?);
            serde_json::to_writer(&mut file, &memo.to_table(root.clone()))?;
            file.flush()?;
            fs::rename(&temporary, self.directory.join(key))
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
            return;
        }
        if let Some(served) = served
            && let Ok(bytes) = serde_json::to_vec(&served)
        {
            let _ = fs::write(&served_path, bytes);
        }
    }

    /// Retire tables this run did not ask for, so the directory holds one
    /// probe's worth.
    pub(super) fn prune(&self, used: &[String]) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let key = name.strip_suffix(&format!(".{SERVED}")).unwrap_or(name);
            if key.len() == 64
                && key
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
                && !used.iter().any(|used| used == key)
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// A source whose contents its name fixes: a registry's package, or a git
/// dependency at its commit.
fn immutable(source: &str) -> bool {
    source.starts_with("registry+")
        || source.starts_with("sparse+")
        || (source.starts_with("git+")
            && source.rsplit_once('#').is_some_and(|(_, commit)| {
                commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
            }))
}

#[cfg(test)]
mod tests {
    use super::super::engine::{Memo, Root};
    use super::{Served, Tables, SERVED};

    fn path(segments: &[&str]) -> Vec<String> {
        segments
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect()
    }

    #[test]
    fn a_grown_demand_of_one_session_outgrows_its_table_and_nothing_else_does() {
        let directory = std::env::temp_dir().join(format!("rawr-served-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let tables = Tables::open(&directory).unwrap();
        let key = "a".repeat(64);
        let root = Root {
            module: [0; 16],
            krate: "0".to_owned(),
        };
        let first = [path(&["iced", "run"])];
        let grown = [path(&["iced", "run"]), path(&["iced", "widget", "text"])];
        let memo = Memo::default();
        tables.store(&key, &root, &memo, Some(Served::new("s", &first, &memo)));
        drop(tables);
        let tables = Tables::open(&directory).unwrap();
        let mut used = Vec::new();
        assert!(tables.outgrown(&key, "s", &grown, &mut used).is_some());
        assert_eq!(used, std::slice::from_ref(&key));
        // The same demand replays, another session's may need what the
        // table holds, and so may a demand that drops a path.
        assert!(tables.outgrown(&key, "s", &first, &mut used).is_none());
        assert!(tables.outgrown(&key, "t", &grown, &mut used).is_none());
        assert!(tables.outgrown(&key, "s", &grown[1..], &mut used).is_none());
        // A table with no record, as one seeded from rawr's home, is read.
        tables.store(&key, &root, &memo, None);
        drop(tables);
        let tables = Tables::open(&directory).unwrap();
        assert!(!directory.join(format!("{key}.{SERVED}")).exists());
        assert!(tables.outgrown(&key, "s", &grown, &mut used).is_none());
        // Records of two sessions do not merge.
        let other = Served::new("t", &first, &memo);
        assert!(Served::new("s", &grown, &memo).merged(&other).is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
