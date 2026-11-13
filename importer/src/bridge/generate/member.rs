//! The runner's library crates (#1670): the base every adapter builds on and
//! one crate of adapters per imported root. Cargo compiles them on several
//! cores, and an edit that changes one root's adapters recompiles that crate
//! and the runner alone. What names the program's crates is the runner's own
//! (#1793), so no member depends on the program.
//!
//! Each is a member of the runner's workspace with the runner's own
//! dependencies but the program's crates: its manifest and its lock entry
//! take them from the runner's, so every step that rewrites the runner's
//! manifest or lock (the mirror, the shared runtime, a member's version)
//! reaches them too.

use std::collections::{BTreeMap, BTreeSet};

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
    pub(super) fn package(&self) -> String {
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

    /// Its manifest, with the runner's dependencies from `runner`'s but
    /// the packages of `without`. The base has a build script, which the
    /// shared runtime's link search reaches every member through (D614).
    pub(super) fn manifest(
        &self,
        runner: &DocumentMut,
        without: &BTreeSet<String>,
    ) -> Result<Vec<u8>, request::Error> {
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
        // The packages it leaves out, which `manifest` reads again after
        // each rewrite of the runner's.
        if !without.is_empty() {
            let mut rawr = Table::new();
            rawr.insert(
                "without",
                toml_edit::value(without.iter().collect::<toml_edit::Array>()),
            );
            let mut metadata = Table::new();
            metadata.set_implicit(true);
            metadata.insert("rawr", Item::Table(rawr));
            package.insert("metadata", Item::Table(metadata));
        }
        document.insert("package", Item::Table(package));
        let mut library = Table::new();
        library.insert("name", toml_edit::value(name));
        library.insert("path", toml_edit::value("src/lib.rs"));
        document.insert("lib", Item::Table(library));
        document.insert("dependencies", Item::Table(dependencies));
        // Framed as a member's manifest is read back, so it matches what
        // `rebase` makes of it again after each rewrite of the runner's.
        let mut document = document
            .to_string()
            .parse::<DocumentMut>()
            .map_err(request::misread)?;
        rebase(runner, &mut document)
            .map(|()| document.to_string().into_bytes())
            .ok_or_else(|| request::misread("the runner's manifest has no version"))
    }
}

/// `{ package = .., path = .. }`.
fn path_dependency(package: &str, path: &str) -> toml_edit::Value {
    let mut table = toml_edit::InlineTable::new();
    table.insert("package", package.into());
    table.insert("path", path.into());
    toml_edit::Value::InlineTable(table)
}

/// The runner's `manifest` with a dependency on each of `members`, its
/// binary named `binary` when it takes a name of its own, and the runner and
/// its members compiled without debug info (#1789).
///
/// The binary takes the script crate's name when it declares the proxies of
/// script types (D594), so their `type_name` is the script's: Cargo names a
/// binary's crate by its target.
pub(super) fn runner_manifest(
    manifest: &[u8],
    members: &[Member],
    binary: Option<&str>,
) -> Result<Vec<u8>, request::Error> {
    let text = std::str::from_utf8(manifest).map_err(request::misread)?;
    let mut document = text.parse::<DocumentMut>().map_err(request::misread)?;
    if let Some(binary) = binary {
        let package = document
            .get_mut("package")
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| request::misread("the runner's manifest has no package"))?;
        package.insert("autobins", toml_edit::value(false));
        let mut target = Table::new();
        target.insert("name", toml_edit::value(binary));
        target.insert("path", toml_edit::value(super::SOURCE_PATH));
        let mut targets = toml_edit::ArrayOfTables::new();
        targets.push(target);
        document.insert("bin", Item::ArrayOfTables(targets));
    }
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
    without_debug_info(
        &mut document,
        std::iter::once(RUNNER.to_owned()).chain(members.iter().map(Member::package)),
    )?;
    Ok(super::normalize(document.to_string().as_bytes()))
}

/// `[profile.dev.package.<package>]` with `debug = 0` for each of `packages`,
/// the runner's own, which `cargo build` builds at `dev`.
///
/// Their debug info names the line of each item of the program's crate
/// they use, so a save that moves the lines of one file recompiles every
/// codegen unit that names an item below the change: 47 of the program
/// member's 219 and 37 of the runner's 240 for one field on islands-theme
/// with twelve more screens, and 5 and 3 without it. Nobody steps through
/// generated code. The units the project's `cargo build` shares keep the
/// project's profile. Without debug info, Cargo strips it with
/// `rust-objcopy`, which some toolchains ship broken, so nothing is
/// stripped (`cargo::internal`).
fn without_debug_info(
    document: &mut DocumentMut,
    packages: impl Iterator<Item = String>,
) -> Result<(), request::Error> {
    let mut table = document.as_table_mut();
    for key in ["profile", "dev", "package"] {
        let entry = table.entry(key).or_insert_with(|| {
            let mut implicit = Table::new();
            implicit.set_implicit(true);
            Item::Table(implicit)
        });
        table = entry
            .as_table_mut()
            .ok_or_else(|| request::misread("the runner's profile is not a table"))?;
    }
    for package in packages {
        let mut profile = Table::new();
        profile.insert("debug", toml_edit::value(0));
        profile.insert("strip", toml_edit::value(false));
        table.insert(&package, Item::Table(profile));
    }
    Ok(())
}

