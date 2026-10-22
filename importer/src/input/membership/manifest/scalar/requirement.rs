//! Complete requirements through PartialVersion's shared pinned grammar.

use super::super::{session::Session, Error, Step};
use super::{partial, version};
use rawr_toml::document;
use std::alloc::Layout;

pub(in super::super) use partial::Op;
pub(in super::super) use version::Rejection;

#[derive(Debug)]
pub(in super::super) struct Requirement {
    comparators: Vec<Comparator>,
}
#[derive(Debug)]
pub(in super::super) struct Comparator {
    pub(in super::super) op: Op,
    pub(in super::super) major: u64,
    pub(in super::super) minor: Option<u64>,
    pub(in super::super) patch: Option<u64>,
    pre: Identifier,
}
impl Comparator {
    pub(in super::super) fn pre(&self) -> &str {
        self.pre.as_str()
    }
}

#[derive(Debug)]
enum Identifier {
    Empty,
    Text(String),
}
impl Identifier {
    fn as_str(&self) -> &str {
        match self {
            Self::Empty => "",
            Self::Text(value) => value,
        }
    }
    fn retain<A: document::Admission>(text: &str, session: &mut Session<'_, A>) -> Step<Self> {
        session.string(text.len())?;
        if text.is_empty() {
            return Ok(Self::Empty);
        }
        let layout = Layout::array::<u8>(text.len());
        let Ok(layout) = layout else {
            return session.fail(Error::Overflow);
        };
        session.admit(document::Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        let mut value = String::new();
        if let Err(error) = value.try_reserve_exact(text.len()) {
            return session.fail(Error::Allocation(error));
        }
        value.push_str(text);
        Ok(Self::Text(value))
    }
}

impl Requirement {
    /// Cargo 59800466's util/semver_ext.rs VersionExt::to_exact_req, from an
    /// already validated full version. The pinned vec! allocates its boxed
    /// array before evaluating the comparator's prerelease clone. Build
    /// metadata is retained by Full but is not part of the requirement.
    pub(in super::super) fn exact<A: document::Admission>(
        version: &super::super::spec::Full,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let layout = Layout::new::<Comparator>();
        session.admit(document::Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        let mut comparators = Vec::new();
        if let Err(error) = comparators.try_reserve_exact(1) {
            return session.fail(Error::Allocation(error));
        }
        let pre = Identifier::retain(version.pre(), session)?;
        comparators.push(Comparator {
            op: Op::Exact,
            major: version.major(),
            minor: Some(version.minor()),
            patch: Some(version.patch()),
            pre,
        });
        Ok(Self { comparators })
    }
    pub(in super::super) fn comparators(&self) -> &[Comparator] {
        &self.comparators
    }
    pub(in super::super) fn parse<A: document::Admission>(
        text: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Rejection>> {
        session.string(text.len())?;
        // The source grammar admits at most 32 comparators. Fixed staging
        // replaces its depth-limited recursive stack, without heap reservation.
        let mut staged: [Option<Comparator>; 32] = std::array::from_fn(|_| None);
        let mut length = 0;
        let mut pre = Identifier::Empty;
        let result = partial::capture(text, |event| {
            session.items(1)?;
            match event {
                partial::Event::Pre(text) => pre = Identifier::retain(text, session)?,
                partial::Event::Build(text) => {
                    drop(Identifier::retain(text, session)?);
                }
                partial::Event::Comparator(value) => {
                    let Some(slot) = staged.get_mut(length) else {
                        return session.fail(Error::State);
                    };
                    *slot = Some(Comparator {
                        op: value.op,
                        major: value.major,
                        minor: value.minor,
                        patch: value.patch,
                        pre: std::mem::replace(&mut pre, Identifier::Empty),
                    });
                    length += 1;
                }
            }
            Ok(())
        });
        match result {
            Err(partial::Failure::Grammar(error)) => return Ok(Err(error)),
            Err(partial::Failure::Capture(stop)) => return Err(stop),
            Ok(()) => {}
        }
        let mut comparators = Vec::new();
        if length != 0 {
            let Ok(layout) = Layout::array::<Comparator>(length) else {
                return session.fail(Error::Overflow);
            };
            session.admit(document::Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            })?;
            if let Err(error) = comparators.try_reserve_exact(length) {
                return session.fail(Error::Allocation(error));
            }
        }
        for slot in staged.iter_mut().take(length) {
            session.items(1)?;
            let Some(value) = slot.take() else {
                return session.fail(Error::State);
            };
            comparators.push(value);
        }
        Ok(Ok(Self { comparators }))
    }
}

#[cfg(test)]
mod tests;
