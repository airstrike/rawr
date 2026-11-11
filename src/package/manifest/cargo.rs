//! The crate's own Cargo package manifest.
//!
//! A Rawr program is one ordinary Cargo crate. When its `Cargo.toml` has a
//! `[package]`, its `[dependencies]` are the program's Rust dependencies,
//! and its native modules are importable as that crate.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use toml_edit::{Document, Item};

use super::features::Features;
use super::{byte_span, item_span, rust, Error};
use crate::project::cargo::features::{Enabled, Operator};
use crate::{Position, Span};

/// A validated Cargo package manifest at a Rawr project root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Cargo {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
    /// `[package] name`.
    pub(crate) package: String,
    /// The library crate name Rust code and Rawr name it by.
    pub(crate) crate_name: String,
    pub(crate) version: Option<String>,
    pub(crate) edition: Option<String>,
    /// `[dependencies]`, normalized to Rawr's Rust dependency form,
    /// optional ones included.
    pub(crate) dependencies: Box<[rust::Dependency]>,
    /// `[build-dependencies]`, in the same form.
    pub(crate) build_dependencies: Box<[rust::Dependency]>,
    /// `[dev-dependencies]`, in the same form, which [`Cargo::on_host`]
    /// joins to `dependencies` for a test build.
    pub(crate) dev_dependencies: Box<[rust::Dependency]>,
    /// Each `[target.'<platform>']` table's dependencies, as written, until
    /// [`Cargo::on_host`] joins those that hold on the host to the lists
    /// above.
    pub(crate) targets: Box<[Targeted]>,
    /// `[features]`.
    pub(crate) features: Features,
    /// The package's build script, absolute, as Cargo finds it: `build`'s
    /// path, else `build.rs` beside the manifest, unless `build = false`.
    pub(crate) build: Option<PathBuf>,
    /// `[package] links` as TOML text, which a generated package standing
    /// in for this one repeats.
    pub(crate) links: Option<String>,
    /// Where Cargo looks for the package's library root, absolute: `[lib]
    /// path`, else `src/lib.rs` beside the manifest. A script has none.
    pub(crate) library: Option<PathBuf>,
}

/// One `[target.'<platform>']` table's dependencies. Cargo decides on
/// which hosts the platform holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Targeted {
    /// The table's key: a `cfg(..)` or a target triple.
    pub(crate) platform: Box<str>,
    pub(crate) dependencies: Box<[rust::Dependency]>,
    pub(crate) build_dependencies: Box<[rust::Dependency]>,
    pub(crate) dev_dependencies: Box<[rust::Dependency]>,
}

/// Which `[target.'<platform>']` dependencies hold on the host.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Host<'a> {
    /// Those Cargo's unit graph of the target being built links.
    Built(&'a crate::project::cargo::Unit),
    /// Every table's, when Cargo was not asked, for a build in this mode:
    /// a runner's snapshot, whose parent resolved the program against
    /// Cargo's answer. The runner reads only the dependencies' names.
    Unasked(crate::project::cargo::Mode),
}

