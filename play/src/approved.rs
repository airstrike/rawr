//! The crates a snippet's frontmatter may name: the dependencies of the
//! approved package (`play/approved/Cargo.toml`), at the versions its
//! lockfile holds. The bot's image vendors them, with what they depend on,
//! and builds every snippet offline against that copy, so nothing a
//! snippet names is downloaded and no build script runs that isn't theirs.
//!
//! A dependency is approved when it names an approved crate's package from
//! that crate's own source: crates.io, with a version requirement the
//! locked version meets, or the same git repository at the same rev. A
//! frontmatter block also holds no table that changes how the bot builds,
//! such as `[profile]` or `[patch]`. The tables cargo refuses in a script
//! (`[lib]`, `[workspace]`, ..) are left to the build, which refuses them
//! with cargo's message before it builds anything.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, TableLike};

/// The variable naming the approved package's directory, which holds its
/// `Cargo.toml` and `Cargo.lock`. A bot without one runs no snippet that
/// has a frontmatter block.
pub const DIRECTORY: &str = "PLAY_APPROVED";

/// The dependency tables of a manifest, and of each of its targets.
const DEPENDENCIES: [&str; 5] = [
    "dependencies",
    "dev-dependencies",
    "dev_dependencies",
    "build-dependencies",
    "build_dependencies",
];

/// The other tables a frontmatter block may hold.
const TABLES: [&str; 4] = ["package", "features", "lints", "target"];

/// The tables cargo refuses in a script, which the build refuses with
/// cargo's message.
const CARGO_REFUSES: [&str; 6] = ["workspace", "lib", "bin", "example", "test", "bench"];

/// A dependency's keys that name a source other than an approved crate's
/// own: a local directory, another registry, or a git branch or tag.
const SOURCES: [&str; 7] = [
    "path",
    "registry",
    "registry-index",
    "branch",
    "tag",
    "workspace",
    "base",
];

/// The approved crates, as their lockfile holds them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Approved {
    crates: Vec<Crate>,
}

/// One approved crate.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Crate {
    /// Its package's name.
    name: String,
    /// The version its lockfile holds.
    version: semver::Version,
    source: Source,
    /// Its dependency line, as the approved manifest declares it, with its
    /// features: `name = { .. }`.
    declaration: String,
}

/// Where an approved crate comes from.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Source {
    /// crates.io.
    Registry,
    /// A git repository, at the rev the approved manifest names.
    Git { url: String, rev: Option<String> },
}

/// Why the approved set could not be read.
#[derive(Debug)]
pub enum Error {
    /// A file of the approved package could not be read.
    Read { path: PathBuf, source: io::Error },
    /// A file of the approved package is not TOML.
    Parse { path: PathBuf, message: String },
    /// An approved dependency the lockfile doesn't lock.
    Unlocked { name: String },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Parse { path, message } => write!(formatter, "{}: {message}", path.display()),
            Self::Unlocked { name } => {
                write!(
                    formatter,
                    "the approved crates' lockfile doesn't lock `{name}`"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

/// What a frontmatter block names that the bot doesn't build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// A crate outside the approved set, or one from another source than
    /// its own. It holds the approved crates, as the reply names them.
    Crate { approved: Vec<String> },
    /// A table that changes how the bot builds.
    Table { name: String },
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Crate { approved } => match approved.split_last() {
                Some((last, [])) => write!(formatter, "?play builds only this crate: {last}."),
                Some((last, rest)) => write!(
                    formatter,
                    "?play builds only these crates: {} and {last}.",
                    rest.join(", ")
                ),
                None => formatter.write_str("?play builds no crates on this server."),
            },
            Self::Table { name } => write!(
                formatter,
                "?play doesn't build with a frontmatter's `[{name}]`. A frontmatter may hold \
                 `[package]`, `[features]`, `[lints]` and dependency tables."
            ),
        }
    }
}

