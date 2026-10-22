//! Per-value checks, before the final unused-optional-dependency check.
use super::*;
use crate::input::membership::manifest::load::normalize::suggest;
mod name;

pub(in super::super::super::super) enum Failure<'a> {
    Message(String),
    Missing {
        feature: &'a str,
        value: &'a Value,
        dependency: &'a str,
        weak: bool,
    },
}
fn fail<'a, A: document::Admission>(
    args: std::fmt::Arguments<'_>,
    session: &mut Session<'_, A>,
) -> Step<Result<(), Failure<'a>>> {
    formatted(args, session).map(|message| Err(Failure::Message(message)))
}
impl Map<'_> {
    pub(in super::super::super::super) fn validate<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure<'_>>> {
        if let Err(error) = self.references(session)? {
            return Ok(Err(error));
        }
        let mut used = crate::input::membership::manifest::load::config::Table::new();
        for entry in self.entries() {
            session.items(1)?;
            for value in entry.values.as_slice() {
                session.items(1)?;
                match value {
                    Value::Dependency(name) | Value::DependencyFeature { name, .. } => {
                        used.insert(name, 0, session)?;
                    }
                    Value::Feature(_) => {}
                }
            }
        }
        if let Some(dependency) = self.dependencies.first_unused(&used, session)? {
            return fail(format_args!("optional dependency `{dependency}` is not included in any feature\nMake sure that `dep:{dependency}` is included in one of features in the [features] table."), session);
        }
        Ok(Ok(()))
    }
    /// Complete key/value traversal; unused optional dependencies are checked later.
    pub(in super::super::super::super) fn references<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure<'_>>> {
        session.check()?;
        for entry in self.entries() {
            session.items(1)?;
            if let Err(error) = name::validate(&entry.name, session)? {
                return Ok(Err(Failure::Message(error)));
            }
            for value in entry.values.as_slice() {
                session.items(1)?;
                if let Err(error) = self.validate_value(&entry.name, value, session)? {
                    return Ok(Err(error));
                }
            }
        }
        Ok(Ok(()))
    }
    fn find<A: document::Admission>(
        &self,
        name: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Option<&Entry>> {
        for entry in self.entries() {
            session.items(1)?;
            session.items(name.len())?;
            session.items(entry.name.len())?;
            if entry.name == name {
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }
    fn explicit_dependency<A: document::Admission>(
        &self,
        name: &str,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        for entry in self.entries() {
            session.items(1)?;
            if !entry.explicit {
                continue;
            }
            for value in entry.values.as_slice() {
                session.items(1)?;
                if let Some(dependency) = value.explicit_dependency() {
                    session.items(name.len())?;
                    session.items(dependency.len())?;
                    if name == dependency {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
    pub(in super::super::super::super) fn validate_value<'a, A: document::Admission>(
        &self,
        feature: &'a str,
        value: &'a Value,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure<'a>>> {
        session.string(feature.len())?;
        let dependency = self.dependency(value.name(), session)?;
        match value {
            Value::Feature(name) => {
                let found = self.find(name, session)?;
                if !found.is_some_and(|entry| entry.explicit) {
                    match dependency {
                        None => {
                            session.items(self.entries().len())?;
                            let closest = suggest::select(name, self.entries().iter().filter(|entry| entry.explicit).map(|entry| entry.name.as_str()), session)?;
                            let help = match closest {
                                Some(name) => formatted(format_args!("\n\nhelp: a feature with a similar name exists: `{name}`"), session)?,
                                None => String::new(),
                            };
                            return fail(format_args!("feature `{feature}` includes `{value}` which is neither a dependency nor another feature{help}"), session);
                        }
                        Some(true) if found.is_none() => return fail(format_args!("feature `{feature}` includes `{value}`, but `{name}` is an optional dependency without an implicit feature\nUse `dep:{name}` to enable the dependency."), session),
                        Some(false) => return fail(format_args!("feature `{feature}` includes `{value}`, but `{name}` is not an optional dependency\nA non-optional dependency of the same name is defined; consider adding `optional = true` to its definition."), session),
                        _ => {}
                    }
                }
            }
            Value::Dependency(name) => match dependency {
                None => return fail(format_args!("feature `{feature}` includes `{value}`, but `{name}` is not listed as a dependency"), session),
                Some(false) => return fail(format_args!("feature `{feature}` includes `{value}`, but `{name}` is not an optional dependency\nA non-optional dependency of the same name is defined; consider adding `optional = true` to its definition."), session),
                _ => {}
            },
            Value::DependencyFeature { name, feature: child, weak } => {
                session.items(child.len())?;
                if child.contains('/') { return fail(format_args!("multiple slashes in feature `{value}` (included by feature `{feature}`) are not allowed"), session); }
                if let Some(stripped) = name.strip_prefix("dep:") {
                    let other = self.explicit_dependency(stripped, session)?;
                    let optional = self.dependency(stripped, session)? == Some(true);
                    let help = if *weak || other || !optional { String::new() } else {
                        formatted(format_args!("\nIf the intent is to avoid creating an implicit feature `{stripped}` for an optional dependency, then consider replacing this with two values:\n    \"dep:{stripped}\", \"{stripped}/{child}\""), session)?
                    };
                    return fail(format_args!("feature `{feature}` includes `{value}` with both `dep:` and `/`\nTo fix this, remove the `dep:` prefix.{help}"), session);
                }
                if dependency.is_none() { return Ok(Err(Failure::Missing { feature, value, dependency: name, weak: *weak })); }
                if *weak && dependency != Some(true) { return fail(format_args!("feature `{feature}` includes `{value}` with a `?`, but `{name}` is not an optional dependency\nA non-optional dependency of the same name is defined; consider removing the `?` or changing the dependency to be optional"), session); }
            }
        }
        Ok(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, value};
    #[test]
    fn unused_optional_check_follows_references_and_counts_dependency_features() {
        let parsed = document::parse(
            "[features]\noptional=[]\nuse=['optional?/child']",
            256,
            &mut reference::Account,
        )
        .unwrap();
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let features = Features::interpret(
            value::Input::raw(parsed.table().get("features").unwrap()),
            &mut session,
        )
        .unwrap();
        let mut map = Map::build(Some(&features), &[], &mut session).unwrap();
        map.dependencies
            .insert("optional", true, &mut session)
            .unwrap();
        assert!(map.validate(&mut session).unwrap().is_ok());
        map.dependencies
            .insert("unused", true, &mut session)
            .unwrap();
        let Err(Failure::Message(message)) = map.validate(&mut session).unwrap() else {
            panic!("unused")
        };
        assert_eq!(message, "optional dependency `unused` is not included in any feature\nMake sure that `dep:unused` is included in one of features in the [features] table.");
        map.implicit("a::invalid", &mut session).unwrap();
        let Err(Failure::Message(message)) = map.validate(&mut session).unwrap() else {
            panic!("name")
        };
        assert!(message.starts_with("invalid character `:`"));
    }
    #[test]
    fn map_reference_traversal_checks_each_name_before_its_values() {
        let parsed =
            document::parse("[features]\nz=['missing']", 256, &mut reference::Account).unwrap();
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let features = Features::interpret(
            value::Input::raw(parsed.table().get("features").unwrap()),
            &mut session,
        )
        .unwrap();
        let mut map = Map::build(Some(&features), &[], &mut session).unwrap();
        map.implicit("a::name", &mut session).unwrap();
        let Err(Failure::Message(message)) = map.references(&mut session).unwrap() else {
            panic!("name")
        };
        assert_eq!(message, "invalid character `:` in feature name: `a::name`, characters must be Unicode XID characters, '-', `+`, or `.` (numbers, `+`, `-`, `_`, `.`, or most letters)");
        let empty = Map::build(None, &[], &mut session).unwrap();
        assert!(empty.references(&mut session).unwrap().is_ok());
    }
    #[test]
    fn feature_references_distinguish_optional_required_and_hidden_dependencies() {
        let parsed = document::parse(
            "[features]\nfeature=[]\nhide=['dep:hidden']",
            256,
            &mut reference::Account,
        )
        .unwrap();
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let features = Features::interpret(
            value::Input::raw(parsed.table().get("features").unwrap()),
            &mut session,
        )
        .unwrap();
        let mut map = Map::build(Some(&features), &[], &mut session).unwrap();
        for (name, optional) in [
            ("required", false),
            ("optional", true),
            ("hidden", true),
            ("feature", false),
        ] {
            map.dependencies
                .insert(name, optional, &mut session)
                .unwrap();
            if optional {
                map.implicit(name, &mut session).unwrap();
            }
        }
        for text in [
            "feature",
            "optional",
            "dep:optional",
            "dep:hidden",
            "required/child",
            "optional?/child",
        ] {
            let value = Value::parse(text, &mut session).unwrap();
            assert!(
                map.validate_value("owner", &value, &mut session)
                    .unwrap()
                    .is_ok(),
                "{text}"
            );
        }
        for (text, expected) in [
            ("required", "feature `owner` includes `required`, but `required` is not an optional dependency\nA non-optional dependency of the same name is defined; consider adding `optional = true` to its definition."),
            ("hidden", "feature `owner` includes `hidden`, but `hidden` is an optional dependency without an implicit feature\nUse `dep:hidden` to enable the dependency."),
            ("required?/child", "feature `owner` includes `required?/child` with a `?`, but `required` is not an optional dependency\nA non-optional dependency of the same name is defined; consider removing the `?` or changing the dependency to be optional"),
            ("dep:optional/child", "feature `owner` includes `dep:optional/child` with both `dep:` and `/`\nTo fix this, remove the `dep:` prefix.\nIf the intent is to avoid creating an implicit feature `optional` for an optional dependency, then consider replacing this with two values:\n    \"dep:optional\", \"optional/child\""),
        ] {
            let value = Value::parse(text, &mut session).unwrap();
            let Err(Failure::Message(message)) = map.validate_value("owner", &value, &mut session).unwrap() else { panic!("{text}") };
            assert_eq!(message, expected);
        }
    }
    #[test]
    fn feature_reference_diagnostics_keep_suggestions_and_structured_missing() {
        let parsed =
            document::parse("[features]\nfeature=[]", 256, &mut reference::Account).unwrap();
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let features = Features::interpret(
            value::Input::raw(parsed.table().get("features").unwrap()),
            &mut session,
        )
        .unwrap();
        let map = Map::build(Some(&features), &[], &mut session).unwrap();
        for (text, expected) in [
            ("featur", "feature `owner` includes `featur` which is neither a dependency nor another feature\n\nhelp: a feature with a similar name exists: `feature`"),
            ("dep:absent", "feature `owner` includes `dep:absent`, but `absent` is not listed as a dependency"),
            ("dep:absent/a/b", "multiple slashes in feature `dep:absent/a/b` (included by feature `owner`) are not allowed"),
            ("dep:absent/a", "feature `owner` includes `dep:absent/a` with both `dep:` and `/`\nTo fix this, remove the `dep:` prefix."),
        ] {
            let value = Value::parse(text, &mut session).unwrap();
            let Err(Failure::Message(message)) = map.validate_value("owner", &value, &mut session).unwrap() else { panic!("message") };
            assert_eq!(message, expected);
        }
        let value = Value::parse("absent?/a", &mut session).unwrap();
        let Err(Failure::Missing {
            feature,
            value,
            dependency,
            weak,
        }) = map.validate_value("owner", &value, &mut session).unwrap()
        else {
            panic!("missing")
        };
        assert_eq!((feature, dependency, weak), ("owner", "absent", true));
        assert_eq!(value.to_string(), "absent?/a");
    }
}
