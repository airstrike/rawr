//! Versioned canonical digest evidence for compiler-private proofs.

use std::fmt;

use crate::{item, receiver, type_def};

const LEFT_OFFSET: u64 = 0xcbf29ce484222325;
const RIGHT_OFFSET: u64 = 0x6c62272e07bb0142;
const PRIME: u64 = 0x100000001b3;

/// Opaque, versioned evidence derived from one exact compiler-private proof.
///
/// A digest is useful in manifests and diagnostics. It is not compiler
/// authority and must never replace exact fingerprint equality.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Digest([u8; 16]);

/// A compiler invariant required by the canonical digest was not present.
#[derive(Debug)]
pub enum Error {
    /// A source-owned slot retained only an opaque Rust representation.
    OpaqueHostIdentity {
        /// Exact proof slot missing semantic identity.
        context: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpaqueHostIdentity { context } => write!(
                formatter,
                "digest invariant failed: {context} retained a host TypeId without a stable semantic path"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Return non-authoritative, domain-separated identity for one exact symbolic
/// event mapper. Runtime use must still rebind the matching [`item::Value`]
/// through the current generation authority.
pub(crate) fn event_mapper(value: &item::Value) -> Result<[u8; 16], Error> {
    let mut writer = Writer::with_version(b"rawr-profile-event-mapper-v1");
    value.write(&mut writer)?;
    Ok(writer.finish().bytes())
}

impl Digest {
    /// Return the fixed-width canonical digest bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Digest({self})")
    }
}

/// Crate-private canonical atoms accepted by digest participants.
///
/// This is intentionally not a serializer and does not implement
/// [`std::hash::Hasher`]. Every participant must choose its semantic tags and
/// fields explicitly.
pub(crate) struct Writer {
    left: u64,
    right: u64,
}

pub(crate) trait Write {
    fn write(&self, writer: &mut Writer) -> Result<(), Error>;
}

impl Writer {
    pub(crate) fn with_version(version: &[u8]) -> Self {
        let mut writer = Self {
            left: LEFT_OFFSET,
            right: RIGHT_OFFSET,
        };
        writer.frame(0, version);
        writer
    }

    /// Write a semantic structure, field, or enum-variant tag.
    pub(crate) fn tag(&mut self, tag: u16) {
        self.frame(1, &tag.to_be_bytes());
    }

    /// Write a platform-independent collection cardinality.
    pub(crate) fn count(&mut self, count: usize) {
        self.frame(2, &(count as u64).to_be_bytes());
    }

    /// Write a UTF-8 semantic name or path.
    pub(crate) fn text(&mut self, text: &str) {
        self.frame(3, text.as_bytes());
    }

    /// Write a fixed-width unsigned integer.
    pub(crate) fn u32(&mut self, value: u32) {
        self.frame(4, &value.to_be_bytes());
    }

    /// Write a boolean under an explicit one-byte representation.
    pub(crate) fn bool(&mut self, value: bool) {
        self.frame(5, &[u8::from(value)]);
    }

    /// Write opaque canonical bytes already framed by their owning protocol.
    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        self.frame(6, bytes);
    }

    fn frame(&mut self, atom: u8, bytes: &[u8]) {
        self.absorb(&[atom]);
        self.absorb(&(bytes.len() as u64).to_be_bytes());
        self.absorb(bytes);
    }

    fn absorb(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.left ^= u64::from(*byte);
            self.left = self.left.wrapping_mul(PRIME);
            self.right ^= u64::from(*byte);
            self.right = self.right.rotate_left(5).wrapping_mul(PRIME);
        }
    }

    pub(crate) fn finish(self) -> Digest {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&self.left.to_be_bytes());
        bytes[8..].copy_from_slice(&self.right.to_be_bytes());
        Digest(bytes)
    }
}

impl Write for item::Key {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1700);
        writer.tag(0x1701);
        writer.text(self.package());
        writer.tag(0x1702);
        writer.count(self.modules().len());
        for module in self.modules() {
            writer.text(module);
        }
        writer.tag(0x1703);
        write_script_option(writer, self.owner(), "item key owner")?;
        writer.tag(0x1704);
        write_item_kind(writer, self.kind());
        writer.tag(0x1705);
        writer.text(self.name());
        Ok(())
    }
}

impl Write for item::Value {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1800);
        writer.tag(0x1801);
        self.key().write(writer)?;
        writer.tag(0x1802);
        write_item_kind(writer, self.kind());
        writer.tag(0x1803);
        write_script_option(writer, self.owner(), "callback owner")?;
        writer.tag(0x1804);
        write_receiver(writer, self.receiver());
        writer.tag(0x1805);
        self.signature().write(writer)?;
        writer.tag(0x1806);
        self.capabilities().write(writer)?;
        Ok(())
    }
}

impl Write for type_def::Key {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1900);
        writer.tag(0x1901);
        writer.text(self.package());
        writer.tag(0x1902);
        writer.count(self.modules().len());
        for module in self.modules() {
            writer.text(module);
        }
        writer.tag(0x1903);
        writer.text(self.name());
        Ok(())
    }
}

fn write_item_kind(writer: &mut Writer, kind: item::Kind) {
    writer.tag(match kind {
        item::Kind::Struct => 0x1b01,
        item::Kind::Enum => 0x1b02,
        item::Kind::Function => 0x1b03,
        item::Kind::AssociatedFunction => 0x1b04,
        item::Kind::Method => 0x1b05,
        item::Kind::VariantConstructor => 0x1b06,
    });
}

pub(crate) fn write_receiver(writer: &mut Writer, receiver: Option<receiver::Mode>) {
    writer.tag(match receiver {
        None => 0x1c00,
        Some(receiver::Mode::Owned) => 0x1c01,
        Some(receiver::Mode::Shared) => 0x1c02,
        Some(receiver::Mode::Mutable) => 0x1c03,
    });
}

pub(crate) fn write_slice<T: Write>(writer: &mut Writer, values: &[T]) -> Result<(), Error> {
    writer.count(values.len());
    for value in values {
        value.write(writer)?;
    }
    Ok(())
}

pub(crate) fn write_option<T: Write>(writer: &mut Writer, value: Option<&T>) -> Result<(), Error> {
    match value {
        Some(value) => {
            writer.tag(0x1d01);
            value.write(writer)?;
        }
        None => writer.tag(0x1d00),
    }
    Ok(())
}

pub(crate) fn write_script_identity(
    writer: &mut Writer,
    identity: &type_def::Identity,
    context: &'static str,
) -> Result<(), Error> {
    let type_def::Identity::Script(key) = identity else {
        return Err(Error::OpaqueHostIdentity { context });
    };
    writer.tag(0x1a01);
    key.write(writer)
}

fn write_script_option(
    writer: &mut Writer,
    identity: Option<&type_def::Identity>,
    context: &'static str,
) -> Result<(), Error> {
    match identity {
        Some(identity) => {
            writer.tag(0x1d01);
            write_script_identity(writer, identity, context)
        }
        None => {
            writer.tag(0x1d00);
            Ok(())
        }
    }
}
