//! Detached image-borrowing cursor permits the source's interleaved path probes.

use super::{observed, stored, Host};
use crate::input::membership::{Error, Result};
use observed::stream::{Row, Transcript};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// Move-stable issuing-host identity only. No account, graph, allocation or
/// historical registry is created. Exhaustion refuses instead of reusing IDs.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(in super::super) struct Scope(u64);
impl Scope {
    pub(in super::super) fn new() -> Result<Self> {
        let value = NEXT.fetch_add(1, Ordering::Relaxed);
        if value == u64::MAX {
            NEXT.store(u64::MAX, Ordering::Relaxed);
            return Err(Error::Encoding("source host identities exhausted"));
        }
        Ok(Self(value))
    }
}

pub(in super::super) struct Cursor<'image> {
    image: &'image stored::Stored,
    scope: Scope,
    record: usize,
    rows: &'image [Row],
    end: observed::End,
    next: usize,
    pending: Option<usize>,
    inspected: bool,
    terminal: bool,
    refused: bool,
}
pub(in super::super) enum Next<'cursor, 'image> {
    Entry(Entry<'cursor, 'image>),
    End(observed::End),
}
pub(in super::super) struct Entry<'cursor, 'image> {
    image: &'image stored::Stored,
    scope: Scope,
    row: &'image Row,
    inspected: &'cursor mut bool,
    refused: &'cursor mut bool,
}
impl<'image, 'budget> Host<'image, 'budget> {
    pub(in super::super) fn stream(
        &mut self,
        path: &[u8],
    ) -> Result<std::result::Result<Cursor<'image>, i32>> {
        let path = observed::Path::from_bytes(path, &mut self.budget)?;
        let record = self.lookup(&observed::Query::DirectoryStream(path))?;
        let observed::Value::DirectoryStream(transcript) = &self.image.records()[record].value
        else {
            return Err(Error::ObservationType);
        };
        match transcript {
            Transcript::OpenFailed(error) | Transcript::UnionOpenFailed { error, .. } => {
                self.used[record] = true;
                Ok(Err(*error))
            }
            Transcript::Started { rows, end, .. } => {
                self.budget.allocation.items(1)?;
                self.streams = self
                    .streams
                    .checked_add(1)
                    .ok_or(Error::Encoding("source cursor count overflow"))?;
                Ok(Ok(Cursor {
                    image: self.image,
                    scope: self.scope,
                    record,
                    rows,
                    end: *end,
                    next: 0,
                    pending: None,
                    inspected: false,
                    terminal: false,
                    refused: false,
                }))
            }
        }
    }
}
impl<'image> Cursor<'image> {
    pub(in super::super) fn next<'cursor>(
        &'cursor mut self,
        host: &mut Host<'image, '_>,
    ) -> Result<Next<'cursor, 'image>> {
        if self.refused {
            return Err(Error::Unconsumed);
        }
        self.refused = true;
        if self.scope != host.scope || !std::ptr::eq(self.image, host.image) {
            return Err(Error::ObservationType);
        }
        host.budget.allocation.items(1)?;
        if let Some(previous) = self.pending.take()
            && self.rows[previous].inspected.is_some() != self.inspected
        {
            return Err(Error::Unconsumed);
        }
        if self.terminal {
            return Err(Error::Unconsumed);
        }
        let Some(row) = self.rows.get(self.next) else {
            self.terminal = true;
            self.refused = false;
            return Ok(Next::End(self.end));
        };
        self.pending = Some(self.next);
        self.next += 1;
        self.inspected = false;
        self.refused = false;
        Ok(Next::Entry(Entry {
            image: self.image,
            scope: self.scope,
            row,
            inspected: &mut self.inspected,
            refused: &mut self.refused,
        }))
    }
    /// Consuming completion is required even for an empty stream. Dropping or
    /// failing a cursor leaves the Host's pending count outstanding.
    pub(in super::super) fn finish(self, host: &mut Host<'image, '_>) -> Result<()> {
        if self.refused
            || !self.terminal
            || self.pending.is_some()
            || !std::ptr::eq(self.image, host.image)
            || self.scope != host.scope
        {
            return Err(Error::Unconsumed);
        }
        host.budget.allocation.items(1)?;
        host.streams = host.streams.checked_sub(1).ok_or(Error::Unconsumed)?;
        host.used[self.record] = true;
        Ok(())
    }
}
impl<'image> Entry<'_, 'image> {
    pub(in super::super) fn name(&self) -> &'image [u8] {
        &self.row.name
    }
    pub(in super::super) fn inspect(
        self,
        host: &mut Host<'image, '_>,
    ) -> Result<std::result::Result<observed::Kind, i32>> {
        *self.refused = true;
        if self.scope != host.scope || !std::ptr::eq(self.image, host.image) {
            return Err(Error::ObservationType);
        }
        host.budget.allocation.items(1)?;
        if observed::stream::hint(self.row.raw_kind).is_none() {
            host.budget.graph.paths(1)?;
        }
        let result = self.row.inspected.ok_or(Error::Unobserved)?;
        *self.inspected = true;
        *self.refused = false;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
