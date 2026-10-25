//! Live ordered directory data under the caller's original account.
//! No captured transcript grants current-source or successful-selector authority.

use std::ffi::CStr;

use super::super::{observed, Budget, Error, Result};
use observed::stream::{Row, Transcript};

// rawr_directory::Stream keeps its read buffer inline; boxing would allocate per directory.
#[allow(clippy::large_enum_variant)]
pub(super) enum Start<'borrow, 'account> {
    Failed(observed::Record),
    Opened(Capture<'borrow, 'account>),
}

pub(super) struct Capture<'borrow, 'account> {
    path: observed::Path,
    mount: rawr_directory::Mount,
    stream: rawr_directory::Stream,
    rows: Vec<Row>,
    end: Option<observed::End>,
    refused: bool,
    budget: &'borrow mut Budget<'account>,
}

pub(super) enum Next<'entry, 'account> {
    Entry(Pending<'entry, 'account>),
    End(observed::End),
}

/// The native entry and output slot remain associated until inspection or skip.
pub(super) struct Pending<'entry, 'account> {
    native: rawr_directory::StreamEntry<'entry>,
    row: &'entry mut Row,
    refused: &'entry mut bool,
    budget: &'entry mut Budget<'account>,
}

pub(super) fn open<'borrow, 'account>(
    path: &[u8],
    budget: &'borrow mut Budget<'account>,
) -> Result<Start<'borrow, 'account>> {
    budget.allocation.check()?;
    let path = observed::Path::from_bytes(path, budget)?;
    let stream = match acquire(&path, budget)? {
        Opened::Failed(transcript) => return record(path, transcript).map(Start::Failed),
        Opened::Stream(stream) => stream,
    };
    Ok(Start::Opened(Capture {
        path,
        mount: stream.mount(),
        stream,
        rows: Vec::new(),
        end: None,
        refused: false,
        budget,
    }))
}

/// Shared native acquisition only. Complete and source-directed consumers keep
/// separate ownership/completion contracts over the same directory foundation.
// rawr_directory::Stream keeps its read buffer inline; boxing would allocate per directory.
#[allow(clippy::large_enum_variant)]
pub(super) enum Opened {
    Failed(Transcript),
    Stream(rawr_directory::Stream),
}

pub(super) fn acquire(path: &observed::Path, budget: &mut Budget<'_>) -> Result<Opened> {
    budget.allocation.check()?;
    let length = path
        .bytes()
        .len()
        .checked_add(1)
        .ok_or(Error::Encoding("directory path length overflow"))?;
    let mut terminated = Vec::new();
    budget.allocation.items(1)?;
    budget.allocation.bytes(length)?;
    terminated.try_reserve_exact(length)?;
    terminated.extend_from_slice(path.bytes());
    terminated.push(0);
    let native = CStr::from_bytes_with_nul(&terminated)
        .map_err(|_| Error::Encoding("invalid directory path"))?;
    let opening = match rawr_directory::Opening::open(native, &mut Admission(budget)) {
        Ok(opening) => opening,
        Err(rawr_directory::Error::Io { source, .. }) => {
            return Ok(Opened::Failed(Transcript::OpenFailed(errno(source)?)));
        }
        Err(error) => return Err(failure(error)),
    };
    let mount = opening.mount();
    let stream = match opening.finish(&mut Admission(budget)) {
        Ok(stream) => stream,
        Err(rawr_directory::Error::Io { source, .. }) => {
            return Ok(Opened::Failed(Transcript::UnionOpenFailed {
                mount,
                error: errno(source)?,
            }));
        }
        Err(error) => return Err(failure(error)),
    };
    Ok(Opened::Stream(stream))
}

