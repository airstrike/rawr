//! Fixed framing and prospective exact-size output ownership.

use std::path::{Path, PathBuf};

use super::{Budget, Error, Result, MAGIC};
use crate::cargo::toolchain;
use crate::input::{proof, Usage};

pub(super) struct Writer<'output> {
    output: Option<&'output mut Vec<u8>>,
    length: usize,
    limit: usize,
}

pub(super) struct Reader<'bytes, 'budget, 'account> {
    bytes: &'bytes [u8],
    offset: usize,
    pub(super) budget: &'budget mut Budget<'account>,
}

pub(super) fn encode(
    budget: &mut Budget<'_>,
    write: impl Fn(&mut Writer<'_>) -> Result<()>,
) -> Result<Vec<u8>> {
    budget.check()?;
    // The encoded baseline is node store content: its physical bound is
    // `node::MAX_CONTENT_BYTES`. The `input_proof_bytes` target only records
    // its measured length on the original account.
    let mut measure = Writer {
        output: None,
        length: 0,
        limit: crate::node::MAX_CONTENT_BYTES,
    };
    write(&mut measure).map_err(|error| budget.failure(error))?;
    let length = measure.length;
    budget.limited(
        "encoded stored baseline bytes",
        budget.limits.input_proof_bytes,
        length,
    )?;
    let mut bytes = budget.buffer(length)?;
    let mut writer = Writer {
        output: Some(&mut bytes),
        length: 0,
        limit: length,
    };
    write(&mut writer).map_err(|error| budget.failure(error))?;
    if writer.length != length {
        return Err(Error::Encoding("encoder length changed"));
    }
    Ok(bytes)
}

impl Writer<'_> {
    pub(super) fn raw(&mut self, value: &[u8]) -> Result<()> {
        let end = self
            .length
            .checked_add(value.len())
            .ok_or(Error::Overflow)?;
        if end > self.limit {
            return Err(crate::Error::Limit {
                resource: "encoded stored baseline bytes",
                limit: self.limit,
                actual: end,
            }
            .into());
        }
        if let Some(output) = &mut self.output {
            output.extend_from_slice(value);
        }
        self.length = end;
        Ok(())
    }

    pub(super) fn byte(&mut self, value: u8) -> Result<()> {
        self.raw(&[value])
    }
    pub(super) fn u32(&mut self, value: u32) -> Result<()> {
        self.raw(&value.to_le_bytes())
    }
    pub(super) fn count(&mut self, value: usize) -> Result<()> {
        let value = u64::try_from(value).map_err(|_| Error::Overflow)?;
        self.raw(&value.to_le_bytes())
    }
    pub(super) fn bytes(&mut self, value: &[u8]) -> Result<()> {
        self.count(value.len())?;
        self.raw(value)
    }
    /// Frame a borrowed serializer inside this writer's unchanged total cap.
    /// The outer measurement pass admits the sole output buffer; the output
    /// pass fills the already-written length field after streaming the section.
    pub(super) fn section(&mut self, write: impl FnOnce(&mut Self) -> Result<()>) -> Result<()> {
        let offset = self.length;
        self.count(0)?;
        let start = self.length;
        write(self)?;
        let length = self.length.checked_sub(start).ok_or(Error::Overflow)?;
        let length = u64::try_from(length).map_err(|_| Error::Overflow)?;
        if let Some(output) = &mut self.output {
            let field = output
                .get_mut(offset..start)
                .ok_or(Error::Encoding("section length field is absent"))?;
            field.copy_from_slice(&length.to_le_bytes());
        }
        Ok(())
    }
    pub(super) fn text(&mut self, value: &str) -> Result<()> {
        self.bytes(value.as_bytes())
    }
    pub(super) fn path(&mut self, value: &Path) -> Result<()> {
        self.bytes(value.as_os_str().as_encoded_bytes())
    }
    pub(super) fn option(&mut self, value: Option<&str>) -> Result<()> {
        match value {
            None => self.byte(0),
            Some(value) => {
                self.byte(1)?;
                self.text(value)
            }
        }
    }
    pub(super) fn usage(&mut self, usage: Usage) -> Result<()> {
        self.count(usage.items)?;
        self.count(usage.bytes)
    }
    pub(super) fn header(&mut self, schema: u32, usage: Usage) -> Result<()> {
        if schema != proof::SCHEMA {
            return Err(Error::Encoding("unknown expected-proof schema"));
        }
        self.raw(MAGIC)?;
        self.text(toolchain::supported_cargo_commit())?;
        self.u32(schema)?;
        self.usage(usage)
    }
}

