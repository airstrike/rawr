//! The runner's library crates (#1670): the base every adapter builds on and
//! one crate of adapters per imported root. Cargo compiles them on several
//! cores, and an edit that changes one root's adapters recompiles that
//! crate and the runner alone.
//!
//! Each is a member of the runner's workspace with the runner's own
//! dependencies: its manifest and its lock entry take them from the
//! runner's, so every step that rewrites the runner's manifest or lock
//! (the mirror, the shared runtime, a member's version) reaches them too.

use toml_edit::{DocumentMut, Item, Table, TableLike};

use super::request;

/// The directory under the runner's root that holds them, which no
/// directory of a project's takes (`runner::mirror` places those).
pub(crate) const DIRECTORY: &str = "rawr-crates";

/// The runner's package. Each member's package begins with it.
pub(crate) const RUNNER: &str = "rawr-generated-runner";

/// The base's package.
const BASE_PACKAGE: &str = "rawr-generated-runner-base";

/// The name the runner's code gives the base, as the crate roots and the
/// adapters' modules spell it.
pub(super) const BASE_KEY: &str = "rawr_generated_runner_base";

/// The dependency tables a member takes from the runner's manifest.
const DEPENDENCIES: [&str; 3] = ["dependencies", "build-dependencies", "dev-dependencies"];

/// One of the runner's library crates.
pub(super) enum Member {
    /// The runtime, families, places and proxies every adapter names, and
    /// the script's impls. Its library takes the script crate's name when
    /// proxies stand for script types (D594), so their `type_name` is theirs.
    Base { library: String },
    /// The adapters of one imported root, by its name as a module segment.
    Adapters { root: String },
}

impl Member {
    fn package(&self) -> String {
        match self {
            Self::Base { .. } => BASE_PACKAGE.to_owned(),
            Self::Adapters { root } => format!("{RUNNER}-adapters-{root}"),
        }
    }

    /// The name the runner's code gives the crate.
    pub(super) fn key(&self) -> String {
        self.package().replace('-', "_")
    }

    /// Its directory under the runner's root.
    pub(super) fn directory(&self) -> String {
        match self {
            Self::Base { .. } => format!("{DIRECTORY}/base"),
            Self::Adapters { root } => format!("{DIRECTORY}/adapters-{root}"),
        }
    }

    /// Its manifest, with the runner's dependencies from `runner`'s. The
    /// base has a build script, which the shared runtime's link search
    /// reaches every member through (D614).
    pub(super) fn manifest(&self, runner: &[u8]) -> Result<Vec<u8>, request::Error> {
        let mut document = DocumentMut::new();
        let mut package = Table::new();
        package.insert("name", toml_edit::value(self.package()));
        package.insert("version", toml_edit::value("0.0.0"));
        package.insert("edition", toml_edit::value("2024"));
        package.insert("publish", toml_edit::value(false));
        let mut dependencies = Table::new();
        let name = match self {
            Self::Base { library } => {
                package.insert("build", toml_edit::value("build.rs"));
                library.clone()
            }
            Self::Adapters { .. } => {
                dependencies.insert(
                    BASE_KEY,
                    Item::Value(path_dependency(BASE_PACKAGE, "../base")),
                );
                self.key()
            }
        };
        document.insert("package", Item::Table(package));
        let mut library = Table::new();
        library.insert("name", toml_edit::value(name));
        library.insert("path", toml_edit::value("src/lib.rs"));
        document.insert("lib", Item::Table(library));
        document.insert("dependencies", Item::Table(dependencies));
        let runner = std::str::from_utf8(runner).map_err(request::misread)?;
        manifest(runner, &document.to_string())
            .map(String::into_bytes)
            .ok_or_else(|| request::misread("the runner's manifest does not parse"))
    }
}

/// `{ package = .., path = .. }`.
fn path_dependency(package: &str, path: &str) -> toml_edit::Value {
    let mut table = toml_edit::InlineTable::new();
    table.insert("package", package.into());
    table.insert("path", path.into());
    toml_edit::Value::InlineTable(table)
}

