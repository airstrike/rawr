use crate::{admit, Admission, Error, Work};

pub(crate) const NAME_OFFSET: usize = 21;
const MAX_NAME: usize = 1023;

/// An initialized syscall prefix; no pointers or unaligned struct casts.
pub struct Batch<'buffer> {
    bytes: &'buffer [u8],
    terminal: bool,
}

/// Vacant inode-zero records are framed and charged before being discarded.
pub enum Record<'buffer> {
    Vacant,
    Entry(Entry<'buffer>),
}

/// A native name borrowed from one validated record in the caller's buffer.
pub struct Entry<'buffer> {
    pub(crate) name: &'buffer [u8],
    pub(crate) kind: u8,
}

impl Entry<'_> {
    pub const fn name(&self) -> &[u8] {
        self.name
    }

    /// The unmodified native hint. Callers must obtain authoritative metadata.
    pub const fn kind(&self) -> u8 {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Framing {
    #[error("returned prefix exceeds transfer storage")]
    Prefix,
    #[error("record header is truncated")]
    Header,
    #[error("record length is zero, unaligned, or outside the returned prefix")]
    Length,
    #[error("name length is outside this record or the native name field")]
    NameLength,
    #[error("name has a missing terminator, embedded NUL, or slash")]
    Name,
}

impl<'buffer> Batch<'buffer> {
    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len()
    }
    pub(crate) const fn new(bytes: &'buffer [u8]) -> Self {
        Self {
            bytes,
            terminal: false,
        }
    }

    pub fn next<A: Admission>(
        &mut self,
        admission: &mut A,
    ) -> Result<Option<Record<'buffer>>, Error<A::Error>> {
        if self.terminal {
            return Err(Error::Terminal);
        }
        self.terminal = true;
        admit(admission, Work::Record)?;
        if self.bytes.is_empty() {
            return Ok(None);
        }
        let header = self.bytes.get(..NAME_OFFSET).ok_or(Framing::Header)?;
        let length = usize::from(u16::from_ne_bytes([header[16], header[17]]));
        // Pinned libc requires four-byte record alignment. XNU's conversion
        // path uses eight, but native extended filesystem entries need not use
        // that conversion path. Do not impose an extra eight-byte restriction.
        if length < 24 || length % 4 != 0 || length > self.bytes.len() {
            return Err(Framing::Length.into());
        }
        let (record, remaining) = self.bytes.split_at(length);
        self.bytes = remaining;
        if header[..8] == [0; 8] {
            self.terminal = false;
            return Ok(Some(Record::Vacant));
        }
        let name_length = usize::from(u16::from_ne_bytes([header[18], header[19]]));
        if name_length == 0 || name_length > MAX_NAME {
            return Err(Framing::NameLength.into());
        }
        let name = record
            .get(NAME_OFFSET..NAME_OFFSET + name_length + 1)
            .ok_or(Framing::NameLength)?;
        admit(admission, Work::Name { bytes: name.len() })?;
        if name[name_length] != 0
            || name[..name_length]
                .iter()
                .any(|byte| matches!(*byte, 0 | b'/'))
        {
            return Err(Framing::Name.into());
        }
        self.terminal = false;
        Ok(Some(Record::Entry(Entry {
            name: &name[..name_length],
            kind: header[20],
        })))
    }
}

#[cfg(test)]
mod tests;
