//! Recorded unit tables, one per selected crate (`engine::Table`). A table's
//! key is the crate alone: its package, its unit's key (which covers its
//! features, profile, target and its dependencies' keys), the compiler and
//! the extraction's formats. Never the demand or the probe's aliases, so
//! any program that reaches the crate reads it. A crate from a registry or
//! a git commit cannot change under its key; any other crate's key takes
//! its crate hash too, which an edit to it or to its dependencies changes.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use super::engine::{self, Memo, Root, Table};
use super::{Invocation, PROTOCOL_SCHEMA};

pub(super) struct Tables {
    directory: PathBuf,
}

impl Tables {
    pub(super) fn open(directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory)
            .map_err(|error| format!("failed to create the extraction table directory: {error}"))?;
        Ok(Self {
            directory: directory.to_path_buf(),
        })
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
    pub(super) fn load(
        &self,
        invocation: &Invocation,
        crate_hash: &str,
        root: &Root,
        used: &mut Vec<String>,
    ) -> Option<Memo> {
        let key = Self::key(invocation, crate_hash);
        used.push(key.clone());
        let bytes = fs::read(self.directory.join(&key)).ok()?;
        let table: Table = serde_json::from_slice(&bytes).ok()?;
        (table.schema == engine::TABLE_SCHEMA && table.root == *root)
            .then(|| Memo::from_table(table))
    }

    /// Keep a crate's table, replacing the one kept before. Failure only
    /// costs a later extraction.
    pub(super) fn store(
        &self,
        invocation: &Invocation,
        crate_hash: &str,
        root: &Root,
        memo: &Memo,
    ) {
        let key = Self::key(invocation, crate_hash);
        let temporary = self
            .directory
            .join(format!(".{key}.tmp-{}", std::process::id()));
        let written = (|| -> std::io::Result<()> {
            let mut file = std::io::BufWriter::new(fs::File::create(&temporary)?);
            serde_json::to_writer(&mut file, &memo.to_table(root.clone()))?;
            file.flush()?;
            fs::rename(&temporary, self.directory.join(&key))
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
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
            if name.len() == 64
                && name
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
                && !used.iter().any(|used| used == name)
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