/// The runner's `manifest` with a dependency on each of `members`.
pub(super) fn runner_manifest(
    manifest: &[u8],
    members: &[Member],
) -> Result<Vec<u8>, request::Error> {
    let text = std::str::from_utf8(manifest).map_err(request::misread)?;
    let mut document = text.parse::<DocumentMut>().map_err(request::misread)?;
    let dependencies = document
        .entry("dependencies")
        .or_insert(Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or_else(|| request::misread("the runner's dependencies are not a table"))?;
    for member in members {
        dependencies.insert(
            &member.key(),
            Item::Value(path_dependency(&member.package(), &member.directory())),
        );
    }
    Ok(super::normalize(document.to_string().as_bytes()))
}

/// The runner's `lock` with an entry for each of `members`, whose
/// dependencies [`lock`] fills in.
pub(super) fn runner_lock(lock: &[u8], members: &[Member]) -> Result<Vec<u8>, request::Error> {
    let text = std::str::from_utf8(lock).map_err(request::misread)?;
    let mut document = text.parse::<DocumentMut>().map_err(request::misread)?;
    let packages = document
        .get_mut("package")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| request::misread("the runner's lock has no package array"))?;
    for member in members {
        let mut entry = Table::new();
        entry.insert("name", toml_edit::value(member.package()));
        entry.insert("version", toml_edit::value("0.0.0"));
        let mut needs = toml_edit::Array::new();
        if let Member::Adapters { .. } = member {
            needs.push(BASE_PACKAGE);
        }
        entry.insert("dependencies", toml_edit::value(needs));
        packages.push(entry);
    }
    let runner = packages
        .iter_mut()
        .find(|package| is_runner(package))
        .ok_or_else(|| request::misread("the runner's lock has no runner"))?;
    let dependencies = runner
        .entry("dependencies")
        .or_insert(toml_edit::value(toml_edit::Array::new()))
        .as_array_mut()
        .ok_or_else(|| request::misread("the runner's lock dependencies are not a list"))?;
    for member in members {
        dependencies.push(member.package());
    }
    self::lock(&document.to_string())
        .map(|lock| super::normalize(lock.as_bytes()))
        .ok_or_else(|| request::misread("the runner's lock does not parse"))
}

/// Whether a lock entry is the runner's own package.
fn is_runner(package: &Table) -> bool {
    package.get("name").and_then(Item::as_str) == Some(RUNNER) && !package.contains_key("source")
}

/// Whether a lock entry is one of the runner's members.
fn is_member(package: &Table) -> bool {
    package
        .get("name")
        .and_then(Item::as_str)
        .is_some_and(|name| name.starts_with(RUNNER) && name != RUNNER)
        && !package.contains_key("source")
}

/// Whether `name` is the name of the runner's package or a member's,
/// as a lock entry names its package (no source).
pub(crate) fn is_runner_package(name: &str) -> bool {
    name.starts_with(RUNNER)
}

/// `path`, a file of the runner's, within its member's directory, when it
/// is a member's: `Cargo.toml` of `rawr-crates/base/Cargo.toml`.
pub(crate) fn file(path: &str) -> Option<&str> {
    let (_, file) = path
        .strip_prefix(DIRECTORY)?
        .strip_prefix('/')?
        .split_once('/')?;
    Some(file)
}