impl Cargo {
    /// Parse `Cargo.toml`: `None` when it is only a workspace (no `[package]`).
    ///
    /// With `ask_cargo`, Cargo resolves what the package inherits from its
    /// workspace. Without it (a runner's snapshot) an inherited dependency
    /// keeps only its alias.
    pub(crate) fn parse(
        path: PathBuf,
        text: String,
        root: &Path,
        ask_cargo: bool,
    ) -> Result<Option<Self>, Error> {
        let document = Document::parse(text.as_str()).map_err(|error| Error {
            message: error.to_string().into(),
            span: error
                .span()
                .map(|range| byte_span(&text, range))
                .unwrap_or_else(|| Span::new(Position::START, Position::START)),
        })?;
        let table = document.as_table();
        let Some(package) = table.get("package").and_then(|item| item.as_table_like()) else {
            return Ok(None);
        };
        let name = package
            .get("name")
            .and_then(|item| item.as_str())
            .ok_or_else(|| Error {
                message: "Cargo `[package]` requires a string `name`".into(),
                span: table.get("package").map_or_else(
                    || Span::new(Position::START, Position::START),
                    |item| item_span(&text, item),
                ),
            })?
            .to_owned();
        // `foo.workspace = true` inherits `foo` from the workspace root.
        let inherits = |item: &Item| {
            item.as_table_like()
                .and_then(|table| table.get("workspace"))
                .and_then(Item::as_bool)
                == Some(true)
        };
        let inheriting =
            ["version", "edition"]
                .iter()
                .any(|key| package.get(key).is_some_and(inherits))
                || std::iter::once(table as &dyn toml_edit::TableLike)
                    .chain(
                        table
                            .get("target")
                            .and_then(Item::as_table_like)
                            .into_iter()
                            .flat_map(|targets| targets.iter())
                            .filter_map(|(_, target)| target.as_table_like()),
                    )
                    .any(|table| {
                        DEPENDENCY_KINDS.iter().any(|kind| {
                            table.get(kind).and_then(Item::as_table_like).is_some_and(
                                |dependencies| dependencies.iter().any(|(_, item)| inherits(item)),
                            )
                        })
                    });
        let inherited = if inheriting && ask_cargo {
            Some(super::workspace::inherited(&path).map_err(|message| Error {
                message: message.into(),
                span: table.get("package").map_or_else(
                    || Span::new(Position::START, Position::START),
                    |item| item_span(&text, item),
                ),
            })?)
        } else {
            None
        };
        let string = |key: &str| match package.get(key) {
            Some(item) if inherits(item) => inherited.as_ref().and_then(|inherited| match key {
                "version" => inherited.version.clone(),
                _ => inherited.edition.clone(),
            }),
            item => item.and_then(|item| item.as_str()).map(str::to_owned),
        };
        let version = string("version");
        let edition = string("edition");
        let crate_name = table
            .get("lib")
            .and_then(|item| item.get("name"))
            .and_then(|item| item.as_str())
            .map_or_else(|| name.replace('-', "_"), str::to_owned);
        let inherits = Inherits {
            normal: inherited.as_ref().map(|inherited| &inherited.dependencies),
            build: inherited
                .as_ref()
                .map(|inherited| &inherited.build_dependencies),
            dev: inherited
                .as_ref()
                .map(|inherited| &inherited.dev_dependencies),
        };
        let dependencies =
            rust::cargo_dependencies(&text, root, table.get("dependencies"), inherits.normal)?;
        let build_dependencies =
            rust::cargo_dependencies(&text, root, table.get("build-dependencies"), inherits.build)?;
        let dev_dependencies =
            rust::cargo_dependencies(&text, root, table.get("dev-dependencies"), inherits.dev)?;
        let targets = self::targets(&text, root, table, inherits)?;
        let features = Features::parse(&text, table)?;
        let directory = path.parent().unwrap_or(root);
        let build = match package.get("build").and_then(Item::as_value) {
            Some(toml_edit::Value::String(script)) => Some(directory.join(script.value())),
            Some(toml_edit::Value::Boolean(build)) if !*build.value() => None,
            _ => Some(directory.join("build.rs")).filter(|script| script.is_file()),
        };
        let links = package
            .get("links")
            .and_then(Item::as_value)
            .map(|links| links.clone().decorated("", "").to_string());
        let library = Some(
            directory.join(
                table
                    .get("lib")
                    .and_then(|item| item.get("path"))
                    .and_then(Item::as_str)
                    .unwrap_or("src/lib.rs"),
            ),
        );
        Ok(Some(Self {
            path,
            package: name,
            crate_name,
            version,
            edition,
            dependencies,
            build_dependencies,
            dev_dependencies,
            targets,
            features,
            build,
            links,
            library,
            text,
        }))
    }

