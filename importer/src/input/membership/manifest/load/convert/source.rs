//! Bounded source identity, without producer-history completion authority.

#[cfg(test)]
mod tests;
mod value;

use super::super::super::{
    session::Session,
    spec::url::{Canonical, Url},
    storage::{self, slots},
    Error, Step,
};
use rawr_toml::document;
use std::{cell::Cell, cmp::Ordering};
use value::Owned;
pub(in super::super) use value::{Git, Kind, Label, Origin, Precise, Selector};

/// External stable record scope. Empty storage alone proves no producer prelude.
pub(in super::super) struct History {
    records: slots::Slots<Record>,
    scope: Scope,
}
struct Scope {
    _nonzero: u8,
}

pub(in super::super) struct Owner<'a> {
    records: slots::Writer<'a, Record>,
    scope: &'a Scope,
    index: storage::Buffer<&'a Record>,
    refused: Cell<bool>,
}

/// Full local identity is distinct from Cargo's public comparison.
#[derive(Clone, Copy)]
pub(in super::super) struct Id<'a> {
    record: &'a Record,
    scope: &'a Scope,
}
impl PartialEq for Id<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.scope, other.scope) && std::ptr::eq(self.record, other.record)
    }
}
impl Eq for Id<'_> {}

pub(in super::super) struct Record {
    url: Url,
    canonical: Canonical,
    value: Owned,
}

impl History {
    pub(in super::super) const fn new() -> Self {
        Self {
            records: slots::Slots::new(),
            scope: Scope { _nonzero: 0 },
        }
    }
    pub(in super::super) fn open<A: document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Owner<'_>> {
        session.items(1)?;
        let Some(records) = self.records.writer() else {
            return session.fail(Error::State);
        };
        Ok(Owner {
            records,
            scope: &self.scope,
            index: storage::Buffer::new(),
            refused: Cell::new(false),
        })
    }
}