/// The runner's `lock` with an entry for each of `members`, whose
/// dependencies [`lock`] fills in, but the packages each leaves out
/// (`without`, by its package).
pub(super) fn runner_lock(
    lock: &[u8],
    members: &[Member],
    without: &BTreeMap<String, BTreeSet<String>>,
) -> Result<Vec<u8>, request::Error> {
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
    fill_lock(&mut document, without)
        .map(|()| super::normalize(document.to_string().as_bytes()))
        .ok_or_else(|| request::misread("the runner's lock has no version"))
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

/// The file name of the runner's executable in the runner's `manifest`:
/// its binary's own name (`runner_manifest`), else its package's.
pub(crate) fn executable(manifest: &str) -> String {
    let named = manifest.parse::<DocumentMut>().ok().and_then(|document| {
        let target = document.get("bin")?.as_array_of_tables()?.iter().next()?;
        target.get("name")?.as_str().map(str::to_owned)
    });
    let name = named.unwrap_or_else(|| RUNNER.to_owned());
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
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

/// The packages each member's manifest of `manifests` leaves out, by the
/// member's package (`Member::manifest`).
pub(crate) fn exclusions<'a>(
    manifests: impl Iterator<Item = &'a DocumentMut>,
) -> BTreeMap<String, BTreeSet<String>> {
    manifests
        .filter_map(|document| {
            let name = document.get("package")?.get("name")?.as_str()?.to_owned();
            Some((name, without(document)))
        })
        .filter(|(_, without)| !without.is_empty())
        .collect()
}

/// The packages a member's manifest leaves out.
fn without(document: &DocumentMut) -> BTreeSet<String> {
    document
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("rawr"))
        .and_then(|rawr| rawr.get("without"))
        .and_then(Item::as_array)
        .map(|packages| {
            packages
                .iter()
                .filter_map(|package| package.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// [`rebase`] on the manifests' texts. `None` when either does not parse.
#[cfg(test)]
fn manifest(runner: &str, member: &str) -> Option<String> {
    let runner = runner.parse::<DocumentMut>().ok()?;
    let mut document = member.parse::<DocumentMut>().ok()?;
    rebase(&runner, &mut document)?;
    Some(document.to_string())
}

/// `document`, a member's manifest, with the dependencies of `runner`, the
/// runner's: every one but the members and the packages the member leaves
/// out, whose relative paths it names from its own directory, and the
/// runner's version. `None` when the runner has no version or the member
/// no package, and then `document` may be partly rewritten.
pub(crate) fn rebase(runner: &DocumentMut, document: &mut DocumentMut) -> Option<()> {
    let without = without(document);
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
        document.insert(kind, Item::Table(rebased(table, &without)));
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
                    platform_tables.insert(kind, Item::Table(rebased(table, &without)));
                }
            }
            copied.insert(platform, Item::Table(platform_tables));
        }
        document.insert("target", Item::Table(copied));
    }
    Some(())
}

/// Whether a dependency names one of the runner's members.
fn names_member(item: &Item) -> bool {
    item.as_table_like()
        .and_then(|table| table.get("package"))
        .and_then(Item::as_str)
        .is_some_and(|package| package.starts_with(RUNNER))
}