    /// The manifest with each `[target.'<platform>']` dependency that holds
    /// on `host` joined to its kind's list, as Cargo builds the package
    /// there. A table whose `cfg(..)` doesn't hold for the target Cargo
    /// builds for joins nothing, so a dependency two tables declare for
    /// two targets is the one its target's table gives (#1395). A test
    /// build's dev-dependencies, the general ones and then the tables',
    /// join the normal list after its own, as `cargo test` links them
    /// into the test harness alone. A key the list already has keeps its
    /// first entry.
    #[must_use]
    pub(crate) fn on_host(mut self, host: Host<'_>) -> Self {
        let (crates, build_crates, mode) = match host {
            Host::Built(unit) => (Some(unit.crates()), Some(unit.build_crates()), unit.mode()),
            Host::Unasked(mode) => (None, None, mode),
        };
        let targets: Vec<Targeted> = std::mem::take(&mut self.targets)
            .into_vec()
            .into_iter()
            .filter(|target| match host {
                Host::Built(unit) => {
                    platform_holds(&target.platform, unit.features()) != Some(false)
                }
                Host::Unasked(_) => true,
            })
            .collect();
        for target in &targets {
            join(&mut self.dependencies, &target.dependencies, crates);
            join(
                &mut self.build_dependencies,
                &target.build_dependencies,
                build_crates,
            );
        }
        let dev_dependencies = std::mem::take(&mut self.dev_dependencies);
        if mode == crate::project::cargo::Mode::Test {
            join(&mut self.dependencies, &dev_dependencies, crates);
            for target in &targets {
                join(&mut self.dependencies, &target.dev_dependencies, crates);
            }
        }
        self
    }

    /// The dependencies Cargo builds the package with, its default
    /// features on: [`Features::enabled`].
    pub(crate) fn enabled_dependencies(&self) -> Box<[rust::Dependency]> {
        self.features.enabled(&self.dependencies)
    }
}

/// The dependency kinds a manifest's tables declare, as Cargo spells them.
const DEPENDENCY_KINDS: [&str; 3] = ["dependencies", "build-dependencies", "dev-dependencies"];

/// The workspace's entries of each dependency kind, which a member's
/// `foo.workspace = true` inherits.
#[derive(Clone, Copy, Default)]
pub(super) struct Inherits<'a> {
    pub(super) normal: Option<&'a BTreeMap<String, rust::Dependency>>,
    pub(super) build: Option<&'a BTreeMap<String, rust::Dependency>>,
    pub(super) dev: Option<&'a BTreeMap<String, rust::Dependency>>,
}

/// Each `[target.'<platform>']` table's dependencies of each kind, as
/// written. `table` is the manifest parsed from `text`.
pub(super) fn targets(
    text: &str,
    root: &Path,
    table: &toml_edit::Table,
    inherits: Inherits<'_>,
) -> Result<Box<[Targeted]>, Error> {
    let mut targets = Vec::new();
    for (platform, target) in table
        .get("target")
        .and_then(Item::as_table_like)
        .into_iter()
        .flat_map(|targets| targets.iter())
    {
        let kind = |kind: &str| target.as_table_like().and_then(|target| target.get(kind));
        targets.push(Targeted {
            platform: platform.into(),
            dependencies: rust::cargo_dependencies(
                text,
                root,
                kind("dependencies"),
                inherits.normal,
            )?,
            build_dependencies: rust::cargo_dependencies(
                text,
                root,
                kind("build-dependencies"),
                inherits.build,
            )?,
            dev_dependencies: rust::cargo_dependencies(
                text,
                root,
                kind("dev-dependencies"),
                inherits.dev,
            )?,
        });
    }
    Ok(targets.into_boxed_slice())
}

/// Whether a `[target.'<platform>']` key holds where `features` were
/// enabled: its `cfg(..)` read as Cargo reads it. `None` for a target
/// triple, which only the crates Cargo links decide.
fn platform_holds(platform: &str, features: &Enabled) -> Option<bool> {
    let mut rest = platform.trim().strip_prefix("cfg(")?.strip_suffix(')')?;
    let held = predicate(&mut rest, features)?;
    rest.trim().is_empty().then_some(held)
}

