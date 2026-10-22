//! Versioned ordered directory data; no historical frontier is upgraded.

use super::{Decoder, Encoder};
use crate::input::membership::{
    observed::{
        self,
        stream::{Row, Transcript},
    },
    Budget, Error, Result,
};

pub(super) fn retain(value: &Transcript, budget: &mut Budget<'_>) -> Result<Transcript> {
    Ok(match value {
        Transcript::OpenFailed(error) => Transcript::OpenFailed(*error),
        Transcript::UnionOpenFailed { mount, error } => Transcript::UnionOpenFailed {
            mount: *mount,
            error: *error,
        },
        Transcript::Started { mount, rows, end } => {
            let mut retained = Vec::new();
            budget.reserve(&mut retained, rows.len())?;
            for row in rows {
                retained.push(Row::checked(
                    budget.string(&row.name)?,
                    row.raw_kind,
                    row.inspected,
                )?);
            }
            Transcript::Started {
                mount: *mount,
                rows: retained,
                end: *end,
            }
        }
    })
}
impl Decoder<'_, '_, '_> {
    fn stream_mount(&mut self) -> Result<rawr_directory::Mount> {
        match self.byte()? {
            0 => Ok(rawr_directory::Mount::Known(false)),
            1 => Ok(rawr_directory::Mount::Known(true)),
            2 => Ok(rawr_directory::Mount::Failed(self.u32()? as i32)),
            _ => Err(Error::Encoding("unknown source directory mount")),
        }
    }
    pub(super) fn stream(&mut self) -> Result<Transcript> {
        let value = match self.byte()? {
            0 => Transcript::OpenFailed(self.u32()? as i32),
            1 => Transcript::UnionOpenFailed {
                mount: self.stream_mount()?,
                error: self.u32()? as i32,
            },
            2 => {
                let mount = self.stream_mount()?;
                let count = self.count()?;
                // A row needs an eight-byte string length, at least one name
                // byte, a raw hint and an inspection tag before any payload.
                if count > self.remaining() / 11 {
                    return Err(Error::Encoding("impossible source directory row count"));
                }
                let mut rows = Vec::new();
                self.budget.reserve(&mut rows, count)?;
                for _ in 0..count {
                    let name = self.string()?;
                    let raw_kind = self.byte()?;
                    let inspected = match self.byte()? {
                        0 => None,
                        1 => Some(Ok(self.kind()?)),
                        2 => Some(Err(self.u32()? as i32)),
                        _ => return Err(Error::Encoding("unknown source entry inspection")),
                    };
                    rows.push(Row::checked(name, raw_kind, inspected)?);
                }
                let end = match self.byte()? {
                    0 => observed::End::Eof,
                    1 => observed::End::Error(self.u32()? as i32),
                    _ => return Err(Error::Encoding("missing source directory end")),
                };
                Transcript::Started { mount, rows, end }
            }
            _ => return Err(Error::Encoding("unknown source directory outcome")),
        };
        value.checked()
    }
}
impl Encoder<'_> {
    fn stream_mount(&mut self, mount: rawr_directory::Mount) -> Result<()> {
        match mount {
            rawr_directory::Mount::Known(false) => self.byte(0),
            rawr_directory::Mount::Known(true) => self.byte(1),
            rawr_directory::Mount::Failed(code) => {
                self.byte(2)?;
                self.u32(code as u32)
            }
        }
    }
    pub(super) fn stream(&mut self, value: &Transcript) -> Result<()> {
        match value {
            Transcript::OpenFailed(code) => {
                self.byte(0)?;
                self.u32(*code as u32)
            }
            Transcript::UnionOpenFailed { mount, error } => {
                self.byte(1)?;
                self.stream_mount(*mount)?;
                self.u32(*error as u32)
            }
            Transcript::Started { mount, rows, end } => {
                self.byte(2)?;
                self.stream_mount(*mount)?;
                self.count(rows.len())?;
                for row in rows {
                    self.bytes(&row.name)?;
                    self.byte(row.raw_kind)?;
                    match row.inspected {
                        None => self.byte(0)?,
                        Some(Ok(kind)) => {
                            self.byte(1)?;
                            self.kind(kind)?
                        }
                        Some(Err(code)) => {
                            self.byte(2)?;
                            self.u32(code as u32)?
                        }
                    }
                }
                match end {
                    observed::End::Eof => self.byte(0),
                    observed::End::Error(code) => {
                        self.byte(1)?;
                        self.u32(*code as u32)
                    }
                }
            }
        }
    }
}