impl<'bytes, 'budget, 'account> Reader<'bytes, 'budget, 'account> {
    pub(super) fn new(bytes: &'bytes [u8], budget: &'budget mut Budget<'account>) -> Self {
        Self {
            bytes,
            offset: 0,
            budget,
        }
    }
    pub(super) fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
    pub(super) fn take(&mut self, length: usize) -> Result<&'bytes [u8]> {
        self.budget.check()?;
        let end = self
            .offset
            .checked_add(length)
            .ok_or(Error::Encoding("record offset overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(Error::Encoding("truncated record"))?;
        self.offset = end;
        Ok(value)
    }
    pub(super) fn byte(&mut self) -> Result<u8> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(Error::Encoding("missing tag"))
    }
    pub(super) fn u32(&mut self) -> Result<u32> {
        let value = self
            .take(4)?
            .try_into()
            .map_err(|_| Error::Encoding("invalid u32"))?;
        Ok(u32::from_le_bytes(value))
    }
    pub(super) fn length(&mut self) -> Result<usize> {
        let value = self
            .take(8)?
            .try_into()
            .map_err(|_| Error::Encoding("invalid u64"))?;
        usize::try_from(u64::from_le_bytes(value))
            .map_err(|_| Error::Encoding("unaddressable length"))
    }
    pub(super) fn count(&mut self) -> Result<usize> {
        let count = self.length()?;
        self.budget
            .limited("stored baseline items", self.budget.limits.items, count)?;
        Ok(count)
    }
    pub(super) fn bytes(&mut self) -> Result<&'bytes [u8]> {
        let length = self.length()?;
        self.take(length)
    }
    pub(super) fn text(&mut self) -> Result<Box<str>> {
        let bytes = self.bytes()?;
        self.budget.text(bytes, true)
    }
    pub(super) fn text_path(&mut self) -> Result<Box<str>> {
        let bytes = self.bytes()?;
        self.budget.text(bytes, false)
    }
    pub(super) fn path(&mut self) -> Result<PathBuf> {
        let bytes = self.bytes()?;
        self.budget.path(bytes)
    }
    pub(super) fn option(&mut self) -> Result<Option<Box<str>>> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.text().map(Some),
            _ => Err(Error::Encoding("unknown option tag")),
        }
    }
    pub(super) fn option_path(&mut self) -> Result<Option<Box<str>>> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.text_path().map(Some),
            _ => Err(Error::Encoding("unknown option tag")),
        }
    }
    pub(super) fn digest(&mut self) -> Result<[u8; 32]> {
        self.take(32)?
            .try_into()
            .map_err(|_| Error::Encoding("invalid digest"))
    }
    pub(super) fn usage(&mut self) -> Result<Usage> {
        let usage = Usage {
            items: self.length()?,
            bytes: self.length()?,
        };
        // The producer's retained usage is measured against accounting
        // targets that never refused it; a stored figure above a target is
        // recorded on this account, never rejected as an encoding error.
        let (items, bytes) = (
            self.budget.limits.items,
            self.budget.limits.input_proof_bytes,
        );
        self.budget
            .limited("stored baseline retained items", items, usage.items)?;
        self.budget
            .limited("stored baseline retained bytes", bytes, usage.bytes)?;
        Ok(usage)
    }
    pub(super) fn header(&mut self) -> Result<(u32, Usage)> {
        if self.take(MAGIC.len())? != MAGIC
            || self.bytes()? != toolchain::supported_cargo_commit().as_bytes()
        {
            return Err(Error::Encoding("unknown baseline schema or Cargo pin"));
        }
        let schema = self.u32()?;
        if schema != proof::SCHEMA {
            return Err(Error::Encoding("unknown expected-proof schema"));
        }
        Ok((schema, self.usage()?))
    }
    pub(super) fn finish(self) -> Result<()> {
        if self.remaining() != 0 {
            Err(Error::Encoding("trailing record bytes"))
        } else {
            Ok(())
        }
    }
}