impl<'account> Capture<'_, 'account> {
    /// Source-required path probes may use this same account between entries.
    pub(super) fn probe<T>(
        &mut self,
        operation: impl FnOnce(&mut Budget<'account>) -> Result<T>,
    ) -> Result<T> {
        self.check()?;
        self.refused = true;
        let result = operation(self.budget)?;
        self.refused = false;
        Ok(result)
    }

    fn check(&self) -> Result<()> {
        if self.refused {
            return Err(Error::Unconsumed);
        }
        self.budget.allocation.check()?;
        Ok(())
    }

    pub(super) fn next(&mut self) -> Result<Next<'_, 'account>> {
        self.check()?;
        if let Some(end) = self.end {
            return Ok(Next::End(end));
        }
        self.refused = true;
        let native = match self.stream.next(&mut Admission(self.budget)) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                self.end = Some(observed::End::Eof);
                self.refused = false;
                return Ok(Next::End(observed::End::Eof));
            }
            Err(rawr_directory::Error::Io { source, .. }) => {
                let end = observed::End::Error(errno(source)?);
                self.end = Some(end);
                self.refused = false;
                return Ok(Next::End(end));
            }
            Err(error) => return Err(failure(error)),
        };
        let name = self.budget.string(native.name())?;
        self.budget.reserve(&mut self.rows, 1)?;
        self.rows.push(Row::checked(name, native.raw_kind(), None)?);
        let row = self.rows.last_mut().ok_or(Error::Unconsumed)?;
        self.refused = false;
        Ok(Next::Entry(Pending {
            native,
            row,
            refused: &mut self.refused,
            budget: self.budget,
        }))
    }

    pub(super) fn finish(self) -> Result<observed::Record> {
        self.check()?;
        let end = self.end.ok_or(Error::Unconsumed)?;
        record(
            self.path,
            Transcript::Started {
                mount: self.mount,
                rows: self.rows,
                end,
            },
        )
    }
}

impl Pending<'_, '_> {
    pub(super) fn name(&self) -> &[u8] {
        &self.row.name
    }

    pub(super) fn inspect(self) -> Result<std::result::Result<observed::Kind, i32>> {
        *self.refused = true;
        self.budget.allocation.check()?;
        let result = self
            .native
            .file_type(&mut Admission(self.budget))
            .map_err(failure)?
            .map(observed::stream::kind);
        self.row.inspected = Some(result);
        *self.refused = false;
        Ok(result)
    }
}

fn record(path: observed::Path, transcript: Transcript) -> Result<observed::Record> {
    observed::Record {
        query: observed::Query::DirectoryStream(path),
        value: observed::Value::DirectoryStream(transcript.checked()?),
    }
    .checked()
}

pub(super) fn errno(source: std::io::Error) -> Result<i32> {
    source
        .raw_os_error()
        .filter(|code| *code > 0)
        .ok_or(Error::Encoding("directory native error must be positive"))
}

pub(super) fn failure(error: rawr_directory::Error<Error>) -> Error {
    match error {
        rawr_directory::Error::Admission(error) => error,
        rawr_directory::Error::Io { source, .. } => match errno(source) {
            Ok(code) => Error::Io(code),
            Err(error) => error,
        },
        rawr_directory::Error::Framing(_) => Error::Encoding("invalid directory frame"),
        rawr_directory::Error::Terminal => Error::Unconsumed,
        rawr_directory::Error::Allocation => Error::Encoding("directory allocation failed"),
    }
}

pub(super) struct Admission<'borrow, 'account>(pub(super) &'borrow mut Budget<'account>);

#[cfg(test)]
mod tests;

impl rawr_directory::Admission for Admission<'_, '_> {
    type Error = Error;
    fn admit(&mut self, work: rawr_directory::Work) -> Result<()> {
        use rawr_directory::{Storage, Work};
        self.0.allocation.check()?;
        match work {
            Work::Open { .. } | Work::Reopen { .. } => self.0.graph.paths(2)?,
            Work::Mount | Work::Read { .. } | Work::Record | Work::EntryType { .. } => {
                self.0.graph.paths(1)?
            }
            Work::Allocate { storage, bytes, .. } => {
                let items = match storage {
                    Storage::Entries => bytes / std::mem::size_of::<rawr_directory::Name>(),
                    Storage::Name => 1,
                };
                self.0.allocation.items(items)?;
                self.0.allocation.bytes(bytes)?;
            }
            Work::Name { bytes } => self
                .0
                .allocation
                .string_exceeded("membership native directory name", bytes),
            Work::Visible { entries } => self
                .0
                .allocation
                .items_exceeded("membership directory entries", entries),
            Work::Retain { .. } | Work::Copy { .. } | Work::Order | Work::Compare { .. } => {}
        }
        Ok(())
    }
}
