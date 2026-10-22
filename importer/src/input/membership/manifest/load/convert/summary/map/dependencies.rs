//! Dependency optionality in the pinned Cargo entry/iteration order.
use crate::input::membership::manifest::{
    load::{config, convert::dependency},
    session::Session,
    Step,
};
use rawr_toml::document;

// Cargo's (&str-backed InternedString, bool) and the existing inert-index
// specialization have the same entry layout and growth branch on this target.
const _: () = {
    assert!(std::mem::size_of::<(&str, bool)>() == std::mem::size_of::<(&str, usize)>());
    assert!(std::mem::align_of::<(&str, bool)>() == std::mem::align_of::<(&str, usize)>());
};
pub(super) struct Map<'a>(config::Table<'a>);
impl<'a> Map<'a> {
    pub(super) fn build<A: document::Admission>(
        dependencies: &'a [dependency::Converted<'_, '_>],
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut map = Self(config::Table::new());
        for dependency in dependencies {
            session.items(1)?;
            let fields = dependency.fields();
            map.insert(
                fields.rename().unwrap_or(fields.name()),
                fields.optional(),
                session,
            )?;
        }
        Ok(map)
    }
    pub(super) fn get<A: document::Admission>(
        &self,
        name: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Option<bool>> {
        Ok(self.0.get(name, session)?.map(|value| value != 0))
    }
    pub(super) fn insert<A: document::Admission>(
        &mut self,
        name: &'a str,
        optional: bool,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        // Cargo entry searches before reserving, then ORs into the incumbent.
        if let Some(previous) = self.0.merge(name, usize::from(optional), session)? {
            self.0
                .replace(name, previous | usize::from(optional), session)?;
        }
        Ok(())
    }
    pub(super) fn first_unused<A: document::Admission>(
        &self,
        used: &config::Table<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Option<&'a str>> {
        let mut entries = self.0.iter(session)?;
        while let Some((name, optional)) = entries.next(session)? {
            if optional == 0 {
                continue;
            }
            if used.get(name, session)?.is_none() {
                return Ok(Some(name));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::reference;
    #[test]
    fn dependency_optionality_accumulates_across_declarations() {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let mut map = Map::build(&[], &mut session).unwrap();
        for (name, optional) in [
            ("z", false),
            ("a", true),
            ("z", true),
            ("a", false),
            ("required", false),
        ] {
            map.insert(name, optional, &mut session).unwrap();
        }
        assert_eq!(map.get("z", &mut session).unwrap(), Some(true));
        assert_eq!(map.get("a", &mut session).unwrap(), Some(true));
        assert_eq!(map.get("required", &mut session).unwrap(), Some(false));
        assert_eq!(map.get("missing", &mut session).unwrap(), None);
    }
}
