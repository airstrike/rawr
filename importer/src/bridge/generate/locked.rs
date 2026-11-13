//! The harness's lockfile, read once per rendering into the packages each
//! question the generator asks of it reads, so no question parses it again.

use std::collections::BTreeMap;

use toml_edit::{DocumentMut, Item};

use super::request;

/// The locked packages and the patches the lock lists as unused.
pub(super) struct Locked {
    packages: Vec<Package>,
    /// Each package's index by its name with every `-` as `_`, as a
    /// macro's `$crate` segment spells it.
    by_crate: BTreeMap<String, Vec<usize>>,
    /// The packages of `[[patch.unused]]`.
    unused: Vec<String>,
}

/// One `[[package]]` of the lock.
pub(super) struct Package {
    name: String,
    version: String,
    /// None for a path package.
    source: Option<String>,
    /// Its dependencies as the lock writes them: `name`, `name version` or
    /// `name version (source)`.
    dependencies: Vec<String>,
}

impl Locked {
    /// The packages of `lock`, a Cargo lockfile.
    pub(super) fn read(lock: &[u8]) -> Result<Self, request::Error> {
        let document = std::str::from_utf8(lock)
            .map_err(request::misread)?
            .parse::<DocumentMut>()
            .map_err(request::misread)?;
        let text = |table: &toml_edit::Table, key: &str| {
            table.get(key).and_then(Item::as_str).map(str::to_owned)
        };
        let packages: Vec<Package> = document
            .get("package")
            .and_then(Item::as_array_of_tables)
            .into_iter()
            .flatten()
            .map(|package| Package {
                name: text(package, "name").unwrap_or_default(),
                version: text(package, "version").unwrap_or_default(),
                source: text(package, "source"),
                dependencies: package
                    .get("dependencies")
                    .and_then(Item::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|dependency| dependency.as_str().map(str::to_owned))
                    .collect(),
            })
            .collect();
        let mut by_crate = BTreeMap::<String, Vec<usize>>::new();
        for (index, package) in packages.iter().enumerate() {
            by_crate
                .entry(package.name.replace('-', "_"))
                .or_default()
                .push(index);
        }
        let unused = document
            .get("patch")
            .and_then(|patch| patch.get("unused"))
            .and_then(Item::as_array_of_tables)
            .into_iter()
            .flatten()
            .filter_map(|entry| text(entry, "name"))
            .collect();
        Ok(Self {
            packages,
            by_crate,
            unused,
        })
    }

    pub(super) fn packages(&self) -> &[Package] {
        &self.packages
    }

    /// The packages whose name a crate spells `name`, `-` as `_`.
    pub(super) fn of_crate(&self, name: &str) -> impl Iterator<Item = &Package> {
        self.by_crate
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.packages[*index])
    }

    /// Whether the lock holds exactly one package named `name`.
    pub(super) fn single(&self, name: &str) -> bool {
        let spelled = name.replace('-', "_");
        let mut named = self
            .of_crate(&spelled)
            .filter(|package| package.name == name);
        named.next().is_some() && named.next().is_none()
    }

    /// The packages `[[patch.unused]]` names.
    pub(super) fn unused_patches(&self) -> impl Iterator<Item = &str> {
        self.unused.iter().map(String::as_str)
    }
}

impl Package {
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn version(&self) -> &str {
        &self.version
    }

    pub(super) fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub(super) fn dependencies(&self) -> impl Iterator<Item = &str> {
        self.dependencies.iter().map(String::as_str)
    }
}
