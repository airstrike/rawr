//! The crate's Cargo dependencies as Rust import requests.

use std::path::{Path, PathBuf};

use toml_edit::{InlineTable, Item, Value};

use super::{byte_span, item_span, valid_alias, valid_package_name, Error};
use crate::{ImmutableString, Position, Span};

const DEFAULT_PROFILE: &str = "dev";

/// What the crate imports from Rust: its Cargo dependencies, with its own
/// `Cargo.toml` as the Cargo authority (profiles and patches).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Config {
    pub(crate) cargo_authority: PathBuf,
    pub(crate) profile: String,
    pub(crate) dependencies: Box<[Dependency]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "the resolution slice will consume this parsed proof bundle"
)]
pub(crate) struct Dependency {
    pub(crate) alias: ImmutableString,
    pub(crate) package: Option<String>,
    pub(crate) source: Source,
    pub(crate) features: Box<[String]>,
    pub(crate) default_features: bool,
    pub(crate) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "the resolution slice will consume this parsed proof bundle"
)]
pub(crate) enum Source {
    Path(PathBuf),
    Registry {
        version: String,
    },
    Git {
        url: String,
        rev: String,
    },
    /// `{ workspace = true }` in a runner's snapshot, which does not ask
    /// Cargo. The runner reads only the alias.
    Workspace,
}

struct Specification<'a> {
    table: &'a InlineTable,
}

impl<'a> Specification<'a> {
    fn parse(text: &str, alias: &str, item: &'a Item) -> Result<Self, Error> {
        let value = item.as_value().ok_or_else(|| Error {
            message: format!("Rust dependency `{alias}` must be an inline table").into(),
            span: item_span(text, item),
        })?;
        let table = value.as_inline_table().ok_or_else(|| Error {
            message: format!(
                "Rust dependency `{alias}` must be a version string or use `{{ ... }}`"
            )
            .into(),
            span: item_span(text, item),
        })?;
        for (key, value) in table.iter() {
            if !matches!(
                key,
                "package" | "path" | "version" | "git" | "rev" | "features" | "default-features"
            ) {
                return Err(Error {
                    message: format!("unknown key `{key}` for Rust dependency `{alias}`").into(),
                    span: value_span(text, value),
                });
            }
        }
        Ok(Self { table })
    }
}

/// The package of the `rawr` attribute crate (`#[rawr::preview]`).
const ATTRIBUTE_CRATE: &str = "rawr-macros";

/// A Cargo `[dependencies]` table in Rawr's Rust dependency form. A
/// version string is `{ version = "..." }`; a `-` in a key becomes `_` in the
/// alias with the key kept as the package; a path wins over a version as it
/// does locally; optional dependencies are off by default and skipped.
pub(super) fn cargo_dependencies(
    text: &str,
    root: &Path,
    item: Option<&Item>,
    inherited: Option<&super::workspace::Inherited>,
) -> Result<Box<[Dependency]>, Error> {
    let Some(item) = item else {
        return Ok(Box::default());
    };
    let table = item.as_table_like().ok_or_else(|| Error {
        message: "Cargo `dependencies` must be a table".into(),
        span: item_span(text, item),
    })?;
    let mut dependencies = Vec::with_capacity(table.len());
    for (key, item) in table.iter() {
        let alias = key.replace('-', "_");
        if !valid_alias(&alias) {
            return Err(Error {
                message: format!("Cargo dependency `{key}` has no valid Rust crate name").into(),
                span: item_span(text, item),
            });
        }
        let mut inline = match item {
            Item::Value(Value::String(version)) => {
                let mut inline = InlineTable::new();
                inline.insert("version", version.value().as_str().into());
                inline
            }
            Item::Value(Value::InlineTable(inline)) => inline.clone(),
            Item::Table(table) => table.clone().into_inline_table(),
            _ => {
                return Err(Error {
                    message: format!(
                        "Cargo dependency `{key}` must be a version string or a table"
                    )
                    .into(),
                    span: item_span(text, item),
                });
            }
        };
        if inline.get("optional").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        // The `rawr` attribute crate is for rustc. Rawr reads its
        // attributes itself, so it imports nothing from it.
        let package = inline.get("package").and_then(Value::as_str).unwrap_or(key);
        if package == ATTRIBUTE_CRATE {
            continue;
        }
        inline.remove("optional");
        if inline.contains_key("workspace") {
            // Cargo resolved the inherited entry, member features included.
            let Some(inherited) = inherited else {
                dependencies.push(Dependency {
                    alias: alias.as_str().into(),
                    package: None,
                    source: Source::Workspace,
                    features: Box::default(),
                    default_features: true,
                    span: item_span(text, item),
                });
                continue;
            };
            let dependency = inherited.dependencies.get(key).ok_or_else(|| Error {
                message: format!(
                    "Cargo dependency `{key}` inherits from a workspace that Cargo could not read"
                )
                .into(),
                span: item_span(text, item),
            })?;
            let mut dependency = dependency.clone();
            dependency.span = item_span(text, item);
            dependencies.push(dependency);
            continue;
        }
        if inline.contains_key("path") {
            inline.remove("version");
        }
        if alias != key && !inline.contains_key("package") {
            inline.insert("package", key.into());
        }
        let normalized = Item::Value(Value::InlineTable(inline));
        let mut dependency = parse_dependency(text, root, &alias, &normalized)?;
        dependency.span = item_span(text, item);
        dependencies.push(dependency);
    }
    dependencies.sort_unstable_by(|left, right| left.alias.cmp(&right.alias));
    Ok(dependencies.into_boxed_slice())
}

