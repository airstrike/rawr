//! Canonical, explicitly framed stream-argument identity material.

use crate::ImmutableString;
use std::fmt;

/// A deterministic identity encoder could not represent one stream argument.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    message: ImmutableString,
}

/// One sealed canonical contribution for one declared stream argument.
#[derive(Clone)]
pub(crate) struct Contribution {
    bytes: Box<[u8]>,
}

/// Opaque canonical atoms available to a stream argument encoder.
///
/// Each method emits a tagged, length-framed atom. Encoders must choose atoms
/// from the logical argument value; formatting, native hashes, `TypeId`, and
/// pointer identity are deliberately unavailable.
pub struct Writer {
    bytes: Vec<u8>,
    atoms: usize,
}

impl Error {
    /// Describe a deterministic encoding failure.
    #[must_use]
    pub fn new(message: impl Into<ImmutableString>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Writer {
    pub(crate) const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            atoms: 0,
        }
    }

    /// Append a fixed-width unsigned 32-bit integer.
    pub fn u32(&mut self, value: u32) {
        self.atom(1, &value.to_be_bytes());
    }

    /// Append a fixed-width signed 64-bit integer.
    pub fn i64(&mut self, value: i64) {
        self.atom(2, &value.to_be_bytes());
    }

    /// Append a fixed-width unsigned 64-bit integer.
    pub fn u64(&mut self, value: u64) {
        self.atom(3, &value.to_be_bytes());
    }

    /// Append one canonical boolean.
    pub fn bool(&mut self, value: bool) {
        self.atom(4, &[u8::from(value)]);
    }

    /// Append canonical UTF-8 text.
    pub fn text(&mut self, value: &str) {
        self.atom(5, value.as_bytes());
    }

    /// Append explicitly canonical opaque bytes.
    pub fn bytes(&mut self, value: &[u8]) {
        self.atom(6, value);
    }

    /// Append a platform-independent collection count.
    pub fn count(&mut self, value: u64) {
        self.atom(7, &value.to_be_bytes());
    }

    pub(crate) fn finish(self) -> Result<Contribution, Error> {
        if self.atoms == 0 {
            return Err(Error::new(
                "stream identity encoder emitted no canonical atoms",
            ));
        }
        Ok(Contribution {
            bytes: self.bytes.into_boxed_slice(),
        })
    }

    fn atom(&mut self, tag: u8, value: &[u8]) {
        self.bytes.push(tag);
        self.bytes
            .extend_from_slice(&(value.len() as u64).to_be_bytes());
        self.bytes.extend_from_slice(value);
        self.atoms += 1;
    }
}

impl Contribution {
    pub(crate) const fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl fmt::Debug for Writer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Writer(<private>)")
    }
}

impl fmt::Debug for Contribution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Contribution(<private>)")
    }
}
