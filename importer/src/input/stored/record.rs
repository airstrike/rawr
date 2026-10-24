//! Borrowed framing for one paired historical resolution record.

use super::codec::{self, Reader};
use super::{Budget, Error, Result, Stored};
use crate::cargo::toolchain;
use crate::input::build;

const MAGIC: &[u8] = b"rawr-resolution-record\0\x02";
const SECTIONS: u32 = 5;

#[cfg(feature = "rustc-driver")]
mod file;
#[cfg(feature = "rustc-driver")]
mod namespace;
#[cfg(feature = "rustc-driver")]
pub(crate) use file::Commitment;

/// Exact section boundaries, not parsed Cargo data or a current owner receipt.
/// Typed consumers must validate every section under their existing accounts.
pub(crate) struct Record<'bytes> {
    #[cfg(feature = "rustc-driver")]
    encoded: &'bytes [u8],
    metadata: &'bytes [u8],
    units: &'bytes [u8],
    build: &'bytes [u8],
    input: &'bytes [u8],
    namespace: &'bytes [u8],
}

impl<'bytes> Record<'bytes> {
    /// Write all five sections into the one prospectively admitted output buffer.
    pub(crate) fn encode(
        metadata: &[u8],
        units: &[u8],
        build: &build::Retained,
        input: &Stored,
        budget: &mut Budget<'_>,
    ) -> Result<Vec<u8>> {
        Self::encode_sections(metadata, units, build, input, |_| Ok(()), budget)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn encode_with_namespace(
        metadata: &[u8],
        units: &[u8],
        build: &build::Retained,
        input: &Stored,
        namespace: &crate::extract::plan::namespace::Retained,
        budget: &mut Budget<'_>,
    ) -> Result<Vec<u8>> {
        Self::encode_sections(
            metadata,
            units,
            build,
            input,
            |writer| namespace::write(writer, namespace),
            budget,
        )
    }

    fn encode_sections(
        metadata: &[u8],
        units: &[u8],
        build: &build::Retained,
        input: &Stored,
        write_namespace: impl Fn(&mut codec::Writer<'_>) -> Result<()>,
        budget: &mut Budget<'_>,
    ) -> Result<Vec<u8>> {
        budget.check()?;
        budget.limited(
            "retained Cargo metadata bytes",
            budget.limits.json_bytes,
            metadata.len(),
        )?;
        budget.limited(
            "retained Cargo unit bytes",
            budget.limits.json_bytes,
            units.len(),
        )?;
        let encoded = codec::encode(budget, |writer| {
            writer.raw(MAGIC)?;
            writer.text(toolchain::supported_cargo_commit())?;
            writer.u32(SECTIONS)?;
            writer.bytes(metadata)?;
            writer.bytes(units)?;
            writer.section(|writer| build.write(writer))?;
            writer.section(|writer| input.write(writer))?;
            writer.section(|writer| write_namespace(writer))?;
            Ok(())
        })?;
        Ok(encoded)
    }

    /// Validate the complete outer framing and borrow its five fixed sections.
    pub(crate) fn decode(bytes: &'bytes [u8], budget: &mut Budget<'_>) -> Result<Self> {
        budget.check()?;
        budget.wire_record(bytes)?;
        let mut reader = Reader::new(bytes, budget);
        if reader.take(MAGIC.len())? != MAGIC
            || reader.bytes()? != toolchain::supported_cargo_commit().as_bytes()
            || reader.u32()? != SECTIONS
        {
            return Err(Error::Encoding(
                "unknown resolution record schema, Cargo pin or sections",
            ));
        }
        let metadata = reader.bytes()?;
        reader.budget.limited(
            "retained Cargo metadata bytes",
            reader.budget.limits.json_bytes,
            metadata.len(),
        )?;
        let units = reader.bytes()?;
        reader.budget.limited(
            "retained Cargo unit bytes",
            reader.budget.limits.json_bytes,
            units.len(),
        )?;
        let build = reader.bytes()?;
        let input = reader.bytes()?;
        let namespace = reader.bytes()?;
        reader.finish()?;
        Ok(Self {
            #[cfg(feature = "rustc-driver")]
            encoded: bytes,
            metadata,
            units,
            build,
            input,
            namespace,
        })
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn decode_namespace(
        &self,
        budget: &mut Budget<'_>,
    ) -> Result<Option<crate::extract::plan::namespace::Retained>> {
        if self.namespace.is_empty() {
            return Ok(None);
        }
        namespace::decode(self.namespace, budget).map(Some)
    }

    pub(crate) const fn metadata(&self) -> &'bytes [u8] {
        self.metadata
    }

    pub(crate) const fn units(&self) -> &'bytes [u8] {
        self.units
    }

    pub(crate) const fn build(&self) -> &'bytes [u8] {
        self.build
    }

    pub(crate) const fn input(&self) -> &'bytes [u8] {
        self.input
    }
}

#[cfg(test)]
mod tests;
