//! Validated Cargo dependency requests from the Rawr manifest.

use std::path::{Path, PathBuf};

use toml_edit::{InlineTable, Item, Table, Value};

use super::{byte_span, item_span, valid_alias, valid_package_name, Error};
use crate::{ImmutableString, Position, Span};

const DEFAULT_PROFILE: &str = "dev";

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "the resolution slice will consume this parsed proof bundle"
)]
pub(crate) struct Config {
    pub(crate) cargo_authority: Option<PathBuf>,
    pub(crate) target: Option<String>,
    pub(crate) profile: String,
    pub(crate) dependencies: Box<[Dependency]>,
    pub(crate) exclusions: Box<[Exclusion]>,
    /// `[rust.limits]` accounting-target overrides, sorted by key.
    pub(crate) limits: Box<[Limit]>,
    /// The importer limits profile with every configured target applied and
    /// validated against its physical ceiling at load.
    #[cfg(feature = "rust-import")]
    pub(crate) resolved_limits: rawr_import::cargo::Limits,
    pub(crate) span: Span,
}

/// One configured accounting target from `[rust.limits]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Limit {
    pub(crate) key: ImmutableString,
    pub(crate) value: u64,
    pub(crate) span: Span,
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

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "the resolution slice will consume this parsed proof bundle"
)]
pub(crate) struct Exclusion {
    pub(crate) alias: ImmutableString,
    pub(crate) path: Box<[ImmutableString]>,
    pub(crate) path_span: Span,
    pub(crate) path_bytes: std::ops::Range<usize>,
    pub(crate) reason: ImmutableString,
    pub(crate) reason_span: Span,
    pub(crate) reason_bytes: std::ops::Range<usize>,
}

struct Fields<'a> {
    table: &'a Table,
}

struct Specification<'a> {
    table: &'a InlineTable,
}

impl Config {
    pub(super) fn parse(text: &str, root: &Path, item: &Item) -> Result<Self, Error> {
        let table = Fields::parse(text, item)?;

        let cargo_authority = optional_nonempty_string(text, &table, "cargo-authority")?
            .map(|authority| root.join(authority));
        let target = optional_nonempty_string(text, &table, "target")?.map(str::to_owned);
        let profile = optional_nonempty_string(text, &table, "profile")?
            .unwrap_or(DEFAULT_PROFILE)
            .to_owned();
        let dependencies = parse_dependencies(text, root, table.table.get("dependencies"))?;
        let exclusions = parse_exclusions(text, table.table.get("exclude"))?;
        let limits = parse_limits(text, table.table.get("limits"))?;
        #[cfg(feature = "rust-import")]
        let resolved_limits = resolve_limits(&limits)?;
        for exclusion in &exclusions {
            if !dependencies
                .iter()
                .any(|dependency| dependency.alias == exclusion.alias)
            {
                return Err(Error {
                    message: format!(
                        "Rust exclusion uses unknown dependency alias `{}`",
                        exclusion.alias
                    )
                    .into(),
                    span: exclusion.path_span,
                });
            }
        }

        Ok(Self {
            cargo_authority,
            target,
            profile,
            dependencies,
            exclusions,
            limits,
            #[cfg(feature = "rust-import")]
            resolved_limits,
            span: item_span(text, item),
        })
    }
}

/// Parse `[rust.limits]`: a flat table of positive integer accounting targets.
/// Key validation and physical-ceiling refusal happen in `resolve_limits`.
fn parse_limits(text: &str, item: Option<&Item>) -> Result<Box<[Limit]>, Error> {
    let Some(item) = item else {
        return Ok(Box::default());
    };
    let table = item.as_table_like().ok_or_else(|| Error {
        message: "`rust.limits` must be a table of positive integers".into(),
        span: item_span(text, item),
    })?;
    let mut limits = Vec::with_capacity(table.len());
    for (key, item) in table.iter() {
        let span = item_span(text, item);
        let value = item
            .as_integer()
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| Error {
                message: format!("`rust.limits.{key}` must be a positive integer").into(),
                span,
            })?;
        limits.push(Limit {
            key: key.into(),
            value,
            span,
        });
    }
    limits.sort_unstable_by(|left, right| left.key.cmp(&right.key));
    if let Some(pair) = limits.windows(2).find(|pair| pair[0].key == pair[1].key) {
        return Err(Error {
            message: format!("`rust.limits.{}` is configured twice", pair[1].key).into(),
            span: pair[1].span,
        });
    }
    Ok(limits.into_boxed_slice())
}