/// The `cfg` predicate at the start of `rest`, which it consumes: `name`,
/// `name = "value"`, or `not`, `all` or `any` of predicates.
fn predicate(rest: &mut &str, features: &Enabled) -> Option<bool> {
    let text = rest.trim_start();
    let end = text
        .find(|letter: char| !(letter.is_alphanumeric() || letter == '_'))
        .unwrap_or(text.len());
    let (name, tail) = text.split_at(end);
    if name.is_empty() {
        return None;
    }
    let tail = tail.trim_start();
    if let Some(mut tail) = tail.strip_prefix('(') {
        let operator = Operator::named(name)?;
        let mut values = Vec::new();
        loop {
            tail = tail.trim_start();
            if let Some(after) = tail.strip_prefix(')') {
                *rest = after;
                return Some(operator.holds(&values));
            }
            values.push(predicate(&mut tail, features)?);
            tail = tail.trim_start();
            tail = tail.strip_prefix(',').unwrap_or(tail);
        }
    }
    if let Some(tail) = tail.strip_prefix('=') {
        let value = tail.trim_start().strip_prefix('"')?;
        let close = value.find('"')?;
        *rest = &value[close + 1..];
        return Some(features.cfg(name, Some(&value[..close]), false));
    }
    *rest = tail;
    Some(features.cfg(name, None, false))
}

/// `more` joined to `list`: each whose name is one of `crates`, the names
/// Cargo links on the host, or each when Cargo was not asked. A key the
/// list already has keeps its first entry.
fn join(
    list: &mut Box<[rust::Dependency]>,
    more: &[rust::Dependency],
    crates: Option<&[Box<str>]>,
) {
    let holds = |dependency: &rust::Dependency| {
        crates.is_none_or(|crates| crates.iter().any(|name| **name == *dependency.alias))
    };
    let mut joined = std::mem::take(list).into_vec();
    for dependency in more {
        if holds(dependency) && !joined.iter().any(|known| known.alias == dependency.alias) {
            joined.push(dependency.clone());
        }
    }
    *list = joined.into_boxed_slice();
}

#[cfg(test)]
mod tests {
    use super::Cargo;
    use std::path::{Path, PathBuf};

