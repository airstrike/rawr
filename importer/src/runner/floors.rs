//! Rawr's version floors in the lock of a runner that compiles rawr in the
//! project.
//!
//! Such a runner's lock starts from the project's, and cargo adds rawr's
//! graph to it. Cargo keeps every locked package whose source no changed
//! dependency poisons, and a dependency only poisons its source when a
//! workspace member names it, or a path package names it without
//! `optional`. Rawr reaches much of its graph through optional path
//! dependencies, so a registry package the project locks below rawr's floor
//! stays locked, and the resolve fails ("failed to select a version").
//! Raising that package to the version rawr's own lock names, within the
//! same semver-compatible range, keeps the requirements the project's
//! version met (a caret requirement accepts every later compatible
//! version) and meets rawr's. No package leaves the lock, so no dependency
//! of a workspace member goes unmatched, nothing poisons a source, and
//! cargo keeps every other version and every git revision the project pins.

use toml_edit::{DocumentMut, Item, Table};

/// The project's lock `document` with each registry package raised to the
/// version rawr's own lock (`host`) names for the same name, source and
/// semver-compatible range, when that version is later. The raised entry
/// takes rawr's version, checksum and dependencies, and every dependency
/// that named the old version names the new one. `None`, with `document`
/// as it was, when no package is below rawr's floor.
pub(crate) fn raise(document: &mut DocumentMut, host: &DocumentMut) -> Option<()> {
    let floors = host
        .get("package")?
        .as_array_of_tables()?
        .iter()
        .filter_map(|table| Some((Locked::of(table)?, table)))
        .collect::<Vec<_>>();
    let packages = document.get_mut("package")?.as_array_of_tables_mut()?;
    let mut raised = Vec::new();
    for table in packages.iter_mut() {
        let Some(locked) = Locked::of(table) else {
            continue;
        };
        let floor = floors.iter().find(|(floor, _)| {
            floor.name == locked.name
                && floor.source == locked.source
                && floor.compatible(&locked)
                && floor.version > locked.version
        });
        if let Some((floor, entry)) = floor {
            raised.push(Raised {
                name: locked.name.to_owned(),
                old: locked.text.to_owned(),
                new: floor.text.to_owned(),
            });
            for key in ["version", "checksum", "dependencies"] {
                match entry.get(key) {
                    Some(item) => {
                        table.insert(key, item.clone());
                    }
                    None => {
                        table.remove(key);
                    }
                }
            }
        }
    }
    if raised.is_empty() {
        return None;
    }
    // A dependency is `name`, `name version` or `name version (source)`.
    for table in packages.iter_mut() {
        let Some(dependencies) = table.get_mut("dependencies").and_then(Item::as_array_mut) else {
            continue;
        };
        for dependency in dependencies.iter_mut() {
            let Some(text) = dependency.as_str() else {
                continue;
            };
            let mut parts = text.splitn(3, ' ');
            let (Some(name), Some(version)) = (parts.next(), parts.next()) else {
                continue;
            };
            let Some(raised) = raised
                .iter()
                .find(|raised| raised.name == name && raised.old == version)
            else {
                continue;
            };
            let rest = parts
                .next()
                .map_or(String::new(), |source| format!(" {source}"));
            let decor = dependency.decor().clone();
            *dependency = format!("{name} {}{rest}", raised.new).into();
            *dependency.decor_mut() = decor;
        }
    }
    Some(())
}

/// A package [`raise`] moved from the `old` version to the `new` one.
struct Raised {
    name: String,
    old: String,
    new: String,
}

/// A registry package as a lock names it.
struct Locked<'a> {
    name: &'a str,
    source: &'a str,
    /// The version as the lock writes it.
    text: &'a str,
    version: semver::Version,
}

impl<'a> Locked<'a> {
    /// The registry package `table` locks. `None` for a path or git package.
    fn of(table: &'a Table) -> Option<Self> {
        let text = |key: &str| table.get(key).and_then(Item::as_str);
        let source = text("source").filter(|source| source.starts_with("registry+"))?;
        let version = text("version")?;
        Some(Self {
            name: text("name")?,
            source,
            text: version,
            version: semver::Version::parse(version).ok()?,
        })
    }

    /// Whether `other` is in this version's semver-compatible range, in
    /// which cargo locks at most one version of a package per source.
    fn compatible(&self, other: &Self) -> bool {
        let (ours, theirs) = (&self.version, &other.version);
        match (ours.major, ours.minor) {
            (0, 0) => theirs.major == 0 && theirs.minor == 0 && theirs.patch == ours.patch,
            (0, minor) => theirs.major == 0 && theirs.minor == minor,
            (major, _) => theirs.major == major,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_package_below_rawr_s_floor_takes_rawr_s_version_and_keeps_the_rest() {
        let registry = "registry+https://github.com/rust-lang/crates.io-index";
        let package = |name: &str, version: &str, source: &str, dependencies: &str| {
            format!(
                "[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = \"{source}\"\n\
                 dependencies = [{dependencies}]\n\n"
            )
        };
        let project = [
            package("parser", "1.1.5", registry, "\"winnow 1.0.0\""),
            package("winnow", "0.7.15", registry, ""),
            package("winnow", "1.0.0", registry, ""),
            package("serde", "1.0.228", registry, ""),
            package(
                "widget",
                "0.1.0",
                "git+https://example.com/widget#abc",
                "\"winnow 1.0.0\"",
            ),
        ]
        .concat();
        let host = [
            package("winnow", "1.0.4", registry, "\"memchr\""),
            package("winnow", "0.7.14", registry, ""),
            package("serde", "1.0.200", registry, ""),
            package("widget", "0.1.0", "git+https://example.com/widget#def", ""),
        ]
        .concat();
        let mut document = project.parse::<toml_edit::DocumentMut>().unwrap();
        let host = host.parse::<toml_edit::DocumentMut>().unwrap();
        assert!(super::raise(&mut document, &host).is_some());
        let expected = [
            package("parser", "1.1.5", registry, "\"winnow 1.0.4\""),
            // An earlier version of rawr's, or one in another range, stays.
            package("winnow", "0.7.15", registry, ""),
            package("winnow", "1.0.4", registry, "\"memchr\""),
            package("serde", "1.0.228", registry, ""),
            // A git package keeps the revision the project pins.
            package(
                "widget",
                "0.1.0",
                "git+https://example.com/widget#abc",
                "\"winnow 1.0.4\"",
            ),
        ]
        .concat();
        assert_eq!(document.to_string(), expected);
        let mut unchanged = expected.parse::<toml_edit::DocumentMut>().unwrap();
        assert!(super::raise(&mut unchanged, &host).is_none());
    }
}
