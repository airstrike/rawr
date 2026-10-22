//! Public typed state, without invoking Cargo's lossy Serialize implementations.

use std::collections::{BTreeMap, BTreeSet};

use cargo_util_schemas::{core, manifest};
use serde_json::{json, Value};

pub(crate) trait Project {
    fn project(&self) -> Value;
}

fn tagged(kind: &str, value: Value) -> Value {
    json!({"type": kind, "value": value})
}

impl<T: Project + ?Sized> Project for &T {
    fn project(&self) -> Value {
        (**self).project()
    }
}
impl<T: Project> Project for Box<T> {
    fn project(&self) -> Value {
        (**self).project()
    }
}
impl<T: Project> Project for Option<T> {
    fn project(&self) -> Value {
        match self {
            Some(value) => tagged("Some", value.project()),
            None => tagged("None", Value::Null),
        }
    }
}
impl<T: Project> Project for Vec<T> {
    fn project(&self) -> Value {
        Value::Array(self.iter().map(Project::project).collect())
    }
}
impl<T: Project> Project for BTreeSet<T> {
    fn project(&self) -> Value {
        Value::Array(self.iter().map(Project::project).collect())
    }
}
impl<K: Project, V: Project> Project for BTreeMap<K, V> {
    fn project(&self) -> Value {
        tagged(
            "BTreeMap",
            Value::Array(
                self.iter()
                    .map(|(key, value)| json!([key.project(), value.project()]))
                    .collect(),
            ),
        )
    }
}
impl Project for str {
    fn project(&self) -> Value {
        Value::String(self.to_owned())
    }
}
impl Project for String {
    fn project(&self) -> Value {
        self.as_str().project()
    }
}
macro_rules! scalar {
    ($($ty:ty),* $(,)?) => {$(
        impl Project for $ty {
            fn project(&self) -> Value { json!(self) }
        }
    )*};
}
scalar!(bool, u64, u32, i64, i8);

// Exhaustive destructuring deliberately fails to compile if the pinned schema
// adds a public field. This is projection, not a reimplementation of visitors.
macro_rules! fields {
    ($name:ident {}) => {
        impl Project for manifest::$name {
            fn project(&self) -> Value {
                let manifest::$name {} = self;
                json!({"type": stringify!($name), "fields": {}})
            }
        }
    };
    ($name:ident { $($field:ident),+ $(,)? }) => {
        impl Project for manifest::$name {
            fn project(&self) -> Value {
                let manifest::$name { $($field),* } = self;
                let mut fields = serde_json::Map::new();
                $(fields.insert(stringify!($field).to_owned(), $field.project());)+
                json!({"type": stringify!($name), "fields": fields})
            }
        }
    };
}

fields!(TomlManifest {
    cargo_features,
    package,
    project,
    badges,
    features,
    lib,
    bin,
    example,
    test,
    bench,
    dependencies,
    dev_dependencies,
    dev_dependencies2,
    build_dependencies,
    build_dependencies2,
    target,
    lints,
    hints,
    workspace,
    profile,
    patch,
    replace,
    _unused_keys,
});

fields!(TomlWorkspace {
    members,
    exclude,
    default_members,
    resolver,
    metadata,
    package,
    dependencies,
    lints,
});

fields!(InheritablePackage {
    version,
    authors,
    description,
    homepage,
    documentation,
    readme,
    keywords,
    categories,
    license,
    license_file,
    repository,
    publish,
    edition,
    badges,
    exclude,
    include,
    rust_version,
});

fields!(TomlPackage {
    edition,
    rust_version,
    name,
    version,
    authors,
    build,
    metabuild,
    default_target,
    forced_target,
    links,
    exclude,
    include,
    publish,
    workspace,
    im_a_teapot,
    autolib,
    autobins,
    autoexamples,
    autotests,
    autobenches,
    default_run,
    description,
    homepage,
    documentation,
    readme,
    keywords,
    categories,
    license,
    license_file,
    repository,
    resolver,
    metadata,
    _invalid_cargo_features,
});

fields!(TomlInheritedDependency {
    workspace,
    features,
    default_features,
    default_features2,
    optional,
    public,
    _unused_keys,
});