/// The runner's dependency `table` without its members and the packages of
/// `without`, with each relative path named from a member's directory, two
/// below the runner's root.
fn rebased(table: &dyn TableLike, without: &BTreeSet<String>) -> Table {
    let mut copied = Table::new();
    for (key, item) in table.iter() {
        let package = item
            .as_table_like()
            .and_then(|item| item.get("package"))
            .and_then(Item::as_str)
            .unwrap_or(key);
        if names_member(item) || without.contains(package) {
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

/// [`fill_lock`] on the lock's text. `None` when it does not parse.
#[cfg(test)]
fn lock(lock: &str, without: &BTreeMap<String, BTreeSet<String>>) -> Option<String> {
    let mut document = lock.parse::<DocumentMut>().ok()?;
    fill_lock(&mut document, without)?;
    Some(document.to_string())
}

/// The lock `document` with each member's entry at the runner's version and
/// depending on what the runner's does but the packages it leaves out
/// (`without`, by its package), besides the members it depends on itself:
/// the resolve Cargo makes of the members' manifests, so a `--locked` build
/// holds. `None`, with `document` as it was, when it has no runner or the
/// runner no version.
pub(crate) fn fill_lock(
    document: &mut DocumentMut,
    without: &BTreeMap<String, BTreeSet<String>>,
) -> Option<()> {
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
        let left = package
            .get("name")
            .and_then(Item::as_str)
            .and_then(|name| without.get(name))
            .cloned()
            .unwrap_or_default();
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
        dependencies.extend(
            shared
                .iter()
                .filter(|dependency| named(dependency).is_none_or(|name| !left.contains(&name)))
                .cloned(),
        );
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
    Some(())
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
            .manifest(&RUNNER_MANIFEST.parse().unwrap(), &BTreeSet::new())
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
        let runner = runner_manifest(RUNNER_MANIFEST.as_bytes(), &[member], None)
            .expect("the runner's manifest renders");
        let runner = std::str::from_utf8(&runner).expect("the manifest is UTF-8");
        assert!(runner.contains(
            "rawr_generated_runner_adapters_iced = { package = \"rawr-generated-runner-adapters-iced\", path = \"rawr-crates/adapters-iced\" }"
        ));
        let again = super::manifest(runner, text).expect("both manifests parse");
        assert_eq!(again.matches("adapters-iced").count(), 1, "{again}");
    }

    /// The runner and each member compile without debug info, and nothing
    /// is stripped; the project's own settings stay as written (#1789).
    #[test]
    fn the_runner_and_its_members_compile_without_debug_info() {
        let project = format!("{RUNNER_MANIFEST}\n[profile.dev.package.\"*\"]\ndebug = 2\n");
        let members = [
            Member::Base {
                library: "app".to_owned(),
            },
            Member::Adapters {
                root: "iced".to_owned(),
            },
        ];
        for manifest in [RUNNER_MANIFEST, project.as_str()] {
            let runner = runner_manifest(manifest.as_bytes(), &members, None)
                .expect("the runner's manifest renders");
            let runner = std::str::from_utf8(&runner).expect("the manifest is UTF-8");
            let document = runner.parse::<DocumentMut>().expect("the manifest parses");
            let packages = &document["profile"]["dev"]["package"];
            let adapters = members[1].package();
            for package in [RUNNER, BASE_PACKAGE, adapters.as_str()] {
                assert_eq!(packages[package]["debug"].as_integer(), Some(0), "{runner}");
                assert_eq!(
                    packages[package]["strip"].as_bool(),
                    Some(false),
                    "{runner}"
                );
            }
            assert_eq!(
                packages.get("*").map(|all| all["debug"].as_integer()),
                (manifest == project).then_some(Some(2)),
                "{runner}"
            );
            assert!(document["profile"]["dev"].get("debug").is_none());
        }
    }

    /// A runner whose binary declares the proxies names its crate as the
    /// script's (D594), and its executable takes that name; one without
    /// keeps the package's (#1793).
    #[test]
    fn a_runner_binary_of_its_own_name_names_its_executable() {
        let named = runner_manifest(RUNNER_MANIFEST.as_bytes(), &[], Some("app"))
            .expect("the runner's manifest renders");
        let named = std::str::from_utf8(&named).expect("the manifest is UTF-8");
        let document = named.parse::<DocumentMut>().expect("the manifest parses");
        let targets = document["bin"]
            .as_array_of_tables()
            .expect("the binary is a target of its own");
        assert_eq!(targets.len(), 1, "{named}");
        let target = targets.iter().next().expect("one target");
        assert_eq!(target["name"].as_str(), Some("app"));
        assert_eq!(target["path"].as_str(), Some("src/main.rs"));
        assert_eq!(document["package"]["autobins"].as_bool(), Some(false));
        let suffix = std::env::consts::EXE_SUFFIX;
        assert_eq!(executable(named), format!("app{suffix}"));
        let plain = runner_manifest(RUNNER_MANIFEST.as_bytes(), &[], None)
            .expect("the runner's manifest renders");
        let plain = std::str::from_utf8(&plain).expect("the manifest is UTF-8");
        assert!(!plain.contains("[[bin]]"), "{plain}");
        assert_eq!(executable(plain), format!("{RUNNER}{suffix}"));
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
        let locked =
            runner_lock(lock.as_bytes(), &members, &BTreeMap::new()).expect("the lock renders");
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
        let relocked = super::lock(&versioned, &BTreeMap::new()).expect("the lock parses");
        assert_eq!(
            relocked.matches("version = \"0.0.0+ab\"").count(),
            3,
            "{relocked}"
        );
    }
}
