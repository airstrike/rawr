//! Scoped package identity; initial storage alone is not producer-history proof.
use super::source;
use crate::input::membership::manifest::{
    session::Session,
    spec::Full,
    storage::{self, slots},
    Error, Step,
};
use rawr_toml::document;
use std::cmp::Ordering;

pub(in super::super) fn normalized<'a, 's: 'a, A: document::Admission>(
    package: &super::super::normalize::package::View<'_, '_>,
    owner: &mut Owner<'a, 's>,
    source: source::Id<'s>,
    sources: &source::Owner<'s>,
    session: &mut Session<'_, A>,
) -> Step<Id<'a, 's>> {
    use crate::input::membership::manifest::{
        load::normalize::{library, package as normalized},
        schema::{package::Field, primitive},
    };
    session.items(1)?;
    let Some(name) = package.get(Field::Name).and_then(library::text) else {
        return session.fail(Error::State);
    };
    let name = super::super::formatted(format_args!("{name}"), session)?;
    session.items(1)?;
    let version = match package.get(Field::Version) {
        Some(normalized::Value::Original(primitive::Value::Version(version))) => Some(version),
        None => None,
        _ => return session.fail(Error::State),
    };
    let version = Full::from_normalized(version, session)?;
    owner.intern(name, version, source, sources, session)
}

pub(in super::super) struct Record<'s> {
    name: String,
    version: Full,
    source: source::Id<'s>,
}
#[derive(Clone, Copy)]
pub(in super::super) struct Id<'a, 's>(&'a Record<'s>);
pub(in super::super) struct Owner<'a, 's> {
    records: slots::Writer<'a, Record<'s>>,
    index: storage::Buffer<&'a Record<'s>>,
    refused: bool,
}
impl<'a, 's: 'a> Owner<'a, 's> {
    pub(in super::super) fn from_slots(records: slots::Writer<'a, Record<'s>>) -> Self {
        Self {
            records,
            index: storage::Buffer::new(),
            refused: false,
        }
    }
    /// Caller has already retained the name and cloned the complete version.
    pub(in super::super) fn intern<A: document::Admission>(
        &mut self,
        name: String,
        version: Full,
        source: source::Id<'s>,
        sources: &source::Owner<'s>,
        session: &mut Session<'_, A>,
    ) -> Step<Id<'a, 's>> {
        session.check()?;
        if self.refused {
            return session.fail(Error::State);
        }
        self.refused = true;
        sources.get(source, session)?;
        for existing in self.index.as_slice() {
            session.items(1)?;
            session.string(existing.name.len())?;
            session.string(name.len())?;
            session.items(existing.name.len())?;
            session.items(name.len())?;
            if existing.name == name
                && existing.version.equal(&version, session)?
                && existing.source == source
            {
                self.refused = false;
                return Ok(Id(existing));
            }
        }
        let record = self.records.push(
            Record {
                name,
                version,
                source,
            },
            session,
        )?;
        session.items(self.index.len())?;
        self.index.push(record, session)?;
        self.refused = false;
        Ok(Id(record))
    }
}
impl<'a, 's> Id<'a, 's> {
    pub(in super::super) fn name(&self) -> &str {
        &self.0.name
    }
    pub(in super::super) fn version(&self) -> &Full {
        &self.0.version
    }
    pub(in super::super) fn source(&self) -> source::Id<'s> {
        self.0.source
    }
    pub(in super::super) fn public_equal<A: document::Admission>(
        self,
        other: Self,
        sources: &source::Owner<'s>,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.check()?;
        sources.get(self.0.source, session)?;
        sources.get(other.0.source, session)?;
        if std::ptr::eq(self.0, other.0) {
            return Ok(true);
        }
        session.string(self.0.name.len())?;
        session.string(other.0.name.len())?;
        session.items(self.0.name.len())?;
        session.items(other.0.name.len())?;
        if self.0.name != other.0.name || !self.0.version.equal(&other.0.version, session)? {
            return Ok(false);
        }
        Ok(sources.public_cmp(self.0.source, other.0.source, session)? == Ordering::Equal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, spec};
    #[test]
    fn precise_sources_keep_separate_records_with_public_equality() {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let mut history = source::History::new();
        let mut sources = history.open(&mut session).unwrap();
        let url = "https://example.com/index";
        let a = sources
            .intern(
                spec::url::Url::parse(url, &mut session).unwrap().unwrap(),
                source::Origin::Registry,
                None,
                None,
                &mut session,
            )
            .unwrap()
            .ok()
            .unwrap();
        let b = sources
            .intern(
                spec::url::Url::parse(url, &mut session).unwrap().unwrap(),
                source::Origin::Registry,
                Some(source::Precise::Locked),
                None,
                &mut session,
            )
            .unwrap()
            .ok()
            .unwrap();
        let mut records = slots::Slots::new();
        let mut owner = Owner::from_slots(records.writer().unwrap());
        let mut add = |source, text| {
            let name = crate::input::membership::manifest::load::formatted(
                format_args!("p"),
                &mut session,
            )
            .unwrap();
            let spec = spec::Spec::parse(text, None, &mut session).unwrap();
            let version = spec.full_version(&mut session).unwrap().unwrap();
            owner
                .intern(name, version, source, &sources, &mut session)
                .unwrap()
        };
        let first = add(a, "p@1.2.3+one");
        let same = add(a, "p@1.2.3+one");
        let precise = add(b, "p@1.2.3+one");
        let build = add(a, "p@1.2.3+two");
        assert!(std::ptr::eq(first.0, same.0));
        assert!(!std::ptr::eq(first.0, precise.0));
        assert!(first.public_equal(precise, &sources, &mut session).unwrap());
        assert!(!first.public_equal(build, &sources, &mut session).unwrap());
    }
}
