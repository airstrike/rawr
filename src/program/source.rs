//! Source promoted into a resolved program.

use crate::{diagnostic, source, Shared};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A dense source identity local to one resolved program.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Id(usize);

/// An opaque source key bound to exactly one compiled source table.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Key {
    table: TableId,
    index: usize,
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct TableId(usize);

/// One immutable table of exact source snapshots retained by an executable.
#[derive(Clone)]
pub(crate) struct Table(Shared<TableData>);

struct TableData {
    id: TableId,
    sources: Box<[diagnostic::Source]>,
}

/// A source key did not belong to the table asked to resolve it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    ForeignTable,
    MissingSource,
}

/// Source text belonging to a successfully resolved program.
///
/// A source receives this type only as part of the complete resolver
/// transformation.
///
/// ```compile_fail
/// let map = rawr::source::Map::new("40 + 2");
/// let _ = rawr::program::Source(map);
/// ```
#[derive(Clone)]
pub struct Source {
    entry: Id,
    maps: Box<[source::Map]>,
    table: Table,
    aggregate: bool,
}

impl Id {
    pub(crate) const ROOT: Self = Self(0);

    #[cfg(not(feature = "no_std"))]
    pub(crate) const fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

impl Source {
    pub(crate) fn from_resolved(map: source::Map, evidence: diagnostic::Source) -> Self {
        Self {
            entry: Id::ROOT,
            maps: vec![map].into_boxed_slice(),
            table: Table::new(vec![evidence].into_boxed_slice()),
            aggregate: false,
        }
    }

    #[cfg(not(feature = "no_std"))]
    pub(crate) fn from_graph(
        entry: Id,
        maps: Box<[source::Map]>,
        evidence: Box<[diagnostic::Source]>,
    ) -> Self {
        debug_assert_eq!(maps.len(), evidence.len());
        Self {
            entry,
            maps,
            table: Table::new(evidence),
            aggregate: true,
        }
    }

    /// Access the resolved program's original source text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.maps[self.entry.index()].text()
    }

    #[cfg_attr(feature = "no_function", allow(dead_code))]
    pub(crate) fn text_at(&self, id: Id) -> &str {
        self.maps[id.index()].text()
    }

    pub(crate) const fn entry(&self) -> Id {
        self.entry
    }

    pub(crate) const fn is_aggregate(&self) -> bool {
        self.aggregate
    }

    pub(crate) fn key(&self, id: Id) -> Key {
        self.table.key(id.index())
    }

    pub(crate) const fn table(&self) -> &Table {
        &self.table
    }
}

impl TableId {
    fn issue() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);

        #[allow(deprecated)] // MSRV 1.91 predates `AtomicUsize::try_update`.
        let issued = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("Rawr source-table identifiers cannot be exhausted");
        Self(issued)
    }
}

impl Table {
    fn new(sources: Box<[diagnostic::Source]>) -> Self {
        Self(Shared::new(TableData {
            id: TableId::issue(),
            sources,
        }))
    }

    pub(crate) fn key(&self, index: usize) -> Key {
        debug_assert!(index < self.0.sources.len());
        Key {
            table: self.0.id,
            index,
        }
    }

    pub(crate) fn resolve(&self, key: Key) -> Result<&diagnostic::Source, Error> {
        if key.table != self.0.id {
            return Err(Error::ForeignTable);
        }
        self.0.sources.get(key.index).ok_or(Error::MissingSource)
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SourceKey(<private>)")
    }
}

impl fmt::Debug for Table {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SourceTable(<private>)")
    }
}

impl fmt::Debug for Source {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Source")
            .field("source_count", &self.maps.len())
            .field("evidence", &"<private>")
            .field("aggregate", &self.aggregate)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, Table};

    fn source(identity: &str, text: &str) -> crate::diagnostic::Source {
        crate::diagnostic::Source::new(identity, text).unwrap()
    }

    #[test]
    fn keys_resolve_only_against_the_table_that_issued_them() {
        let first = Table::new(vec![source("first.rs", "first")].into_boxed_slice());
        let second = Table::new(vec![source("second.rs", "second")].into_boxed_slice());
        let key = first.key(0);

        assert_eq!(first.resolve(key).unwrap().text(), "first");
        assert_eq!(second.resolve(key), Err(Error::ForeignTable));
    }

    #[test]
    fn debug_output_never_enumerates_source_identity_or_text() {
        let table = Table::new(
            vec![source("secret-identity.rs", "secret source text")].into_boxed_slice(),
        );
        let rendered = format!("{table:?} {:?}", table.key(0));

        assert!(!rendered.contains("secret-identity"));
        assert!(!rendered.contains("secret source text"));
    }
}
