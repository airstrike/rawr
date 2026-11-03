//! Which toolchain a project's import reads crate APIs with, found as rustup
//! finds the one `cargo` runs on in the project's directory.
//!
//! In rustup's order: `RUSTUP_TOOLCHAIN`, which `cargo +toolchain` sets for
//! the commands it runs (rustup marks one it chose by default with
//! `RUSTUP_TOOLCHAIN_SOURCE=default`, and that one names nothing); then,
//! from the project's directory up, a `rustup override set` directory and a
//! `rust-toolchain` or `rust-toolchain.toml` file. A project that names none
//! reads with the pinned nightly, not rustup's default.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// A toolchain name as rustup takes it after `+`, without the host triple:
/// `nightly`, `nightly-2026-07-11`, or a linked toolchain's name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Selection {
    name: Box<str>,
    source: Source,
}

/// What named the toolchain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Source {
    /// `RUSTUP_TOOLCHAIN`, from `cargo +toolchain` or the environment.
    Environment,
    /// A `rustup override set` for this directory.
    Override(PathBuf),
    /// A `rust-toolchain` or `rust-toolchain.toml` file.
    File(PathBuf),
    /// Nothing: the pinned nightly.
    Pinned,
}

impl Selection {
    /// The toolchain for the project in `project`, reading the environment
    /// through `variable` and rustup's settings in `rustup_home`.
    pub(crate) fn resolve<'a>(
        project: &Path,
        variable: impl Fn(&str) -> Option<&'a str>,
        rustup_home: &Path,
        host: &str,
    ) -> Result<Self> {
        if let Some(name) = variable("RUSTUP_TOOLCHAIN").filter(|name| !name.is_empty())
            && variable("RUSTUP_TOOLCHAIN_SOURCE") != Some("default")
        {
            return Ok(Self::named(name, host, Source::Environment));
        }
        let overrides = Overrides::read(rustup_home)?;
        for directory in project.ancestors() {
            if let Some(name) = overrides.get(directory) {
                let source = Source::Override(directory.to_path_buf());
                return Ok(Self::named(name, host, source));
            }
            for file in ["rust-toolchain", "rust-toolchain.toml"] {
                let path = directory.join(file);
                if let Some(name) = file_channel(&path)? {
                    return Ok(Self::named(&name, host, Source::File(path)));
                }
            }
        }
        Ok(Self {
            name: super::CHANNEL.into(),
            source: Source::Pinned,
        })
    }

    fn named(name: &str, host: &str, source: Source) -> Self {
        let name = name
            .strip_suffix(host)
            .and_then(|name| name.strip_suffix('-'))
            .unwrap_or(name);
        Self {
            name: name.into(),
            source,
        }
    }

    /// The pinned nightly, which a project that names no toolchain reads
    /// with.
    #[cfg(test)]
    pub(crate) fn pinned() -> Self {
        Self {
            name: super::CHANNEL.into(),
            source: Source::Pinned,
        }
    }

    /// The name rustup takes after `+` and `--toolchain`.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// What named it.
    pub(crate) fn source(&self) -> &Source {
        &self.source
    }

    /// The toolchain's directory under rustup's home: its own name for a
    /// linked toolchain, else the name with the host triple.
    pub(crate) fn root(&self, rustup_home: &Path, host: &str) -> PathBuf {
        let toolchains = rustup_home.join("toolchains");
        let linked = toolchains.join(self.name.as_ref());
        if linked.exists() {
            linked
        } else {
            toolchains.join(format!("{}-{host}", self.name))
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Environment => f.write_str("`RUSTUP_TOOLCHAIN`"),
            Self::Override(directory) => {
                write!(f, "the rustup override for `{}`", directory.display())
            }
            Self::File(path) => write!(f, "`{}`", path.display()),
            Self::Pinned => f.write_str("rawr-import's pinned nightly"),
        }
    }
}

/// `rustup override set` directories, from rustup's `settings.toml`.
struct Overrides {
    entries: Vec<(PathBuf, Box<str>)>,
}

