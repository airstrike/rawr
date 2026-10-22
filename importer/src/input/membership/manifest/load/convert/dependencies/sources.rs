// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Cargo's name_in_toml -> public source identity consistency check.
use super::super::source;
use super::gather;
use crate::input::membership::manifest::{
    load::normalize::target, session::Session, storage, Step,
};
use rawr_toml::document;
use std::cmp::Ordering;

pub(super) fn check<A: document::Admission>(
    dependencies: &gather::Collected<'_, '_>,
    sources: &source::Owner<'_>,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.check()?;
    let mut names: storage::Buffer<(&str, source::Id<'_>)> = storage::Buffer::new();
    for dependency in dependencies.as_slice() {
        session.items(1)?;
        let fields = dependency.fields();
        let name = fields.rename().unwrap_or(fields.name());
        let id = fields.source();
        let mut low = 0;
        let mut high = names.len();
        let mut found = None;
        while low < high {
            session.items(1)?;
            let middle = low + (high - low) / 2;
            let (previous, _) = names.as_slice()[middle];
            session.items(previous.len().min(name.len()))?;
            match previous.cmp(name) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => {
                    found = Some(middle);
                    break;
                }
            }
        }
        if let Some(index) = found {
            let previous = std::mem::replace(&mut names.as_mut_slice()[index].1, id);
            if sources.public_cmp(previous, id, session)? != Ordering::Equal {
                return target::formatted(format_args!(
                    "Dependency '{name}' has different source paths depending on the build target. Each dependency must have a single canonical source path irrespective of build target."
                ), session).map(Err);
            }
        } else {
            // Flat ordered storage replaces BTreeMap's private nodes. Charge
            // growth and shifting before mutation; no key cloning is needed.
            insert(&mut names, low, (name, id), session)?;
        }
    }
    Ok(Ok(()))
}

fn insert<'a, 's, A: document::Admission>(
    names: &mut storage::Buffer<(&'a str, source::Id<'s>)>,
    at: usize,
    entry: (&'a str, source::Id<'s>),
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.items(names.len())?;
    session.items(names.len() - at)?;
    names.push(entry, session)?;
    names.as_mut_slice()[at..].rotate_right(1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, spec::url::Url, Stop};
    #[test]
    fn ascending_growth_refuses_before_moving_existing_entries() {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let mut history = source::History::new();
        let mut sources = history.open(&mut session).unwrap();
        let url = Url::parse("file:///p", &mut session).unwrap().unwrap();
        let id = sources
            .intern(url, source::Origin::Path, None, None, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        let mut names = storage::Buffer::new();
        for name in ["a", "b", "c", "d"] {
            let at = names.len();
            insert(&mut names, at, (name, id), &mut session).unwrap();
        }
        struct Refuse;
        impl document::Admission for Refuse {
            type Error = ();
            fn admit(&mut self, request: document::Request) -> Result<(), ()> {
                assert!(matches!(request, document::Request::Items(4)));
                Err(())
            }
        }
        let mut refuse = Refuse;
        let mut session = Session::new(&mut refuse);
        assert!(matches!(
            insert(&mut names, 4, ("e", id), &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(names.len(), 4);
        assert_eq!(names.as_slice()[0].0, "a");
        assert!(matches!(
            insert(&mut names, 4, ("e", id), &mut session),
            Err(Stop::Refused)
        ));
    }
}