/// Apply every configured target to the standard importer profile, failing
/// closed at load on unknown keys and on any target above a physical ceiling.
#[cfg(feature = "rust-import")]
fn resolve_limits(limits: &[Limit]) -> Result<rawr_import::cargo::Limits, Error> {
    let mut resolved = rawr_import::cargo::Limits::standard();
    for limit in limits {
        resolved
            .configure(&limit.key, limit.value)
            .map_err(|error| Error {
                message: error.to_string().into(),
                span: limit.span,
            })?;
    }
    Ok(resolved)
}

impl<'a> Fields<'a> {
    fn parse(text: &str, item: &'a Item) -> Result<Self, Error> {
        let table = item.as_table().ok_or_else(|| Error {
            message: "`rust` must be a table".into(),
            span: item_span(text, item),
        })?;
        for (key, item) in table.iter() {
            if !matches!(
                key,
                "cargo-authority" | "target" | "profile" | "dependencies" | "exclude" | "limits"
            ) {
                return Err(Error {
                    message: format!("unknown rust key `{key}`").into(),
                    span: item_span(text, item),
                });
            }
        }
        Ok(Self { table })
    }
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

fn optional_nonempty_string<'a>(
    text: &str,
    table: &'a Fields<'_>,
    key: &str,
) -> Result<Option<&'a str>, Error> {
    let Some(item) = table.table.get(key) else {
        return Ok(None);
    };
    let value = item.as_str().ok_or_else(|| Error {
        message: format!("`rust.{key}` must be a string").into(),
        span: item_span(text, item),
    })?;
    if value.is_empty() {
        return Err(Error {
            message: format!("`rust.{key}` must not be empty").into(),
            span: item_span(text, item),
        });
    }
    Ok(Some(value))
}

fn parse_dependencies(
    text: &str,
    root: &Path,
    item: Option<&Item>,
) -> Result<Box<[Dependency]>, Error> {
    let Some(item) = item else {
        return Ok(Box::default());
    };
    let table = item.as_table().ok_or_else(|| Error {
        message: "`rust.dependencies` must be a table".into(),
        span: item_span(text, item),
    })?;
    let mut dependencies = Vec::with_capacity(table.len());
    for (alias, item) in table.iter() {
        if !valid_alias(alias) {
            return Err(Error {
                message: format!("invalid Rust dependency alias `{alias}`").into(),
                span: item_span(text, item),
            });
        }
        // A version string is `{ version = "..." }`, as in Cargo.
        let dependency = match item {
            Item::Value(Value::String(version)) => {
                let mut inline = InlineTable::new();
                inline.insert("version", version.value().as_str().into());
                let normalized = Item::Value(Value::InlineTable(inline));
                let mut dependency = parse_dependency(text, root, alias, &normalized)?;
                dependency.span = item_span(text, item);
                dependency
            }
            _ => parse_dependency(text, root, alias, item)?,
        };
        dependencies.push(dependency);
    }
    dependencies.sort_unstable_by(|left, right| left.alias.cmp(&right.alias));
    Ok(dependencies.into_boxed_slice())
}