impl Overrides {
    fn read(rustup_home: &Path) -> Result<Self> {
        let path = rustup_home.join("settings.toml");
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(Error::Io {
                    operation: "read rustup's settings",
                    path,
                    source,
                });
            }
        };
        let document = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| invalid(&path, &error.to_string()))?;
        let entries = document
            .get("overrides")
            .and_then(toml_edit::Item::as_table_like)
            .map(|table| {
                table
                    .iter()
                    .filter_map(|(directory, name)| {
                        Some((PathBuf::from(directory), Box::from(name.as_str()?)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self { entries })
    }

    fn get(&self, directory: &Path) -> Option<&str> {
        self.entries
            .iter()
            .find_map(|(path, name)| (path == directory).then_some(name.as_ref()))
    }
}

/// The channel a toolchain file names, or `None` when there is no file. A
/// `rust-toolchain` file may hold the bare name; either file may be TOML
/// with `[toolchain] channel`. One that names a `path` instead is refused,
/// since rawr-import installs and identifies a toolchain by its channel.
fn file_channel(path: &Path) -> Result<Option<String>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::IsADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(source) => {
            return Err(Error::Io {
                operation: "read the project's toolchain file",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let trimmed = text.trim();
    let bare = !trimmed.contains(['\n', '=', '[']);
    if path.extension().is_none() && bare && !trimmed.is_empty() {
        return Ok(Some(trimmed.to_owned()));
    }
    let document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| invalid(path, &error.to_string()))?;
    let toolchain = document
        .get("toolchain")
        .and_then(toml_edit::Item::as_table_like);
    if let Some(channel) = toolchain
        .and_then(|table| table.get("channel"))
        .and_then(toml_edit::Item::as_str)
    {
        return Ok(Some(channel.to_owned()));
    }
    if toolchain.is_some_and(|table| table.contains_key("path")) {
        return Err(Error::Request {
            reason: format!(
                "`{}` names its toolchain by `path`; rawr-import reads crate APIs with a nightly \
                 named by `channel`, such as `channel = \"{}\"`",
                path.display(),
                super::CHANNEL
            )
            .into_boxed_str(),
        });
    }
    Err(invalid(path, "it names no `[toolchain] channel`"))
}

fn invalid(path: &Path, reason: &str) -> Error {
    Error::Request {
        reason: format!(
            "could not read the toolchain in `{}`: {reason}",
            path.display()
        )
        .into_boxed_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Selection, Source};
    use std::fs;
    use std::path::{Path, PathBuf};

    const HOST: &str = "aarch64-apple-darwin";

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rawr-toolchain-selection-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("rustup")).unwrap();
        fs::create_dir_all(root.join("workspace/project/src")).unwrap();
        root
    }

    fn resolve(root: &Path, environment: &[(&str, &str)]) -> Selection {
        let variable = |name: &str| {
            environment
                .iter()
                .find_map(|(key, value)| (*key == name).then_some(*value))
        };
        Selection::resolve(
            &root.join("workspace/project"),
            variable,
            &root.join("rustup"),
            HOST,
        )
        .unwrap()
    }

    #[test]
    fn a_project_that_names_nothing_reads_with_the_pinned_nightly() {
        let root = scratch("nothing");
        assert_eq!(resolve(&root, &[]), Selection::pinned());
        // rustup's own default, as `cargo` passes it on, names nothing.
        assert_eq!(
            resolve(
                &root,
                &[
                    ("RUSTUP_TOOLCHAIN", "stable-aarch64-apple-darwin"),
                    ("RUSTUP_TOOLCHAIN_SOURCE", "default"),
                ],
            ),
            Selection::pinned()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_toolchain_file_above_the_project_names_it() {
        let root = scratch("file");
        let file = root.join("workspace/rust-toolchain.toml");
        fs::write(
            &file,
            "[toolchain]\nchannel = \"nightly-2026-11-02\"\ncomponents = [\"rustfmt\"]\n",
        )
        .unwrap();
        let selection = resolve(&root, &[]);
        assert_eq!(selection.name(), "nightly-2026-11-02");
        assert_eq!(selection.source(), &Source::File(file));
        // A nearer legacy file wins, as in rustup.
        fs::write(root.join("workspace/project/rust-toolchain"), "nightly\n").unwrap();
        assert_eq!(resolve(&root, &[]).name(), "nightly");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_environment_and_a_plus_override_win_over_files() {
        let root = scratch("environment");
        fs::write(
            root.join("workspace/project/rust-toolchain.toml"),
            "[toolchain]\nchannel = \"nightly-2026-07-11\"\n",
        )
        .unwrap();
        // `cargo +nightly` passes the full name, host and all.
        let selection = resolve(
            &root,
            &[
                ("RUSTUP_TOOLCHAIN", "nightly-aarch64-apple-darwin"),
                ("RUSTUP_TOOLCHAIN_SOURCE", "cli"),
            ],
        );
        assert_eq!(selection.name(), "nightly");
        assert_eq!(selection.source(), &Source::Environment);
        assert_eq!(
            resolve(&root, &[("RUSTUP_TOOLCHAIN", "nightly-2026-08-01")]).name(),
            "nightly-2026-08-01"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_rustup_override_names_its_directory_and_those_below() {
        let root = scratch("override");
        let workspace = root.join("workspace");
        let mut settings = toml_edit::DocumentMut::new();
        settings["overrides"] = toml_edit::table();
        settings["overrides"][workspace.to_str().unwrap()] = toml_edit::value("nightly-2026-09-01");
        fs::write(root.join("rustup/settings.toml"), settings.to_string()).unwrap();
        let selection = resolve(&root, &[]);
        assert_eq!(selection.name(), "nightly-2026-09-01");
        assert_eq!(selection.source(), &Source::Override(workspace));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_path_toolchain_is_refused_with_the_channel_to_name() {
        let root = scratch("path");
        fs::write(
            root.join("workspace/project/rust-toolchain.toml"),
            "[toolchain]\npath = \"/opt/rust\"\n",
        )
        .unwrap();
        let error = Selection::resolve(
            &root.join("workspace/project"),
            |_| None,
            &root.join("rustup"),
            HOST,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("`channel`"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_linked_toolchain_is_found_by_its_own_name() {
        let root = scratch("linked");
        let rustup = root.join("rustup");
        fs::create_dir_all(rustup.join("toolchains/dev")).unwrap();
        let named = |name: &str| Selection::named(name, HOST, Source::Environment);
        assert_eq!(
            named("dev").root(&rustup, HOST),
            rustup.join("toolchains/dev")
        );
        assert_eq!(
            named("nightly-aarch64-apple-darwin").root(&rustup, HOST),
            rustup.join("toolchains/nightly-aarch64-apple-darwin")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
