//! The native instances of marked generics (D866): what rawr's own
//! workspace wrapper records when it checks a generated app crate, beside
//! the unit's artifact, so a fresh unit keeps the record its last check
//! wrote. The driver writes it ([`Record::write`]); rawr reads it
//! ([`Record::read`]) to compile each marked generic native code calls at
//! the types it calls it with.
//!
//! The driver and the library both compile this file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The record's layout. A record of another is read as none.
pub const SCHEMA: u32 = 1;

/// The environment variable that names, one per line, the manifest
/// directories of the generated crates the wrapper records. Its value is
/// part of each recorded unit's fingerprint.
pub const ROOTS: &str = "RAWR_IMPORT_RECORD";

/// The extension of a record beside its unit's `.rmeta`.
const EXTENSION: &str = "rawr-instances";

/// One check's instances, in the order the driver found them.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    schema: u32,
    instances: Vec<Instance>,
}

/// One instance of a generic trampoline: the path its `Call` names, from
/// the crate root, as `widget::sidebar::row_group`; the `type_name` of the
/// tuple of its type arguments, which the trampoline sends, as
/// `(islands_theme::Message,)`; and each argument as a Rust path from the
/// crate root, as `crate::Message`.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub path: String,
    pub name: String,
    pub types: Vec<String>,
}

impl Record {
    /// A record of `instances`, each once, in a stable order.
    #[must_use]
    pub fn new(mut instances: Vec<Instance>) -> Self {
        instances.sort();
        instances.dedup();
        Self {
            schema: SCHEMA,
            instances,
        }
    }

    /// The instances native code calls.
    #[must_use]
    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    /// Where the record of the unit whose artifact is `artifact` is.
    #[must_use]
    pub fn beside(artifact: &Path) -> PathBuf {
        artifact.with_extension(EXTENSION)
    }

    /// The record beside `artifact`: `None` when there is none, or it is
    /// of another layout.
    ///
    /// # Errors
    ///
    /// The record can't be read, or isn't a record.
    pub fn read(artifact: &Path) -> io::Result<Option<Self>> {
        let bytes = match fs::read(Self::beside(artifact)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let record: Self = serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok((record.schema == SCHEMA).then_some(record))
    }

    /// Write the record beside `artifact`, replacing the one there.
    ///
    /// # Errors
    ///
    /// The record can't be written.
    pub fn write(&self, artifact: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        fs::write(Self::beside(artifact), bytes)
    }
}
