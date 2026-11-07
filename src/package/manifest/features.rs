//! A package's `[features]`, and what Cargo enables from them.
//!
//! Rawr enables what `cargo run` and `cargo build` enable with no feature
//! flags: the `default` feature, if the package has one, and each feature
//! and optional dependency it reaches.

use std::collections::{BTreeMap, BTreeSet};

use super::{item_span, rust, Error};

/// The `[features]` table: each feature and the values it enables, as the
/// manifest writes them (`name`, `dep:name`, `name/feature` and
/// `name?/feature`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Features {
    table: BTreeMap<String, Box<[String]>>,
}

impl Features {
    /// The `[features]` of `table`, the manifest parsed from `text`.
    pub(crate) fn parse(text: &str, table: &toml_edit::Table) -> Result<Self, Error> {
        let Some(item) = table.get("features") else {
            return Ok(Self::default());
        };
        let features = item.as_table_like().ok_or_else(|| Error {
            message: "Cargo `features` must be a table".into(),
            span: item_span(text, item),
        })?;
        let mut table = BTreeMap::new();
        for (name, item) in features.iter() {
            let values = item
                .as_array()
                .and_then(|values| {
                    values
                        .iter()
                        .map(|value| value.as_str().map(str::to_owned))
                        .collect::<Option<Box<[String]>>>()
                })
                .ok_or_else(|| Error {
                    message: format!("Cargo feature `{name}` must be an array of strings").into(),
                    span: item_span(text, item),
                })?;
            table.insert(name.to_owned(), values);
        }
        Ok(Self { table })
    }

    /// The `[features]` table of a generated package that declares
    /// `declared` (its dependencies and build dependencies), exactly as the
    /// package's. A value that names a dependency the generated package
    /// leaves out (the attribute crate) is left out with it.
    #[cfg(feature = "rust-import")]
    pub(crate) fn manifest(&self, declared: &[&rust::Dependency]) -> String {
        use std::fmt::Write as _;
        if self.table.is_empty() {
            return String::new();
        }
        let declares = |key: &str| declared.iter().any(|dependency| dependency.key == key);
        let mut text = String::from("\n[features]\n");
        for (name, values) in &self.table {
            let values = values
                .iter()
                .filter(|value| match Value::of(value) {
                    Value::Feature(name) => self.table.contains_key(name) || declares(name),
                    Value::Dependency(key) | Value::Forwarded { key, .. } => declares(key),
                })
                .collect::<Vec<_>>();
            let _ = writeln!(text, "{name:?} = {values:?}");
        }
        text
    }

    /// The dependencies of `dependencies` that Cargo builds with the
    /// default features on: each one that is not optional, and each
    /// optional one a feature enables, with the features of its own that
    /// the package's features enable.
    pub(crate) fn enabled(&self, dependencies: &[rust::Dependency]) -> Box<[rust::Dependency]> {
        let optional = |key: &str| {
            dependencies
                .iter()
                .any(|dependency| dependency.optional && dependency.key == key)
        };
        // An optional dependency is a feature of its own name unless a
        // `dep:` value names it.
        let implicit = |key: &str| {
            optional(key)
                && !self.table.contains_key(key)
                && !self
                    .table
                    .values()
                    .flatten()
                    .any(|value| value.strip_prefix("dep:") == Some(key))
        };
        let mut features = BTreeSet::new();
        let mut on = BTreeSet::new();
        let mut asked: Vec<(&str, &str, bool)> = Vec::new();
        let mut pending = vec!["default"];
        while let Some(name) = pending.pop() {
            if !features.insert(name) {
                continue;
            }
            let Some(values) = self.table.get(name) else {
                if implicit(name) {
                    on.insert(name);
                }
                continue;
            };
            for value in values {
                match Value::of(value) {
                    Value::Feature(name) => pending.push(name),
                    Value::Dependency(key) => {
                        on.insert(key);
                    }
                    Value::Forwarded { key, feature, weak } => {
                        asked.push((key, feature, weak));
                        if !weak {
                            on.insert(key);
                            if implicit(key) {
                                pending.push(key);
                            }
                        }
                    }
                }
            }
        }
        dependencies
            .iter()
            .filter(|dependency| !dependency.optional || on.contains(dependency.key.as_str()))
            .map(|dependency| {
                let mut dependency = dependency.clone();
                let mut features = dependency.features.into_vec();
                features.extend(
                    asked
                        .iter()
                        .filter(|(key, _, _)| *key == dependency.key)
                        .map(|(_, feature, _)| (*feature).to_owned()),
                );
                features.sort_unstable();
                features.dedup();
                dependency.features = features.into_boxed_slice();
                dependency
            })
            .collect()
    }
}

/// One value of a feature, as Cargo reads it.
enum Value<'a> {
    /// `name`: another feature, or an optional dependency's own.
    Feature(&'a str),
    /// `dep:key`: an optional dependency, and no feature.
    Dependency(&'a str),
    /// `key/feature`, which enables the dependency `key` too, or
    /// `key?/feature`, which does not (`weak`).
    Forwarded {
        key: &'a str,
        feature: &'a str,
        weak: bool,
    },
}

impl<'a> Value<'a> {
    fn of(value: &'a str) -> Self {
        if let Some(key) = value.strip_prefix("dep:") {
            return Self::Dependency(key);
        }
        let Some((key, feature)) = value.split_once('/') else {
            return Self::Feature(value);
        };
        match key.strip_suffix('?') {
            Some(key) => Self::Forwarded {
                key,
                feature,
                weak: true,
            },
            None => Self::Forwarded {
                key,
                feature,
                weak: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Features;
    use crate::package::manifest::cargo::Cargo;
    use std::path::{Path, PathBuf};

    fn cargo(text: &str) -> Cargo {
        Cargo::parse(
            PathBuf::from("/app/Cargo.toml"),
            text.to_owned(),
            Path::new("/app"),
            false,
        )
        .unwrap()
        .unwrap()
    }

    /// The default features reach other features and optional
    /// dependencies, by `dep:`, by a dependency's own feature and by
    /// `key/feature`; `key?/feature` adds a feature to a dependency that is
    /// on and enables none.
    #[test]
    fn the_default_features_enable_what_cargo_enables() {
        let cargo = cargo(
            r#"
[package]
name = "app"
version = "0.1.0"

[dependencies]
plain = "1"
by-dep = { version = "1", optional = true }
by-name = { version = "1", optional = true }
by-feature = { version = "1", optional = true }
weak = { version = "1", optional = true }
off = { version = "1", optional = true }

[features]
default = ["a", "by-name"]
a = ["b", "dep:by-dep", "plain/extra"]
b = ["by-feature/fast", "weak?/x"]
c = ["dep:off"]
"#,
        );
        let enabled = cargo.enabled_dependencies();
        let found = enabled
            .iter()
            .map(|dependency| (dependency.key.as_str(), dependency.features.join(",")))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                ("by-dep", String::new()),
                ("by-feature", "fast".to_owned()),
                ("by-name", String::new()),
                ("plain", "extra".to_owned()),
            ]
        );
        assert_eq!(Features::default().enabled(&cargo.dependencies).len(), 1);
    }
}