/// The package of the `rawr` attribute crate (`#[rawr::preview]`).
const ATTRIBUTE_CRATE: &str = "rawr-attributes";

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
    /// The `[rust]` table a Cargo package manifest implies: its
    /// dependencies, itself as the Cargo authority (profiles and patches),
    /// the `dev` profile and no exclusions or limit overrides.
    pub(crate) fn from_cargo(manifest: &Path, dependencies: Box<[Dependency]>) -> Self {
        Self {
            cargo_authority: Some(manifest.to_path_buf()),
            target: None,
            profile: DEFAULT_PROFILE.to_owned(),
            dependencies,
            exclusions: Box::default(),
            limits: Box::default(),
            #[cfg(feature = "rust-import")]
            resolved_limits: rawr_import::cargo::Limits::standard(),
            span: start_span(),
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

fn parse_exclusions(text: &str, item: Option<&Item>) -> Result<Box<[Exclusion]>, Error> {
    let Some(item) = item else {
        return Ok(Box::default());
    };
    let table = item.as_table().ok_or_else(|| Error {
        message: "`rust.exclude` must be a table".into(),
        span: item_span(text, item),
    })?;
    let mut exclusions = Vec::with_capacity(table.len());
    for (configured, item) in table.iter() {
        let mut components = configured.split("::");
        let alias = components.next().unwrap_or_default();
        let path = components.collect::<Vec<_>>();
        if !valid_alias(alias)
            || path.is_empty()
            || path
                .iter()
                .any(|component| !valid_rust_identifier(component))
        {
            return Err(Error {
                message: format!(
                    "Rust exclusion `{configured}` must be `alias::path` with valid Rust identifiers"
                )
                .into(),
                span: table_key_span(text, table, configured, item),
            });
        }
        let reason = item.as_str().ok_or_else(|| Error {
            message: format!("Rust exclusion `{configured}` reason must be a string").into(),
            span: item_span(text, item),
        })?;
        if reason.is_empty() {
            return Err(Error {
                message: format!("Rust exclusion `{configured}` reason must not be empty").into(),
                span: item_span(text, item),
            });
        }
        let path_bytes = table_key_range(table, configured, item);
        let reason_bytes = item.span().unwrap_or(0..0);
        exclusions.push(Exclusion {
            alias: alias.into(),
            path: path
                .into_iter()
                .map(ImmutableString::from)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            path_span: byte_span(text, path_bytes.clone()),
            path_bytes,
            reason: reason.into(),
            reason_span: item_span(text, item),
            reason_bytes,
        });
    }
    exclusions.sort_unstable_by(|left, right| {
        left.alias
            .cmp(&right.alias)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(exclusions.into_boxed_slice())
}

fn valid_rust_identifier(identifier: &str) -> bool {
    crate::tokenizer::is_valid_identifier(identifier)
        && !matches!(
            identifier,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "dyn"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "Self"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
                | "abstract"
                | "become"
                | "box"
                | "do"
                | "final"
                | "gen"
                | "macro"
                | "override"
                | "priv"
                | "try"
                | "typeof"
                | "unsized"
                | "virtual"
                | "yield"
        )
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

fn table_key_span(text: &str, table: &Table, key: &str, fallback: &Item) -> Span {
    byte_span(text, table_key_range(table, key, fallback))
}

fn table_key_range(table: &Table, key: &str, fallback: &Item) -> std::ops::Range<usize> {
    table
        .key(key)
        .and_then(toml_edit::Key::span)
        .or_else(|| fallback.span())
        .unwrap_or(0..0)
}

const fn start_span() -> Span {
    Span::new(Position::START, Position::START)
}

#[cfg(test)]
mod manifest_rust {
    use std::path::Path;

    use super::{start_span, Source};
    use crate::package::manifest::Manifest;

    #[test]
    fn valid_sources_defaults_and_order_are_canonical() {
        let manifest = parse(
            r#"
                [rust]
                cargo-authority = "../Cargo.toml"
                target = "aarch64-apple-darwin"

                [rust.dependencies]
                zebra = { git = "https://example.invalid/zebra", rev = "abc123", features = ["serde", "alloc", "serde"], default-features = false }
                alpha = { version = "1.2", package = "alpha-core" }
                middle = { path = "../middle" }
            "#,
        );
        let config = manifest.rust.expect("Rust configuration is present");

        assert_eq!(config.cargo_authority, Some(root().join("../Cargo.toml")));
        assert_eq!(config.target.as_deref(), Some("aarch64-apple-darwin"));
        assert_eq!(config.profile, "dev");
        assert_eq!(
            config
                .dependencies
                .iter()
                .map(|dependency| dependency.alias.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "middle", "zebra"]
        );
        assert_eq!(
            config.dependencies[0].package.as_deref(),
            Some("alpha-core")
        );
        assert_eq!(
            config.dependencies[0].source,
            Source::Registry {
                version: "1.2".to_owned()
            }
        );
        assert!(config.dependencies[0].default_features);
        assert!(config.dependencies[0].features.is_empty());
        assert_eq!(
            config.dependencies[1].source,
            Source::Path(root().join("../middle"))
        );
        assert_eq!(
            config.dependencies[2].source,
            Source::Git {
                url: "https://example.invalid/zebra".to_owned(),
                rev: "abc123".to_owned()
            }
        );
        assert_eq!(config.dependencies[2].features.as_ref(), ["alloc", "serde"]);
        assert!(!config.dependencies[2].default_features);
        assert_ne!(config.dependencies[0].span, start_span());
    }

    #[test]
    fn a_version_string_is_a_registry_dependency_as_in_cargo() {
        let manifest = parse(
            r#"
                [rust.dependencies]
                rand = "0.9"
            "#,
        );
        let config = manifest.rust.expect("Rust configuration is present");

        assert_eq!(config.dependencies[0].alias, "rand");
        assert_eq!(
            config.dependencies[0].source,
            Source::Registry {
                version: "0.9".to_owned()
            }
        );
        assert!(config.dependencies[0].default_features);
        assert_ne!(config.dependencies[0].span, start_span());

        let error = Manifest::parse("[rust.dependencies]\nrand = 9\n", root())
            .expect_err("a number is not a dependency");
        assert_eq!(
            error.message.as_str(),
            "Rust dependency `rand` must be a version string or use `{ ... }`"
        );
    }

    #[test]
    fn explicit_profile_and_exclusion_spans_are_retained() {
        let manifest = parse(
            r#"
                [rust]
                profile = "release"

                [rust.dependencies]
                alpha = { version = "1" }
                zebra = { version = "1" }

                [rust.exclude]
                "zebra::unstable" = "platform-specific"
                "alpha::private" = "not part of this contract"
            "#,
        );
        let config = manifest.rust.expect("Rust configuration is present");

        assert_eq!(config.profile, "release");
        assert_eq!(
            config
                .exclusions
                .iter()
                .map(|exclusion| {
                    format!(
                        "{}::{}",
                        exclusion.alias,
                        exclusion
                            .path
                            .iter()
                            .map(|part| part.as_str())
                            .collect::<Vec<_>>()
                            .join("::")
                    )
                })
                .collect::<Vec<_>>(),
            ["alpha::private", "zebra::unstable"]
        );
        assert_eq!(config.exclusions[0].reason, "not part of this contract");
        assert_ne!(config.exclusions[0].path_span, start_span());
        assert_ne!(config.exclusions[0].reason_span, start_span());
        assert_ne!(
            config.exclusions[0].path_span,
            config.exclusions[0].reason_span
        );
    }

    #[test]
    fn invalid_source_shapes_and_values_are_positioned() {
        let cases = [
            "[rust.dependencies]\nmissing = {}\n",
            "[rust.dependencies]\nmixed = { path = \"../x\", version = \"1\" }\n",
            "[rust.dependencies]\ngit = { git = \"https://example.invalid/git\" }\n",
            "[rust.dependencies]\nrev = { rev = \"abc\" }\n",
            "[rust.dependencies]\nbad = { path = 4 }\n",
            "[rust.dependencies]\nbad = { path = \"../x\", features = [\"ok\", 4] }\n",
            "[rust.dependencies]\nbad = { path = \"../x\", features = \"ok\" }\n",
            "[rust.dependencies]\nbad = { path = \"../x\", default-features = \"no\" }\n",
            "[rust.dependencies]\nbad = { path = \"../x\", package = true }\n",
            "[rust.dependencies]\nbad-name = { path = \"../x\" }\n",
            "[rust.dependencies]\nbad = { path = \"../x\", unknown = true }\n",
            "[rust]\ntarget = false\n",
            "[rust]\nunknown = true\n",
            "[rust.exclude]\n\"x::y\" = false\n",
            "[rust.exclude]\n\"x::fn\" = \"keyword is not an item identifier\"\n",
        ];

        for text in cases {
            let error = Manifest::parse(text, root()).expect_err(text);
            assert_ne!(error.span, start_span(), "{text}: {}", error.message);
        }
    }

    #[test]
    fn duplicate_cargo_authorities_are_rejected_as_ambiguous() {
        let text = "[rust]\ncargo-authority = \"../one/Cargo.toml\"\ncargo-authority = \"../two/Cargo.toml\"\n";
        let error = Manifest::parse(text, root()).expect_err("duplicate authority must reject");

        assert_ne!(error.span, start_span());
    }

    #[test]
    fn rawr_dependencies_remain_separate_and_in_declaration_order() {
        let manifest = parse(
            r#"
                [dependencies]
                second = { path = "../second" }
                first = { path = "../first" }

                [rust.dependencies]
                second = { version = "2" }
                first = { version = "1" }
            "#,
        );

        assert_eq!(
            manifest
                .dependencies
                .iter()
                .map(|dependency| dependency.alias.as_str())
                .collect::<Vec<_>>(),
            ["second", "first"]
        );
        assert_eq!(manifest.dependencies[0].path, root().join("../second"));
        assert_eq!(
            manifest
                .rust
                .expect("Rust configuration is present")
                .dependencies
                .iter()
                .map(|dependency| dependency.alias.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    /// `[rust.limits]` configures accounting targets by name; the table
    /// is parsed into sorted positive integers, duplicates are rejected, and
    /// (with the importer linked) unknown keys and any target above a physical
    /// ceiling are refused at load with a positioned message.
    #[test]
    fn rust_limits_table_configures_accounting_targets_and_fails_closed() {
        let manifest = parse(
            r#"
                [rust]
                cargo-authority = "../Cargo.toml"

                [rust.limits]
                graph-path-checks = 70000
                request-bytes = 1048576
                graph-decode-bytes = 67108864
            "#,
        );
        let config = manifest.rust.expect("Rust configuration is present");
        assert_eq!(
            config
                .limits
                .iter()
                .map(|limit| (limit.key.as_str(), limit.value))
                .collect::<Vec<_>>(),
            [
                ("graph-decode-bytes", 67_108_864),
                ("graph-path-checks", 70_000),
                ("request-bytes", 1_048_576),
            ]
        );
        #[cfg(feature = "rust-import")]
        {
            let standard = rawr_import::cargo::Limits::standard();
            assert_ne!(config.resolved_limits, standard);
            assert!(config.resolved_limits.is_configured());
            assert!(!standard.is_configured());
            let mut expected = standard;
            expected.configure("graph-path-checks", 70_000).unwrap();
            expected.configure("request-bytes", 1_048_576).unwrap();
            expected
                .configure("graph-decode-bytes", 67_108_864)
                .unwrap();
            assert_eq!(config.resolved_limits, expected);
        }
        assert!(parse("[rust]\ncargo-authority = \"../Cargo.toml\"\n")
            .rust
            .unwrap()
            .limits
            .is_empty());

        for text in [
            "[rust]\n[rust.limits]\ngraph-path-checks = 0\n",
            "[rust]\n[rust.limits]\ngraph-path-checks = -1\n",
            "[rust]\n[rust.limits]\ngraph-path-checks = \"many\"\n",
            "[rust]\nlimits = 7\n",
        ] {
            let error = Manifest::parse(text, root()).expect_err(text);
            assert_ne!(error.span, start_span(), "{text}: {}", error.message);
            assert!(error.message.contains("positive integer") || error.message.contains("table"));
        }
        #[cfg(feature = "rust-import")]
        for (text, expected) in [
            (
                "[rust]\n[rust.limits]\ndepth = 512\n",
                "not a configurable accounting target",
            ),
            (
                "[rust]\n[rust.limits]\nstdout-bytes = 1\n",
                "not a configurable accounting target",
            ),
            (
                "[rust]\n[rust.limits]\njson-bytes = 268435457\n",
                "exceeds the physical child stdout capture ceiling 268435456",
            ),
            (
                "[rust]\n[rust.limits]\ncanonical-bytes = 1207959553\n",
                "exceeds the physical node store payload ceiling 1207959552",
            ),
        ] {
            let error = Manifest::parse(text, root()).expect_err(text);
            assert_ne!(error.span, start_span(), "{text}: {}", error.message);
            assert!(
                error.message.contains(expected),
                "{text}: {}",
                error.message
            );
        }
    }

    /// `request-bytes` targets the manifest-class read, whose physical
    /// ceiling is `process::TAIL_BYTES`; one byte above it is refused at load
    /// with a message positioned on the configured value.
    #[cfg(feature = "rust-import")]
    #[test]
    fn rust_limits_request_bytes_above_the_manifest_read_ceiling_is_refused_at_load() {
        let text = "[rust]\n[rust.limits]\nrequest-bytes = 1048577\n";
        let error = Manifest::parse(text, root()).expect_err(text);
        assert_ne!(error.span, start_span(), "{}", error.message);
        assert_eq!(error.span.start().line(), Some(3), "{}", error.message);
        assert!(
            error
                .message
                .contains("exceeds the physical manifest-class read ceiling 1048576"),
            "{}",
            error.message
        );
    }

    fn parse(text: &str) -> Manifest {
        Manifest::parse(text, root()).expect("manifest fixture is valid")
    }

    fn root() -> &'static Path {
        Path::new("/workspace/app")
    }
}