impl Config {
    /// The imports a Cargo package manifest implies: its dependencies,
    /// itself as the Cargo authority and the `dev` profile.
    pub(crate) fn from_cargo(manifest: &Path, dependencies: Box<[Dependency]>) -> Self {
        Self {
            cargo_authority: manifest.to_path_buf(),
            profile: DEFAULT_PROFILE.to_owned(),
            dependencies,
        }
    }
}

fn parse_dependency(
    text: &str,
    root: &Path,
    alias: &str,
    item: &Item,
) -> Result<Dependency, Error> {
    let specification = Specification::parse(text, alias, item)?;
    let table = specification.table;

    let package = optional_dependency_string(text, alias, table, "package")?.map(str::to_owned);
    if let Some(package) = package.as_deref()
        && !valid_package_name(package)
    {
        return Err(Error {
            message: format!("invalid package name `{package}` for Rust dependency `{alias}`")
                .into(),
            span: inline_value_span(text, table, "package"),
        });
    }

    let source = Source::parse(text, root, alias, item, table)?;
    let features = parse_features(text, alias, table)?;
    let default_features = parse_default_features(text, alias, table)?;

    Ok(Dependency {
        alias: alias.into(),
        package,
        source,
        features,
        default_features,
        span: item_span(text, item),
    })
}

fn optional_dependency_string<'a>(
    text: &str,
    alias: &str,
    table: &'a InlineTable,
    key: &str,
) -> Result<Option<&'a str>, Error> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    let value = value.as_str().ok_or_else(|| Error {
        message: format!("Rust dependency `{alias}` `{key}` must be a string").into(),
        span: value_span(text, value),
    })?;
    if value.is_empty() {
        return Err(Error {
            message: format!("Rust dependency `{alias}` `{key}` must not be empty").into(),
            span: inline_value_span(text, table, key),
        });
    }
    Ok(Some(value))
}

impl Source {
    fn parse(
        text: &str,
        root: &Path,
        alias: &str,
        item: &Item,
        table: &InlineTable,
    ) -> Result<Self, Error> {
        let path = optional_dependency_string(text, alias, table, "path")?;
        let version = optional_dependency_string(text, alias, table, "version")?;
        let git = optional_dependency_string(text, alias, table, "git")?;
        let rev = optional_dependency_string(text, alias, table, "rev")?;
        match (path, version, git, rev) {
            (Some(path), None, None, None) => Ok(Self::Path(root.join(path))),
            (None, Some(version), None, None) => Ok(Self::Registry {
                version: version.to_owned(),
            }),
            (None, None, Some(url), Some(rev)) => Ok(Self::Git {
                url: url.to_owned(),
                rev: rev.to_owned(),
            }),
            (None, None, Some(_), None) => Err(Error {
                message: format!("Rust dependency `{alias}` with `git` also requires `rev`").into(),
                span: inline_value_span(text, table, "git"),
            }),
            (None, None, None, Some(_)) => Err(Error {
                message: format!("Rust dependency `{alias}` has `rev` without `git`").into(),
                span: inline_value_span(text, table, "rev"),
            }),
            (None, None, None, None) => Err(Error {
                message: format!(
                    "Rust dependency `{alias}` requires exactly one of `path`, `version`, or `git` + `rev`"
                )
                .into(),
                span: item_span(text, item),
            }),
            _ => Err(Error {
                message: format!(
                    "Rust dependency `{alias}` must use exactly one of `path`, `version`, or `git` + `rev`"
                )
                .into(),
                span: item_span(text, item),
            }),
        }
    }
}

fn parse_features(text: &str, alias: &str, table: &InlineTable) -> Result<Box<[String]>, Error> {
    let Some(value) = table.get("features") else {
        return Ok(Box::default());
    };
    let array = value.as_array().ok_or_else(|| Error {
        message: format!("Rust dependency `{alias}` `features` must be an array of strings").into(),
        span: value_span(text, value),
    })?;
    let mut features = Vec::with_capacity(array.len());
    for value in array.iter() {
        let feature = value.as_str().ok_or_else(|| Error {
            message: format!("Rust dependency `{alias}` features must contain only strings").into(),
            span: value_span(text, value),
        })?;
        if feature.is_empty() {
            return Err(Error {
                message: format!(
                    "Rust dependency `{alias}` features must not contain an empty name"
                )
                .into(),
                span: value_span(text, value),
            });
        }
        features.push(feature.to_owned());
    }
    features.sort_unstable();
    features.dedup();
    Ok(features.into_boxed_slice())
}

fn parse_default_features(text: &str, alias: &str, table: &InlineTable) -> Result<bool, Error> {
    let Some(value) = table.get("default-features") else {
        return Ok(true);
    };
    value.as_bool().ok_or_else(|| Error {
        message: format!("Rust dependency `{alias}` `default-features` must be a boolean").into(),
        span: value_span(text, value),
    })
}

fn inline_value_span(text: &str, table: &InlineTable, key: &str) -> Span {
    table
        .get(key)
        .map(|value| value_span(text, value))
        .unwrap_or_else(start_span)
}

fn value_span(text: &str, value: &Value) -> Span {
    value
        .span()
        .map(|range| byte_span(text, range))
        .unwrap_or_else(start_span)
}

const fn start_span() -> Span {
    Span::new(Position::START, Position::START)
}