    /// A `[target.'<platform>']` table's dependency joins when Cargo's unit
    /// graph for the host links it, and every table's does when Cargo was
    /// not asked.
    #[test]
    fn target_dependencies_join_when_cargo_links_them_on_the_host() {
        let text = r#"
[package]
name = "tour"
version = "0.1.0"

[dependencies]
iced = "0.13"

[target."cfg(not(target_arch = \"wasm32\"))".dependencies]
tracing-subscriber = "0.3"

[target.'cfg(any(unix, windows))'.dependencies]
open = "5"

[target."cfg(target_arch = \"wasm32\")".dependencies]
console_log = "1.0"
"#;
        let cargo = Cargo::parse(
            PathBuf::from("/tour/Cargo.toml"),
            text.to_owned(),
            Path::new("/tour"),
            false,
        )
        .unwrap()
        .unwrap();
        let aliases = |cargo: &Cargo| {
            cargo
                .dependencies
                .iter()
                .map(|dependency| dependency.alias.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(aliases(&cargo), ["iced"]);
        let unit = crate::project::cargo::Unit::linking(&["iced", "open", "tracing_subscriber"]);
        assert_eq!(
            aliases(&cargo.clone().on_host(super::Host::Built(&unit))),
            ["iced", "tracing_subscriber", "open"]
        );
        assert_eq!(
            aliases(&cargo.on_host(super::Host::Unasked(crate::project::cargo::Mode::Build))),
            ["iced", "tracing_subscriber", "open", "console_log"]
        );
    }

    /// A dependency two `[target.'cfg(..)']` tables declare for two targets
    /// is the one whose `cfg` holds for the target Cargo builds for (#1395).
    #[test]
    fn a_dependency_declared_per_target_is_its_target_s() {
        let text = r#"
[package]
name = "universe"
version = "0.1.0"

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
tokio = { version = "1", features = ["rt-multi-thread"] }

[target.'cfg(all(target_arch = "wasm32", not(unix)))'.dependencies]
tokio = { version = "1", features = ["rt"] }
"#;
        let cargo = Cargo::parse(
            PathBuf::from("/universe/Cargo.toml"),
            text.to_owned(),
            Path::new("/universe"),
            false,
        )
        .unwrap()
        .unwrap();
        let features = |lines: &[&str]| {
            let unit = crate::project::cargo::Unit::linking_for(&["tokio"], lines);
            let cargo = cargo.clone().on_host(super::Host::Built(&unit));
            let [tokio] = &*cargo.dependencies else {
                panic!("one tokio: {:?}", cargo.dependencies);
            };
            tokio
                .features
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(features(&["target_arch=\"wasm32\""]), ["rt"]);
        assert_eq!(
            features(&["target_arch=\"aarch64\"", "unix"]),
            ["rt-multi-thread"]
        );
    }

    /// Cargo accepts a Git dependency at a branch, a tag, a short rev or its
    /// default branch, and pins the commit in `Cargo.lock`.
    #[test]
    fn git_dependencies_keep_the_reference_cargo_accepts() {
        use super::super::rust::GitReference;
        use super::super::rust::Source;

        let text = r#"
[package]
name = "app"
version = "0.1.0"

[dependencies]
by-branch = { git = "https://example.invalid/a", branch = "main" }
by-tag = { git = "https://example.invalid/b", tag = "v1.0.0" }
by-rev = { git = "https://example.invalid/c", rev = "23f0211" }
by-default = { git = "https://example.invalid/d" }
"#;
        let cargo = Cargo::parse(
            PathBuf::from("/app/Cargo.toml"),
            text.to_owned(),
            Path::new("/app"),
            false,
        )
        .unwrap()
        .unwrap();
        let sources = cargo
            .dependencies
            .iter()
            .map(|dependency| (dependency.alias.as_str(), &dependency.source))
            .collect::<Vec<_>>();
        let git = |url: &str, reference| Source::Git {
            url: url.to_owned(),
            reference,
            version: None,
        };
        assert_eq!(
            sources,
            [
                (
                    "by_branch",
                    &git(
                        "https://example.invalid/a",
                        GitReference::Branch("main".into())
                    )
                ),
                (
                    "by_default",
                    &git("https://example.invalid/d", GitReference::DefaultBranch)
                ),
                (
                    "by_rev",
                    &git(
                        "https://example.invalid/c",
                        GitReference::Rev("23f0211".into())
                    )
                ),
                (
                    "by_tag",
                    &git(
                        "https://example.invalid/b",
                        GitReference::Tag("v1.0.0".into())
                    )
                ),
            ]
        );

        // Cargo accepts `version` beside `git` and checks it against the
        // Git package itself (#521).
        let versioned = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n\
            dep = { git = \"https://example.invalid/a\", tag = \"v1.2.0\", version = \"1.2\" }\n";
        let cargo = Cargo::parse(
            PathBuf::from("/app/Cargo.toml"),
            versioned.to_owned(),
            Path::new("/app"),
            false,
        )
        .expect("Cargo accepts `version` beside `git`")
        .unwrap();
        assert_eq!(
            cargo.dependencies[0].source,
            Source::Git {
                url: "https://example.invalid/a".to_owned(),
                reference: GitReference::Tag("v1.2.0".into()),
                version: Some("1.2".to_owned()),
            }
        );
        #[cfg(feature = "rust-import")]
        let rendered = crate::package::rust::app_manifest(&cargo, Path::new("lib.rs"));
        #[cfg(feature = "rust-import")]
        assert!(
            rendered.contains("dep = { git = \"https://example.invalid/a\", tag = \"v1.2.0\", version = \"1.2\" }"),
            "{rendered}"
        );

        let both = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n\
            dep = { git = \"https://example.invalid/a\", path = \"dep\" }\n";
        let error = Cargo::parse(
            PathBuf::from("/app/Cargo.toml"),
            both.to_owned(),
            Path::new("/app"),
            false,
        )
        .expect_err("Cargo allows one of git and path");
        assert!(
            error
                .message
                .contains("Only one of `git` or `path` is allowed."),
            "{}",
            error.message
        );

        let ambiguous = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n\
            dep = { git = \"https://example.invalid/a\", branch = \"main\", rev = \"abc\" }\n";
        let error = Cargo::parse(
            PathBuf::from("/app/Cargo.toml"),
            ambiguous.to_owned(),
            Path::new("/app"),
            false,
        )
        .expect_err("Cargo allows one of branch, tag and rev");
        assert!(
            error.message.contains("specification is ambiguous"),
            "{}",
            error.message
        );
    }
}