impl Approved {
    /// The approved set `DIRECTORY` names, if the environment names one.
    pub fn from_env() -> Option<Result<Self, Error>> {
        let directory = PathBuf::from(std::env::var_os(DIRECTORY)?);
        Some(Self::read(&directory))
    }

    /// The approved package in `directory`: each of its dependencies, at
    /// the version its lockfile holds.
    pub fn read(directory: &Path) -> Result<Self, Error> {
        let manifest = document(&directory.join("Cargo.toml"))?;
        let lock = document(&directory.join("Cargo.lock"))?;
        let packages = lock
            .get("package")
            .and_then(Item::as_array_of_tables)
            .map(|packages| packages.iter().collect::<Vec<_>>())
            .unwrap_or_default();
        let field = |package: &toml_edit::Table, key: &str| {
            package.get(key).and_then(Item::as_str).map(str::to_owned)
        };
        let own = manifest
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(Item::as_str);
        // The approved package's own entry: the one of its name with no source.
        let root = packages
            .iter()
            .find(|package| {
                field(package, "name").as_deref() == own && package.get("source").is_none()
            })
            .copied();
        // Its locked dependencies, each `name`, or `name version` where the
        // lock holds two versions of one name.
        let locked = root
            .and_then(|root| root.get("dependencies"))
            .and_then(Item::as_array)
            .map(|dependencies| {
                dependencies
                    .iter()
                    .filter_map(|dependency| dependency.as_str())
                    .map(|dependency| {
                        let mut words = dependency.split(' ');
                        let name = words.next().unwrap_or_default().to_owned();
                        (name, words.next().map(str::to_owned))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let declared = manifest
            .get("dependencies")
            .and_then(Item::as_table_like)
            .map(|table| {
                table
                    .iter()
                    .map(|(key, item)| {
                        let name = item
                            .get("package")
                            .and_then(Item::as_str)
                            .unwrap_or(key)
                            .to_owned();
                        // A `[dependencies.name]` table, as one line.
                        let value = item.clone().into_value().map_or_else(
                            |item| item.to_string(),
                            |value| value.decorated("", "").to_string(),
                        );
                        (name, format!("{key} = {}", value.trim()))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let crates = declared
            .into_iter()
            .map(|(name, declaration)| {
                let unlocked = || Error::Unlocked { name: name.clone() };
                let version = locked
                    .iter()
                    .find(|(locked, _)| *locked == name)
                    .ok_or_else(unlocked)?
                    .1
                    .clone();
                let package = packages
                    .iter()
                    .find(|package| {
                        field(package, "name").as_deref() == Some(name.as_str())
                            && version.as_ref().is_none_or(|version| {
                                field(package, "version").as_ref() == Some(version)
                            })
                    })
                    .ok_or_else(unlocked)?;
                let version = field(package, "version")
                    .and_then(|version| semver::Version::parse(&version).ok())
                    .ok_or_else(unlocked)?;
                let source = field(package, "source")
                    .as_deref()
                    .and_then(Source::locked)
                    .ok_or_else(unlocked)?;
                Ok(Crate {
                    name,
                    version,
                    source,
                    declaration,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self { crates })
    }

    /// Each approved crate's dependency line, as the approved manifest
    /// declares it: what `rawr-play warm` builds each of them with.
    pub fn declarations(&self) -> impl Iterator<Item = &str> {
        self.crates
            .iter()
            .map(|approved| approved.declaration.as_str())
    }

    /// What `manifest`, a frontmatter block's, names that the bot doesn't
    /// build, if it names anything.
    #[must_use]
    pub fn refusal(&self, manifest: &DocumentMut) -> Option<Refusal> {
        let table = manifest.as_table();
        if let Some((name, _)) = table.iter().find(|(name, _)| {
            !TABLES.contains(name) && !CARGO_REFUSES.contains(name) && !DEPENDENCIES.contains(name)
        }) {
            return Some(Refusal::Table {
                name: name.to_owned(),
            });
        }
        let targets = table
            .get("target")
            .and_then(Item::as_table_like)
            .into_iter()
            .flat_map(TableLike::iter)
            .filter_map(|(_, target)| target.as_table_like());
        let mut tables = vec![table as &dyn TableLike];
        for target in targets {
            if let Some((name, _)) = target.iter().find(|(name, _)| !DEPENDENCIES.contains(name)) {
                return Some(Refusal::Table {
                    name: format!("target.<target>.{name}"),
                });
            }
            tables.push(target);
        }
        let admitted = tables
            .into_iter()
            .flat_map(|table| DEPENDENCIES.iter().filter_map(|key| table.get(key)))
            .all(|dependencies| {
                dependencies.as_table_like().is_some_and(|dependencies| {
                    dependencies
                        .iter()
                        .all(|(name, dependency)| self.admits(name, dependency))
                })
            });
        (!admitted).then(|| Refusal::Crate {
            approved: self.crates.iter().map(Crate::named).collect(),
        })
    }

    /// Whether the dependency `name = dependency` names an approved crate
    /// from its own source.
    fn admits(&self, name: &str, dependency: &Item) -> bool {
        let table = dependency.as_table_like();
        if dependency.as_str().is_none() && table.is_none() {
            return false;
        }
        let field = |key: &str| table.and_then(|table| table.get(key));
        if SOURCES.iter().any(|key| field(key).is_some()) {
            return false;
        }
        let package = match field("package") {
            Some(package) => package.as_str(),
            None => Some(name),
        };
        let Some(approved) = package
            .and_then(|package| self.crates.iter().find(|approved| approved.name == package))
        else {
            return false;
        };
        let from_its_source = match (&approved.source, field("git")) {
            (Source::Registry, None) => field("rev").is_none(),
            (Source::Git { url, rev }, Some(git)) => {
                git.as_str() == Some(url.as_str())
                    && field("rev").and_then(Item::as_str) == rev.as_deref()
            }
            _ => false,
        };
        let requirement = dependency
            .as_str()
            .map(Some)
            .or_else(|| field("version").map(Item::as_str));
        let versioned = match requirement {
            None => true,
            Some(None) => false,
            Some(Some(requirement)) => semver::VersionReq::parse(requirement)
                .is_ok_and(|requirement| requirement.matches(&approved.version)),
        };
        from_its_source && versioned
    }
}

impl Crate {
    /// The crate as a refusal names it: its name and version, and the rev
    /// of a git crate.
    fn named(&self) -> String {
        match &self.source {
            Source::Registry => format!("{} {}", self.name, self.version),
            Source::Git { rev, .. } => {
                let rev = rev.as_deref().unwrap_or_default();
                format!(
                    "{} {} (git {})",
                    self.name,
                    self.version,
                    rev.get(..7).unwrap_or(rev)
                )
            }
        }
    }
}

impl Source {
    /// The source a lockfile's `source` names: a registry, or
    /// `git+<url>?rev=<rev>#<commit>`.
    fn locked(source: &str) -> Option<Self> {
        if source.starts_with("registry+") || source.starts_with("sparse+") {
            return Some(Self::Registry);
        }
        let git = source.strip_prefix("git+")?;
        let git = git.split_once('#').map_or(git, |(git, _)| git);
        let (url, query) = git.split_once('?').unwrap_or((git, ""));
        let rev = query
            .split('&')
            .find_map(|pair| pair.strip_prefix("rev="))
            .map(str::to_owned);
        Some(Self::Git {
            url: url.to_owned(),
            rev,
        })
    }
}

/// The TOML document at `path`.
fn document(path: &Path) -> Result<DocumentMut, Error> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_owned(),
        source,
    })?;
    text.parse::<DocumentMut>().map_err(|error| Error::Parse {
        path: path.to_owned(),
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{Approved, Refusal};
    use std::path::PathBuf;

    fn approved() -> Approved {
        Approved::read(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("approved")).unwrap()
    }

    fn refusal(manifest: &str) -> Option<Refusal> {
        approved().refusal(&manifest.parse().unwrap())
    }

    /// The approved package's lockfile gives each crate its version, and
    /// a git crate its rev.
    #[test]
    fn the_approved_crates_are_the_approved_package_s_dependencies() {
        let Some(Refusal::Crate { approved }) = refusal("[dependencies]\nserde = \"1\"\n") else {
            panic!("serde is not approved");
        };
        let names = approved
            .iter()
            .map(|named| named.split(' ').next().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, ["iced", "iced_test", "libc", "ratatui", "tokio"]);
        assert!(approved[0].ends_with("(git be88873)"), "{approved:?}");
    }

    #[test]
    fn a_dependency_on_an_approved_crate_at_its_version_is_admitted() {
        for manifest in [
            "[dependencies]\nlibc = \"0.2.172\"\n",
            "[dependencies]\nlibc = { version = \"0.2\", default-features = false }\n",
            "[dependencies]\nc = { package = \"libc\", version = \"0.2\" }\n",
            "[package]\nedition = \"2021\"\n[dependencies]\ntokio = { version = \"1\", features = [\"rt\"] }\n",
            "[target.'cfg(unix)'.dependencies]\nlibc = \"*\"\n",
            "[dependencies]\niced = { git = \"https://github.com/iced-rs/iced\", rev = \"be88873d38544455e26c7a104eb757da2cbbac6b\" }\n",
            "[package]\nname = \"demo\"\n",
        ] {
            assert_eq!(refusal(manifest), None, "{manifest}");
        }
    }

    /// Another crate, another version or another source is refused, as is
    /// a table that changes how the bot builds.
    #[test]
    fn anything_else_a_frontmatter_names_is_refused() {
        for manifest in [
            "[dependencies]\nserde = \"1\"\n",
            "[dependencies]\nlibc = \"0.1\"\n",
            "[dependencies]\nlibc = \"zero\"\n",
            "[dependencies]\nlibc = { path = \"/src/libc\" }\n",
            "[dependencies]\nlibc = { git = \"https://example.com/libc\" }\n",
            "[dependencies]\nevil = { package = \"evil\", version = \"1\" }\n",
            "[dependencies]\niced = \"0.13\"\n",
            "[dependencies]\niced = { git = \"https://github.com/iced-rs/iced\", branch = \"master\" }\n",
            "[dev-dependencies]\nserde = \"1\"\n",
            "[target.'cfg(unix)'.dependencies]\nserde = \"1\"\n",
        ] {
            assert!(
                matches!(refusal(manifest), Some(Refusal::Crate { .. })),
                "{manifest}"
            );
        }
        for (manifest, table) in [
            ("[profile.dev]\nopt-level = 3\n", "profile"),
            ("[patch.crates-io]\nlibc = { path = \"/x\" }\n", "patch"),
            ("[replace]\n\"libc:0.2.0\" = { path = \"/x\" }\n", "replace"),
            (
                "cargo-features = [\"profile-rustflags\"]\n",
                "cargo-features",
            ),
        ] {
            assert_eq!(
                refusal(manifest),
                Some(Refusal::Table {
                    name: table.to_owned()
                }),
                "{manifest}"
            );
        }
    }

    #[test]
    fn a_refusal_is_one_plain_line() {
        let refused = Refusal::Crate {
            approved: vec![
                "a 1.0.0".to_owned(),
                "b 2.0.0".to_owned(),
                "c 3.0.0".to_owned(),
            ],
        };
        assert_eq!(
            refused.to_string(),
            "?play builds only these crates: a 1.0.0, b 2.0.0 and c 3.0.0."
        );
        assert!(!refused.to_string().contains('\n'));
    }
}