fields!(TomlDetailedDependency {
    version,
    registry,
    registry_index,
    path,
    base,
    git,
    branch,
    tag,
    rev,
    features,
    optional,
    default_features,
    default_features2,
    package,
    public,
    artifact,
    lib,
    target,
    _unused_keys,
});

fields!(TomlProfile {
    opt_level,
    lto,
    codegen_backend,
    codegen_units,
    debug,
    split_debuginfo,
    debug_assertions,
    rpath,
    panic,
    overflow_checks,
    incremental,
    dir_name,
    inherits,
    strip,
    rustflags,
    package,
    build_override,
    trim_paths,
    hint_mostly_unused,
});

fields!(TomlTarget {
    name,
    crate_type,
    crate_type2,
    path,
    filename,
    test,
    doctest,
    bench,
    doc,
    doc_scrape_examples,
    proc_macro,
    proc_macro2,
    harness,
    required_features,
    edition,
});

fields!(TomlPlatform {
    dependencies,
    build_dependencies,
    build_dependencies2,
    dev_dependencies,
    dev_dependencies2,
});

fields!(InheritableLints { workspace, lints });

fields!(TomlLintConfig {
    level,
    priority,
    config,
});

fields!(Hints { mostly_unused });

fields!(InvalidCargoFeatures {});

impl<T: Project> Project for manifest::InheritableField<T> {
    fn project(&self) -> Value {
        match self {
            Self::Value(value) => tagged("InheritableField::Value", value.project()),
            // The only private member is the zero-sized WorkspaceValue. Its
            // successful conversion is exactly true (upstream manifest:621–659).
            Self::Inherit(_) => tagged("InheritableField::Inherit", json!({"workspace": true})),
        }
    }
}
macro_rules! variants {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        impl Project for manifest::$name {
            fn project(&self) -> Value {
                match self { $(Self::$variant(value) => tagged(
                    concat!(stringify!($name), "::", stringify!($variant)), value.project()
                )),* }
            }
        }
    };
}
variants!(InheritableDependency { Value, Inherit });
variants!(TomlDependency { Simple, Detailed });
variants!(StringOrBool { String, Bool });
variants!(TomlPackageBuild {
    Auto,
    SingleScript,
    MultipleScript
});
variants!(VecStringOrBool { VecString, Bool });
variants!(TomlLint { Level, Config });

macro_rules! unit_variants {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        impl Project for manifest::$name {
            fn project(&self) -> Value {
                match self { $(Self::$variant => tagged(
                    concat!(stringify!($name), "::", stringify!($variant)), Value::Null
                )),* }
            }
        }
    };
}
unit_variants!(TomlDebugInfo {
    None,
    LineDirectivesOnly,
    LineTablesOnly,
    Limited,
    Full
});
unit_variants!(TomlLintLevel {
    Forbid,
    Deny,
    Warn,
    Allow
});
unit_variants!(TomlTrimPathsValue {
    Diagnostics,
    Macro,
    Object
});

impl Project for manifest::TomlTrimPaths {
    fn project(&self) -> Value {
        match self {
            Self::Values(value) => tagged("TomlTrimPaths::Values", value.project()),
            Self::All => tagged("TomlTrimPaths::All", Value::Null),
        }
    }
}
impl Project for manifest::TomlProfiles {
    fn project(&self) -> Value {
        tagged("TomlProfiles", self.0.project())
    }
}
impl Project for manifest::StringOrVec {
    fn project(&self) -> Value {
        tagged("StringOrVec", self.0.project())
    }
}
impl Project for manifest::TomlOptLevel {
    fn project(&self) -> Value {
        tagged("TomlOptLevel", self.0.project())
    }
}
impl Project for manifest::PathValue {
    fn project(&self) -> Value {
        // This field is constructed from a TOML String; no lossy pathname conversion.
        tagged(
            "PathValue",
            json!({"utf8": self.0.to_str(),
            "encoded_bytes": self.0.as_os_str().as_encoded_bytes()}),
        )
    }
}
macro_rules! name {
    ($($name:ident),* $(,)?) => {$(
        impl Project for manifest::$name {
            fn project(&self) -> Value { tagged(stringify!($name), self.as_ref().project()) }
        }
    )*};
}
name!(
    PackageName,
    RegistryName,
    PathBaseName,
    ProfileName,
    FeatureName
);