/// `member`, a member's manifest, with the dependencies of `runner`, the
/// runner's: every one but the members, whose relative paths it names from
/// its own directory, and the runner's version. `None` when either does not
/// parse.
pub(crate) fn manifest(runner: &str, member: &str) -> Option<String> {
    let runner = runner.parse::<DocumentMut>().ok()?;
    let mut document = member.parse::<DocumentMut>().ok()?;
    let version = runner.get("package")?.get("version")?.clone();
    document
        .get_mut("package")?
        .as_table_like_mut()?
        .insert("version", version);
    // Its own dependencies on members stay; the rest are the runner's.
    let own = document
        .get("dependencies")
        .and_then(Item::as_table_like)
        .map(|table| {
            table
                .iter()
                .filter(|(_, item)| names_member(item))
                .map(|(key, item)| (key.to_owned(), item.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for kind in DEPENDENCIES {
        document.remove(kind);
    }
    document.remove("target");
    for kind in DEPENDENCIES {
        let Some(table) = runner.get(kind).and_then(Item::as_table_like) else {
            continue;
        };
        document.insert(kind, Item::Table(rebased(table)));
    }
    let dependencies = document
        .entry("dependencies")
        .or_insert(Item::Table(Table::new()))
        .as_table_like_mut()?;
    for (key, item) in own {
        dependencies.insert(&key, item);
    }
    if let Some(targets) = runner.get("target").and_then(Item::as_table_like) {
        let mut copied = Table::new();
        copied.set_implicit(true);
        for (platform, tables) in targets.iter() {
            let Some(tables) = tables.as_table_like() else {
                continue;
            };
            let mut platform_tables = Table::new();
            platform_tables.set_implicit(true);
            for kind in DEPENDENCIES {
                if let Some(table) = tables.get(kind).and_then(Item::as_table_like) {
                    platform_tables.insert(kind, Item::Table(rebased(table)));
                }
            }
            copied.insert(platform, Item::Table(platform_tables));
        }
        document.insert("target", Item::Table(copied));
    }
    Some(document.to_string())
}

/// Whether a dependency names one of the runner's members.
fn names_member(item: &Item) -> bool {
    item.as_table_like()
        .and_then(|table| table.get("package"))
        .and_then(Item::as_str)
        .is_some_and(|package| package.starts_with(RUNNER))
}

/// The runner's dependency `table` without its members, with each relative
/// path named from a member's directory, two below the runner's root.
fn rebased(table: &dyn TableLike) -> Table {
    let mut copied = Table::new();
    for (key, item) in table.iter() {
        if names_member(item) {
            continue;
        }
        let mut item = item.clone();
        if let Some(dependency) = item.as_table_like_mut()
            && let Some(path) = dependency.get("path").and_then(Item::as_str)
            && std::path::Path::new(path).is_relative()
        {
            let path = format!("../../{path}");
            dependency.insert("path", toml_edit::value(path));
        }
        copied.insert(key, item);
    }
    copied
}

/// `lock` with each member's entry at the runner's version and depending on
/// what the runner's does, besides the members it depends on itself: the
/// resolve Cargo makes of the members' manifests, so a `--locked` build
/// holds. `None` when it does not parse.
pub(crate) fn lock(lock: &str) -> Option<String> {
    let mut document = lock.parse::<DocumentMut>().ok()?;
    let packages = document.get_mut("package")?.as_array_of_tables_mut()?;
    let runner = packages.iter().find(|package| is_runner(package))?;
    let version = runner.get("version")?.clone();
    let members = packages
        .iter()
        .filter(|package| is_member(package))
        .filter_map(|package| package.get("name").and_then(Item::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let named = |dependency: &toml_edit::Value| {
        dependency
            .as_str()
            .and_then(|text| text.split_whitespace().next())
            .map(str::to_owned)
    };
    let shared = runner
        .get("dependencies")
        .and_then(Item::as_array)
        .map(|dependencies| {
            dependencies
                .iter()
                .filter(|dependency| named(dependency).is_none_or(|name| !members.contains(&name)))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for package in packages.iter_mut().filter(|package| is_member(package)) {
        package.insert("version", version.clone());
        let mut dependencies = package
            .get("dependencies")
            .and_then(Item::as_array)
            .map(|dependencies| {
                dependencies
                    .iter()
                    .filter(|dependency| {
                        named(dependency).is_some_and(|name| members.contains(&name))
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        dependencies.extend(shared.iter().cloned());
        dependencies.sort_by_key(|dependency| named(dependency).unwrap_or_default());
        // Cargo writes no list for a package without dependencies.
        if dependencies.is_empty() {
            package.remove("dependencies");
            continue;
        }
        package.insert("dependencies", toml_edit::value(listed(dependencies)));
    }
    // The runner's own list in Cargo's order: by name, and as Cargo wrote
    // them among one name.
    if let Some(runner) = packages.iter_mut().find(|package| is_runner(package))
        && let Some(dependencies) = runner.get_mut("dependencies").and_then(Item::as_array_mut)
    {
        let mut sorted = dependencies.iter().cloned().collect::<Vec<_>>();
        sorted.sort_by_key(|dependency| named(dependency).unwrap_or_default());
        *dependencies = listed(sorted);
    }
    Some(document.to_string())
}

/// `dependencies` as Cargo writes a lock entry's: one to a line.
fn listed(dependencies: Vec<toml_edit::Value>) -> toml_edit::Array {
    let mut array = toml_edit::Array::new();
    for dependency in dependencies {
        array.push_formatted(dependency.decorated("\n ", ""));
    }
    array.set_trailing("\n");
    array.set_trailing_comma(true);
    array
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNER_MANIFEST: &str = "[workspace]\n\n[package]\nname = \"rawr-generated-runner\"\nversion = \"0.0.0+0123\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[dependencies]\nalpha = { path = \"app/alpha\", default-features = true }\nbeta = { path = \"/abs/beta\" }\niced = \"0.14\"\n\n[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n";

    /// A crate of adapters takes the runner's dependencies, its relative
    /// paths from two directories down, the runner's version, and the
    /// base by its own dependency.
    #[test]
    fn a_member_takes_the_runners_dependencies_and_version() {
        let member = Member::Adapters {
            root: "iced".to_owned(),
        };
        let manifest = member
            .manifest(RUNNER_MANIFEST.as_bytes())
            .expect("the member manifest renders");
        let text = std::str::from_utf8(&manifest).expect("the manifest is UTF-8");
        let document = text.parse::<DocumentMut>().expect("the manifest parses");
        assert_eq!(
            document["package"]["name"].as_str(),
            Some("rawr-generated-runner-adapters-iced")
        );
        assert_eq!(document["package"]["version"].as_str(), Some("0.0.0+0123"));
        assert_eq!(
            document["lib"]["name"].as_str(),
            Some("rawr_generated_runner_adapters_iced")
        );
        let dependencies = &document["dependencies"];
        assert_eq!(
            dependencies["alpha"]["path"].as_str(),
            Some("../../app/alpha")
        );
        assert_eq!(dependencies["beta"]["path"].as_str(), Some("/abs/beta"));
        assert_eq!(dependencies["iced"].as_str(), Some("0.14"));
        assert_eq!(dependencies[BASE_KEY]["path"].as_str(), Some("../base"));
        assert_eq!(
            document["target"]["cfg(unix)"]["dependencies"]["libc"].as_str(),
            Some("0.2")
        );
        // The runner's dependencies on its members never reach a member.
        let runner = runner_manifest(RUNNER_MANIFEST.as_bytes(), &[member])
            .expect("the runner's manifest renders");
        let runner = std::str::from_utf8(&runner).expect("the manifest is UTF-8");
        assert!(runner.contains(
            "rawr_generated_runner_adapters_iced = { package = \"rawr-generated-runner-adapters-iced\", path = \"rawr-crates/adapters-iced\" }"
        ));
        let again = super::manifest(runner, text).expect("both manifests parse");
        assert_eq!(again.matches("adapters-iced").count(), 1, "{again}");
    }

    /// Each member's lock entry depends on what the runner's does, as Cargo
    /// writes a list.
    #[test]
    fn a_members_lock_entry_takes_the_runners_dependencies() {
        let lock = "version = 4\n\n[[package]]\nname = \"iced\"\nversion = \"0.14.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n[[package]]\nname = \"rawr-generated-runner\"\nversion = \"0.0.0\"\ndependencies = [\n \"iced\",\n \"zeta\",\n]\n\n[[package]]\nname = \"zeta\"\nversion = \"1.0.0\"\n";
        let members = [
            Member::Base {
                library: "app".to_owned(),
            },
            Member::Adapters {
                root: "iced".to_owned(),
            },
        ];
        let locked = runner_lock(lock.as_bytes(), &members).expect("the lock renders");
        let locked = std::str::from_utf8(&locked).expect("the lock is UTF-8");
        assert_eq!(
            locked,
            "version = 4\n\n[[package]]\nname = \"iced\"\nversion = \"0.14.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n\
             [[package]]\nname = \"rawr-generated-runner\"\nversion = \"0.0.0\"\ndependencies = [\n \"iced\",\n \"rawr-generated-runner-adapters-iced\",\n \"rawr-generated-runner-base\",\n \"zeta\",\n]\n\n\
             [[package]]\nname = \"zeta\"\nversion = \"1.0.0\"\n\n\
             [[package]]\nname = \"rawr-generated-runner-base\"\nversion = \"0.0.0\"\ndependencies = [\n \"iced\",\n \"zeta\",\n]\n\n\
             [[package]]\nname = \"rawr-generated-runner-adapters-iced\"\nversion = \"0.0.0\"\ndependencies = [\n \"iced\",\n \"rawr-generated-runner-base\",\n \"zeta\",\n]\n"
        );
        // A runner at a version of its own gives it to its members.
        let versioned = locked.replace(
            "name = \"rawr-generated-runner\"\nversion = \"0.0.0\"",
            "name = \"rawr-generated-runner\"\nversion = \"0.0.0+ab\"",
        );
        let relocked = super::lock(&versioned).expect("the lock parses");
        assert_eq!(
            relocked.matches("version = \"0.0.0+ab\"").count(),
            3,
            "{relocked}"
        );
    }
}
