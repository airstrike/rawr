use crate::{admit, Admission, Error, Work};

pub(crate) use layout::NAME_OFFSET;

/// Darwin's `struct dirent` from `__getdirentries64`. See ../SAFETY.md.
#[cfg(target_os = "macos")]
mod layout {
    pub(crate) const NAME_OFFSET: usize = 21;
    pub(super) const TYPE: usize = 20;
    pub(super) const MAX_NAME: usize = 1023;
}

/// Linux's `struct linux_dirent64` from `getdents64`. See ../SAFETY.md.
#[cfg(not(target_os = "macos"))]
mod layout {
    pub(crate) const NAME_OFFSET: usize = 19;
    pub(super) const TYPE: usize = 18;
    pub(super) const MAX_NAME: usize = 255;
}

/// An initialized syscall prefix; no pointers or unaligned struct casts.
pub struct Batch<'buffer> {
    bytes: &'buffer [u8],
    terminal: bool,
}

/// Darwin's vacant inode-zero records are framed and charged before being
/// discarded. Linux has no vacant records.
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
        if !framed(length) || length > self.bytes.len() {
            return Err(Framing::Length.into());
        }
        let (record, remaining) = self.bytes.split_at(length);
        self.bytes = remaining;
        let Some(name_length) = name_length(record, admission)? else {
            self.terminal = false;
            return Ok(Some(Record::Vacant));
        };
        let name = &record[NAME_OFFSET..NAME_OFFSET + name_length + 1];
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
            kind: header[layout::TYPE],
        })))
    }
}

// Pinned libc requires four-byte record alignment. XNU's conversion path uses
// eight, but native extended filesystem entries need not use that conversion
// path. Do not impose an extra eight-byte restriction.
#[cfg(target_os = "macos")]
const fn framed(length: usize) -> bool {
    length >= 24 && length.is_multiple_of(4)
}

// Linux fs/readdir.c filldir64 is the only producer of these records. Its
// record length is the eight-byte-aligned header, name and terminator, so the
// shortest record, a one-byte name, is 24 bytes.
#[cfg(not(target_os = "macos"))]
const fn framed(length: usize) -> bool {
    length >= 24 && length.is_multiple_of(8)
}

/// Darwin records carry their name length, and an inode-zero record is vacant.
/// Its name fields are never interpreted. The name scan is admitted before it.
#[cfg(target_os = "macos")]
fn name_length<A: Admission>(
    record: &[u8],
    admission: &mut A,
) -> Result<Option<usize>, Error<A::Error>> {
    if record[..8] == [0; 8] {
        return Ok(None);
    }
    let name_length = usize::from(u16::from_ne_bytes([record[18], record[19]]));
    if name_length == 0
        || name_length > layout::MAX_NAME
        || NAME_OFFSET + name_length >= record.len()
    {
        return Err(Framing::NameLength.into());
    }
    admit(
        admission,
        Work::Name {
            bytes: name_length + 1,
        },
    )?;
    Ok(Some(name_length))
}

/// Linux records end their name at its first NUL. The whole name field, at most
/// seven padding bytes past that terminator, is admitted before the scan.
/// filldir64 writes ALIGN(offsetof(d_name) + namlen + 1, sizeof(u64)) exactly,
/// so any other length is a misframed record, never padding to skip.
/// An inode-zero record is an entry: the kernel, glibc since 2.33, musl and
/// std's ReadDir all return it, so it is never vacant here.
#[cfg(not(target_os = "macos"))]
fn name_length<A: Admission>(
    record: &[u8],
    admission: &mut A,
) -> Result<Option<usize>, Error<A::Error>> {
    let field = &record[NAME_OFFSET..];
    admit(admission, Work::Name { bytes: field.len() })?;
    let name_length = field
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(Framing::Name)?;
    if name_length == 0 || name_length > layout::MAX_NAME {
        return Err(Framing::NameLength.into());
    }
    if record.len() != (NAME_OFFSET + name_length + 1).next_multiple_of(8) {
        return Err(Framing::Length.into());
    }
    Ok(Some(name_length))
}

#[cfg(test)]
mod tests;
