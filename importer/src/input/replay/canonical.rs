//! Counted canonical JSON hashing without a serialized ownership buffer.

use std::io;

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::input::stored::{Budget, Error, Result};

struct Canonical {
    hash: Option<Sha256>,
    bytes: usize,
    overflow: bool,
}

impl io::Write for Canonical {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self.bytes.checked_add(bytes.len()) {
            Some(length) => self.bytes = length,
            None => self.overflow = true,
        }
        if let Some(hash) = &mut self.hash {
            hash.update(bytes);
        }
        // Refusal happens after the count pass, before constructing a hasher.
        // Reporting it outside serde avoids allocating a serde I/O error.
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Serialize the caller's same borrowed schema twice: first to count, then to
/// hash after admission. This adds no decoded storage or filesystem work.
pub(in crate::input) fn digest(
    evidence: &impl Serialize,
    resource: &'static str,
    limit: usize,
    budget: &mut Budget<'_>,
) -> Result<[u8; 32]> {
    budget.check()?;
    let mut count = Canonical {
        hash: None,
        bytes: 0,
        overflow: false,
    };
    serde_json::to_writer(&mut count, evidence)
        .map_err(|_| Error::Encoding("canonical evidence cannot be serialized"))?;
    if count.overflow {
        return Err(budget.overflow());
    }
    budget.limited(resource, limit, count.bytes)?;
    budget.hash(count.bytes)?;
    let mut writer = Canonical {
        hash: Some(Sha256::new()),
        bytes: 0,
        overflow: false,
    };
    serde_json::to_writer(&mut writer, evidence)
        .map_err(|_| Error::Encoding("canonical evidence cannot be serialized"))?;
    if writer.overflow || writer.bytes != count.bytes {
        return Err(Error::Encoding("canonical evidence length changed"));
    }
    let hash = writer
        .hash
        .ok_or(Error::Encoding("canonical hash state missing"))?;
    Ok(hash.finalize().into())
}
