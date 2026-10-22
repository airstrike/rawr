//! Constructed feature values; validation is a separate, required transformation.
use super::value::Value;
use crate::input::membership::manifest::{
    load::{convert::dependency, formatted},
    schema::manifest::Features,
    session::Session,
    storage, Step,
};
use rawr_toml::document;
mod dependencies;
pub(super) mod validate;

pub(in super::super::super) struct Map<'a> {
    dependencies: dependencies::Map<'a>,
    entries: storage::Buffer<Entry>,
}
pub(in super::super::super) struct Entry {
    pub(in super::super::super) name: String,
    pub(in super::super::super) values: storage::Buffer<Value>,
    pub(in super::super::super) explicit: bool,
}
impl<'a> Map<'a> {
    pub(in super::super::super) fn into_entries(self) -> storage::Buffer<Entry> {
        self.entries
    }
    pub(in super::super::super) fn entries(&self) -> &[Entry] {
        self.entries.as_slice()
    }
    pub(in super::super::super) fn build<A: document::Admission>(
        features: Option<&Features<'_>>,
        dependencies: &'a [dependency::Converted<'_, '_>],
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut map = Self {
            dependencies: dependencies::Map::build(dependencies, session)?,
            entries: storage::Buffer::new(),
        };
        if let Some(features) = features {
            for entry in features.entries() {
                session.items(1)?;
                let name = formatted(format_args!("{}", entry.key().as_str()), session)?;
                let mut values = storage::Buffer::new();
                for text in entry.value().as_slice() {
                    session.items(1)?;
                    let value = Value::parse(text.as_str(), session)?;
                    session.items(values.len())?;
                    values.push(value, session)?;
                }
                session.items(map.entries.len())?;
                map.entries.push(
                    Entry {
                        name,
                        values,
                        explicit: true,
                    },
                    session,
                )?;
            }
        }
        for dependency in dependencies {
            session.items(1)?;
            let fields = dependency.fields();
            if fields.optional() {
                map.implicit(fields.rename().unwrap_or(fields.name()), session)?;
            }
        }
        Ok(map)
    }
    pub(in super::super::super) fn dependency<A: document::Admission>(
        &self,
        name: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Option<bool>> {
        self.dependencies.get(name, session)
    }
    fn implicit<A: document::Admission>(
        &mut self,
        name: &str,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let mut position = 0;
        let mut replacement = false;
        for entry in self.entries.as_slice() {
            session.items(1)?;
            session.items(name.len())?;
            session.items(entry.name.len())?;
            match entry.name.as_str().cmp(name) {
                std::cmp::Ordering::Less => position += 1,
                std::cmp::Ordering::Equal if entry.explicit => return Ok(()),
                std::cmp::Ordering::Equal => replacement = true,
                _ => {}
            }
            if !entry.explicit {
                continue;
            }
            for value in entry.values.as_slice() {
                session.items(1)?;
                if let Some(explicit) = value.explicit_dependency() {
                    session.items(explicit.len())?;
                    session.items(name.len())?;
                    if explicit == name {
                        return Ok(());
                    }
                }
            }
        }
        let key = formatted(format_args!("{name}"), session)?;
        let dependency = formatted(format_args!("{name}"), session)?;
        let mut values = storage::Buffer::new();
        values.push(Value::Dependency(dependency), session)?;
        if replacement {
            self.entries.as_mut_slice()[position].values = values;
            return Ok(());
        }
        session.items(self.entries.len())?;
        session.items(self.entries.len() - position)?;
        self.entries.push(
            Entry {
                name: key,
                values,
                explicit: false,
            },
            session,
        )?;
        self.entries.as_mut_slice()[position..].rotate_right(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, value};
    #[test]
    fn feature_map_implicit_suppression_and_key_order() {
        let source = "[features]\nz=['dep:hidden','weak?/x']\nexplicit=[]";
        let parsed = document::parse(source, 256, &mut reference::Account).unwrap();
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let features = Features::interpret(
            value::Input::raw(parsed.table().get("features").unwrap()),
            &mut session,
        )
        .unwrap();
        let mut map = Map::build(Some(&features), &[], &mut session).unwrap();
        for name in ["hidden", "explicit", "weak", "a", "a"] {
            map.implicit(name, &mut session).unwrap();
        }
        assert_eq!(
            map.entries()
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "explicit", "weak", "z"]
        );
        assert!(!map.entries()[0].explicit);
        assert!(map.entries()[1].explicit);
        assert_eq!(
            map.entries()[2].values.as_slice()[0].to_string(),
            "dep:weak"
        );
    }
}
