//! Ordered source-call data, never a live descriptor or freshness authority.

use super::{End, Kind};
use crate::input::membership::{Error, Result};

#[derive(Debug, Eq, PartialEq)]
pub(in super::super) struct Row {
    pub(in super::super) name: Vec<u8>,
    pub(in super::super) raw_kind: u8,
    pub(in super::super) inspected: Option<std::result::Result<Kind, i32>>,
}

#[derive(Debug, Eq, PartialEq)]
pub(in super::super) enum Transcript {
    OpenFailed(i32),
    UnionOpenFailed {
        mount: rawr_directory::Mount,
        error: i32,
    },
    Started {
        mount: rawr_directory::Mount,
        rows: Vec<Row>,
        end: End,
    },
}

impl Row {
    pub(in super::super) fn checked(
        name: Vec<u8>,
        raw_kind: u8,
        inspected: Option<std::result::Result<Kind, i32>>,
    ) -> Result<Self> {
        let row = Self {
            name,
            raw_kind,
            inspected,
        };
        row.validate()?;
        Ok(row)
    }
    pub(in super::super) fn validate(&self) -> Result<()> {
        if self.name.is_empty()
            || self.name.len() > 1023
            || matches!(self.name.as_slice(), b"." | b"..")
            || self.name.iter().any(|byte| matches!(*byte, 0 | b'/'))
            || self.raw_kind == 14
        {
            return Err(Error::Encoding("invalid source directory entry"));
        }
        if matches!(self.inspected, Some(Err(code)) if code <= 0) {
            return Err(Error::Encoding("invalid source entry errno"));
        }
        if let (Some(expected), Some(actual)) = (hint(self.raw_kind), self.inspected)
            && actual != Ok(expected)
        {
            return Err(Error::Encoding("source entry hint/result mismatch"));
        }
        Ok(())
    }
}
impl Transcript {
    pub(in super::super) fn checked(self) -> Result<Self> {
        self.validate()?;
        Ok(self)
    }
    pub(in super::super) fn validate(&self) -> Result<()> {
        match self {
            Self::OpenFailed(code) if *code > 0 => Ok(()),
            Self::UnionOpenFailed {
                mount: rawr_directory::Mount::Known(true),
                error,
            } if *error > 0 => Ok(()),
            Self::Started { mount, rows, end } => {
                if matches!(mount, rawr_directory::Mount::Failed(code) if *code <= 0)
                    || matches!(end, End::Error(code) if *code <= 0)
                {
                    return Err(Error::Encoding("invalid source directory errno"));
                }
                // Union collection errors occur before any source iterator is
                // returned; a successfully started union stream ends at EOF.
                if *mount == rawr_directory::Mount::Known(true) && *end != End::Eof {
                    return Err(Error::Encoding("union error after source open"));
                }
                for row in rows {
                    row.validate()?;
                }
                Ok(())
            }
            _ => Err(Error::Encoding("invalid source directory open result")),
        }
    }
}

pub(in super::super) fn hint(raw: u8) -> Option<Kind> {
    Some(match raw {
        1 => Kind::Fifo,
        2 => Kind::Character,
        4 => Kind::Directory,
        6 => Kind::Block,
        8 => Kind::File,
        10 => Kind::Symlink,
        12 => Kind::Socket,
        _ => return None,
    })
}
pub(in super::super) fn kind(value: rawr_directory::FileType) -> Kind {
    match value {
        rawr_directory::FileType::Directory => Kind::Directory,
        rawr_directory::FileType::File => Kind::File,
        rawr_directory::FileType::Symlink => Kind::Symlink,
        rawr_directory::FileType::Character => Kind::Character,
        rawr_directory::FileType::Block => Kind::Block,
        rawr_directory::FileType::Fifo => Kind::Fifo,
        rawr_directory::FileType::Socket => Kind::Socket,
        rawr_directory::FileType::Other => Kind::Unknown,
    }
}