impl Project for semver::Version {
    fn project(&self) -> Value {
        let Self {
            major,
            minor,
            patch,
            pre,
            build,
        } = self;
        json!({"type": "Version", "major": major, "minor": minor, "patch": patch,
            "pre": pre.as_str(), "build": build.as_str()})
    }
}
impl Project for semver::Prerelease {
    fn project(&self) -> Value {
        self.as_str().project()
    }
}
impl Project for semver::BuildMetadata {
    fn project(&self) -> Value {
        self.as_str().project()
    }
}
impl Project for core::PartialVersion {
    fn project(&self) -> Value {
        let Self {
            major,
            minor,
            patch,
            pre,
            build,
        } = self;
        json!({"type": "PartialVersion", "major": major, "minor": minor.project(),
            "patch": patch.project(), "pre": pre.project(), "build": build.project()})
    }
}
impl Project for manifest::RustVersion {
    fn project(&self) -> Value {
        tagged("RustVersion", self.to_partial().project())
    }
}
impl Project for url::Url {
    fn project(&self) -> Value {
        tagged("Url", self.as_str().project())
    }
}
impl Project for core::PackageIdSpec {
    fn project(&self) -> Value {
        json!({"type": "PackageIdSpec", "name": self.name(),
            "version": self.partial_version().project(), "url": self.url().project(),
            "kind": self.kind().project()})
    }
}
impl Project for core::SourceKind {
    fn project(&self) -> Value {
        match self {
            Self::Git(value) => tagged("SourceKind::Git", value.project()),
            Self::Path => tagged("SourceKind::Path", Value::Null),
            Self::Registry => tagged("SourceKind::Registry", Value::Null),
            Self::SparseRegistry => tagged("SourceKind::SparseRegistry", Value::Null),
            Self::LocalRegistry => tagged("SourceKind::LocalRegistry", Value::Null),
            Self::Directory => tagged("SourceKind::Directory", Value::Null),
        }
    }
}
impl Project for core::GitReference {
    fn project(&self) -> Value {
        match self {
            Self::Tag(value) => tagged("GitReference::Tag", value.project()),
            Self::Branch(value) => tagged("GitReference::Branch", value.project()),
            Self::Rev(value) => tagged("GitReference::Rev", value.project()),
            Self::DefaultBranch => tagged("GitReference::DefaultBranch", Value::Null),
        }
    }
}
impl Project for manifest::ProfilePackageSpec {
    fn project(&self) -> Value {
        match self {
            Self::Spec(value) => tagged("ProfilePackageSpec::Spec", value.project()),
            Self::All => tagged("ProfilePackageSpec::All", Value::Null),
        }
    }
}
impl Project for toml::Table {
    fn project(&self) -> Value {
        tagged(
            "TomlTable",
            Value::Array(
                self.iter()
                    .map(|(key, value)| json!([key, value.project()]))
                    .collect(),
            ),
        )
    }
}
impl Project for toml::Value {
    fn project(&self) -> Value {
        match self {
            Self::String(value) => tagged("TomlValue::String", value.project()),
            Self::Integer(value) => tagged("TomlValue::Integer", value.project()),
            Self::Float(value) => tagged(
                "TomlValue::Float",
                json!({
                    "bits": format!("{:016x}", value.to_bits()),
                    "class": if value.is_nan() { "nan" } else if value.is_infinite() {
                        "infinite"
                    } else { "finite" },
                    "negative": value.is_sign_negative()
                }),
            ),
            Self::Boolean(value) => tagged("TomlValue::Boolean", value.project()),
            Self::Datetime(value) => tagged("TomlValue::Datetime", value.project()),
            Self::Array(value) => tagged("TomlValue::Array", value.project()),
            Self::Table(value) => tagged("TomlValue::Table", value.project()),
        }
    }
}
impl Project for toml_datetime::Datetime {
    fn project(&self) -> Value {
        json!({"text": self.to_string(),
        "date": self.date.map(|v| json!({"year":v.year,"month":v.month,"day":v.day})),
        "time": self.time.map(|v| json!({"hour":v.hour,"minute":v.minute,
            "second":v.second,"nanosecond":v.nanosecond})),
        "offset": self.offset.map(|v| match v {
            toml_datetime::Offset::Z => tagged("Offset::Z", Value::Null),
            toml_datetime::Offset::Custom { minutes } => tagged("Offset::Custom", json!(minutes)),
        })})
    }
}
