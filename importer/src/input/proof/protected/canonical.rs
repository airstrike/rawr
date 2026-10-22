//! Exact admitted canonical ownership for a freshly derived projection.

use std::io;

use crate::cargo::{universe::BuildProjection, Limits};
use crate::input::proof::ProtectedEntry;
use crate::input::stored::{Budget, Error, Result};

pub(super) fn current(
    entries: &[ProtectedEntry],
    limits: &Limits,
    budget: &mut Budget<'_>,
) -> Result<BuildProjection> {
    budget.check()?;
    let value = &(crate::cargo::universe::BUILD_PROJECTION_SCHEMA, entries);
    let mut count = Counter {
        bytes: 0,
        overflow: false,
    };
    serde_json::to_writer(&mut count, value)
        .map_err(|_| Error::Encoding("protected projection cannot be serialized"))?;
    if count.overflow {
        return Err(budget.overflow());
    }
    let bytes = count
        .bytes
        .checked_add(1)
        .ok_or_else(|| budget.overflow())?;
    budget.limited(
        "protected source/build projection bytes",
        limits
            .input_proof_bytes
            .min(budget.limits.input_proof_bytes),
        bytes,
    )?;
    budget.storage(1, bytes)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(bytes)
        .map_err(|error| budget.failure(Error::Allocation(error)))?;
    let mut writer = Writer {
        output,
        limit: count.bytes,
        overflow: false,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| Error::Encoding("protected projection cannot be serialized"))?;
    if writer.overflow {
        return Err(budget.overflow());
    }
    if writer.output.len() != count.bytes {
        return Err(Error::Encoding(
            "protected projection serialized length differs",
        ));
    }
    writer.output.push(b'\n');
    let output = budget.owned_box(writer.output)?;
    let hashed = BuildProjection::hash_bytes(output.len()).ok_or_else(|| budget.overflow())?;
    budget.hash(hashed)?;
    Ok(BuildProjection::from_canonical(output))
}

struct Counter {
    bytes: usize,
    overflow: bool,
}

impl io::Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self.bytes.checked_add(bytes.len()) {
            Some(value) => self.bytes = value,
            None => self.overflow = true,
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Writer {
    output: Vec<u8>,
    limit: usize,
    overflow: bool,
}

impl io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .output
            .len()
            .checked_add(bytes.len())
            .is_none_or(|length| length > self.limit)
        {
            self.overflow = true;
        } else if !self.overflow {
            self.output.extend_from_slice(bytes);
        }
        // Keep refusal outside serde's allocating I/O-error conversion.
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