impl<'a> Owner<'a> {
    /// Cargo SourceId::for_manifest_path, before a recursive manifest read.
    pub(in super::super) fn manifest(
        &mut self,
        path: &std::path::Path,
        session: &mut Session<'_, crate::input::membership::host::Host<'_, '_>>,
    ) -> Step<Result<Id<'a>, String>> {
        use std::os::unix::ffi::OsStrExt;
        self.check(session)?;
        self.refused.set(true);
        let path = if super::super::read::is_embedded(path)
            && session.is_file(path.as_os_str().as_bytes())?
        {
            path
        } else {
            let Some(parent) = path.parent() else {
                return session.fail(Error::State);
            };
            parent
        };
        let url = match super::url::path(path, session)? {
            Ok(url) => url,
            Err(error) => {
                self.refused.set(false);
                return Ok(Err(error));
            }
        };
        let value = Owned::from(Kind::Path, None, None, session)?;
        self.insert(url, value, session)
    }
    fn check<A: document::Admission>(&self, session: &mut Session<'_, A>) -> Step<()> {
        session.check()?;
        if self.refused.get() {
            return session.fail(Error::State);
        }
        Ok(())
    }

    /// All candidate payload copies and canonicalization occur before lookup.
    pub(in super::super) fn intern<A: document::Admission>(
        &mut self,
        url: Url,
        origin: Origin<'_>,
        precise: Option<Precise<'_>>,
        label: Option<Label<'_>>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Id<'a>, String>> {
        self.check(session)?;
        self.refused.set(true);
        let kind = match origin {
            Origin::Path => Kind::Path,
            Origin::Git(git) => Kind::Git(git),
            Origin::Registry => {
                if url.as_str(session)?.starts_with("sparse+") {
                    Kind::SparseRegistry
                } else {
                    Kind::Registry
                }
            }
        };
        let value = Owned::from(kind, precise, label, session)?;
        self.insert(url, value, session)
    }

    pub(in super::super) fn git<A: document::Admission>(
        &mut self,
        url: &Url,
        selector: Selector,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Id<'a>, String>> {
        self.check(session)?;
        self.refused.set(true);
        let copy = url.copy(session)?;
        self.insert(copy, Owned::git(selector), session)
    }

    pub(in super::super) fn registry<A: document::Admission>(
        &mut self,
        url: &Url,
        label: Option<&str>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Id<'a>, String>> {
        self.check(session)?;
        self.refused.set(true);
        let kind = if url.as_str(session)?.starts_with("sparse+") {
            Kind::SparseRegistry
        } else {
            Kind::Registry
        };
        let value = Owned::from(kind, None, label.map(Label::Registry), session)?;
        let copy = url.copy(session)?;
        self.insert(copy, value, session)
    }

    fn insert<A: document::Admission>(
        &mut self,
        url: Url,
        value: Owned,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Id<'a>, String>> {
        let canonical = match Canonical::from(&url, session)? {
            Ok(canonical) => canonical,
            Err(error) => {
                self.refused.set(false);
                return Ok(Err(error));
            }
        };
        let candidate = Record {
            url,
            canonical,
            value,
        };
        let mut low = 0;
        let mut high = self.index.len();
        while low < high {
            let middle = low + (high - low) / 2;
            let existing = self.index.as_slice()[middle];
            match compare(existing, &candidate, true, session)? {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => {
                    self.refused.set(false);
                    return Ok(Ok(Id {
                        record: existing,
                        scope: self.scope,
                    }));
                }
            }
        }
        // Existing Buffer may move the complete index during replacement;
        // additionally insertion moves exactly this suffix. Admit before either.
        let count = self.index.len();
        session.items(count)?;
        session.items(count - low)?;
        let record = self.records.push(candidate, session)?;
        self.index.push(record, session)?;
        self.index.as_mut_slice()[low..].rotate_right(1);
        self.refused.set(false);
        Ok(Ok(Id {
            record,
            scope: self.scope,
        }))
    }

    pub(in super::super) fn get<A: document::Admission>(
        &self,
        id: Id<'a>,
        session: &mut Session<'_, A>,
    ) -> Step<&'a Record> {
        self.check(session)?;
        self.refused.set(true);
        if !std::ptr::eq(self.scope, id.scope) {
            return session.fail(Error::State);
        }
        self.refused.set(false);
        Ok(id.record)
    }

    pub(in super::super) fn public_cmp<A: document::Admission>(
        &self,
        left: Id<'a>,
        right: Id<'a>,
        session: &mut Session<'_, A>,
    ) -> Step<Ordering> {
        let left = self.get(left, session)?;
        let right = self.get(right, session)?;
        if std::ptr::eq(left, right) {
            return Ok(Ordering::Equal);
        }
        self.refused.set(true);
        let result = compare(left, right, false, session)?;
        self.refused.set(false);
        Ok(result)
    }
}

impl Record {
    pub(in super::super) fn url<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<&Url> {
        session.check()?;
        Ok(&self.url)
    }
    pub(in super::super) fn label(&self) -> Option<Label<'_>> {
        self.value.label()
    }
    pub(in super::super) fn kind(&self) -> Kind<'_> {
        self.value.kind()
    }
}

fn compare<A: document::Admission>(
    left: &Record,
    right: &Record,
    precise: bool,
    session: &mut Session<'_, A>,
) -> Step<Ordering> {
    session.items(1)?;
    let kind = left.value.compare_kind(&right.value, session)?;
    if kind != Ordering::Equal {
        return Ok(kind);
    }
    if precise {
        let precise = left.value.compare_precise(&right.value, session)?;
        if precise != Ordering::Equal {
            return Ok(precise);
        }
    }
    let left = left.canonical.as_str(session)?;
    let right = right.canonical.as_str(session)?;
    session.string(left.len())?;
    session.string(right.len())?;
    session.items(left.len())?;
    session.items(right.len())?;
    Ok(left.cmp(right))
}
