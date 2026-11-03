//! The nightly a process imports with, identified by its `rustc -vV` and
//! `cargo -vV`.
//!
//! A process imports one project, so it adopts one toolchain: the first
//! selection sets it, and every key and check that names the compiler or
//! Cargo reads it from here. Until a selection, it is the pinned nightly,
//! which tests and fixtures assume.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, OnceLock};

use super::selection::{Selection, Source};
use crate::{Error, Result};

/// One installed nightly's identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Identity {
    name: Box<str>,
    root: Option<PathBuf>,
    rustc: Version,
    commit_date: Box<str>,
    cargo: Version,
}

/// A tool's `release:` and `commit-hash:`.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Version {
    release: Box<str>,
    commit: Box<str>,
}

static PINNED: LazyLock<Identity> = LazyLock::new(|| Identity {
    name: super::CHANNEL.into(),
    root: None,
    rustc: Version {
        release: super::RUST_RELEASE.into(),
        commit: super::RUST_COMMIT.into(),
    },
    commit_date: super::window::OLDEST.into(),
    cargo: Version {
        release: super::CARGO_RELEASE.into(),
        commit: super::CARGO_COMMIT.into(),
    },
});

static ACTIVE: OnceLock<Identity> = OnceLock::new();

/// The toolchain this process imports with: the one it adopted, else the
/// pinned nightly.
pub fn active() -> &'static Identity {
    ACTIVE.get().unwrap_or(&PINNED)
}

impl Identity {
    /// The pinned nightly's identity, the one prebuilt drivers and the
    /// checked-in std table are for.
    pub fn pinned() -> &'static Self {
        &PINNED
    }

    /// Read the identity of `selection`, installed at `root`, with the
    /// import's `environment`. It must be a nightly inside the driver's
    /// window. A root this process already adopted is not read again.
    pub(crate) fn read(
        selection: &Selection,
        root: &Path,
        environment: &[(OsString, OsString)],
    ) -> Result<Self> {
        if let Some(active) = ACTIVE.get()
            && active.root.as_deref() == Some(root)
        {
            return Ok(active.clone());
        }
        let rustc = version_text(&root.join("bin/rustc"), environment)?;
        let cargo = version_text(&root.join("bin/cargo"), environment)?;
        let identity = Self::parse(selection.name(), root, &rustc, &cargo)?;
        identity.supported(selection.source())?;
        Ok(identity)
    }

    fn parse(name: &str, root: &Path, rustc: &str, cargo: &str) -> Result<Self> {
        let value = |text: &str, document: &'static str, prefix: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(prefix))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(Box::from)
                .ok_or_else(|| Error::Schema {
                    document,
                    reason: format!("missing `{}` line", prefix.trim_end()).into_boxed_str(),
                })
        };
        Ok(Self {
            name: name.into(),
            root: Some(root.to_path_buf()),
            rustc: Version {
                release: value(rustc, "rustc -vV", "release: ")?,
                commit: value(rustc, "rustc -vV", "commit-hash: ")?,
            },
            commit_date: value(rustc, "rustc -vV", "commit-date: ")?,
            cargo: Version {
                release: value(cargo, "cargo -vV", "release: ")?,
                commit: value(cargo, "cargo -vV", "commit-hash: ")?,
            },
        })
    }

    /// Refuse a toolchain the driver can't be built on: one that isn't a
    /// nightly, since only a nightly has rustc's private crates, or one
    /// outside the window `window.rs` names.
    fn supported(&self, source: &Source) -> Result<()> {
        let fix = format!(
            "Name a nightly from {} in the project's `rust-toolchain.toml`, or a stable \
             release or none to read with rawr-import's pinned `{}`",
            super::window::RANGE,
            super::CHANNEL
        );
        if !self.rustc.release.ends_with("-nightly") {
            return Err(Error::Request {
                reason: format!(
                    "rawr-import reads crate APIs with rustc's private crates, which only a \
                     nightly toolchain has, and {source} names `{}`, which is Rust {}. {fix}",
                    self.name, self.rustc.release
                )
                .into_boxed_str(),
            });
        }
        if !super::window::contains(&self.commit_date) {
            return Err(Error::Request {
                reason: format!(
                    "rawr-import's driver builds on {}, and {source} names `{}`, a nightly \
                     from {}. {fix}",
                    super::window::RANGE,
                    self.name,
                    self.commit_date
                )
                .into_boxed_str(),
            });
        }
        Ok(())
    }

    /// Make this the process's toolchain. A process imports with one: a
    /// second, different one is refused, and the same one again is kept.
    pub(crate) fn adopt(self) -> Result<&'static Self> {
        let adopted = ACTIVE.get_or_init(|| self.clone());
        if adopted.rustc == self.rustc && adopted.cargo == self.cargo {
            Ok(adopted)
        } else {
            Err(Error::Request {
                reason: format!(
                    "this rawr-import reads crate APIs with `{}` and the project now names \
                     `{}`; run rawr again to use it",
                    adopted.name, self.name
                )
                .into_boxed_str(),
            })
        }
    }

    /// The name rustup takes after `+` and `--toolchain`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where rustup installed it; `None` for the pinned nightly before a
    /// selection found it.
    pub(crate) fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Whether this is the pinned nightly.
    pub fn is_pinned(&self) -> bool {
        self.rustc == PINNED.rustc
    }

    /// rustc's `release:`, as `1.99.0-nightly`.
    pub fn rustc_release(&self) -> &str {
        &self.rustc.release
    }

    /// rustc's full commit hash.
    pub fn rustc_commit(&self) -> &str {
        &self.rustc.commit
    }

    /// rustc's `commit-date:`.
    pub fn commit_date(&self) -> &str {
        &self.commit_date
    }

    /// Cargo's `release:`.
    pub fn cargo_release(&self) -> &str {
        &self.cargo.release
    }

    /// Cargo's full commit hash.
    pub fn cargo_commit(&self) -> &str {
        &self.cargo.commit
    }
}

/// `tool -vV`'s output.
fn version_text(tool: &Path, environment: &[(OsString, OsString)]) -> Result<String> {
    let output = Command::new(tool)
        .arg("-vV")
        .env_clear()
        .envs(environment.iter().cloned())
        .stdin(Stdio::null())
        .output()
        .map_err(|source| Error::Io {
            operation: "read the toolchain's version",
            path: tool.to_path_buf(),
            source,
        })?;
    if !output.status.success() {
        return Err(Error::Process {
            program: "toolchain version",
            status: output.status.to_string().into_boxed_str(),
            stderr: String::from_utf8_lossy(&output.stderr)
                .into_owned()
                .into_boxed_str(),
        });
    }
    String::from_utf8(output.stdout).map_err(|error| Error::Schema {
        document: "tool -vV",
        reason: error.to_string().into_boxed_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::selection::Source;
    use super::Identity;
    use std::path::Path;

    const NEWEST: &str = "rustc 1.101.0-nightly (3e92e1a2d 2026-11-01)\nbinary: rustc\n\
        commit-hash: 3e92e1a2d20b991ef23a6c6064c34d57ee8c98d3\ncommit-date: 2026-11-01\n\
        host: aarch64-apple-darwin\nrelease: 1.101.0-nightly\nLLVM version: 23.1.5\n";
    const CARGO: &str = "cargo 1.101.0-nightly (0123456 2026-10-30)\nrelease: 1.101.0-nightly\n\
        commit-hash: 0123456789abcdef0123456789abcdef01234567\ncommit-date: 2026-10-30\n";

    fn identity(rustc: &str) -> Identity {
        Identity::parse("nightly", Path::new("/toolchains/nightly"), rustc, CARGO).unwrap()
    }

    #[test]
    fn a_nightly_is_identified_by_its_commits() {
        let identity = identity(NEWEST);
        assert_eq!(
            identity.rustc_commit(),
            "3e92e1a2d20b991ef23a6c6064c34d57ee8c98d3"
        );
        assert_eq!(identity.commit_date(), "2026-11-01");
        assert_eq!(
            identity.cargo_commit(),
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert!(!identity.is_pinned());
        assert!(identity.supported(&Source::Environment).is_ok());
        assert!(Identity::pinned().is_pinned());
        assert_eq!(super::active(), Identity::pinned());
    }

    #[test]
    fn a_stable_toolchain_is_refused_as_not_nightly() {
        let stable = NEWEST
            .replace("1.101.0-nightly", "1.95.0")
            .replace("2026-11-01", "2026-05-11");
        let error = identity(&stable)
            .supported(&Source::Environment)
            .unwrap_err()
            .to_string();
        assert!(error.contains("only a nightly toolchain has"), "{error}");
        assert!(error.contains("Rust 1.95.0"), "{error}");
    }

    #[test]
    fn a_nightly_outside_the_window_names_the_range() {
        let old = NEWEST.replace("2026-11-01", "2026-05-01");
        let error = identity(&old)
            .supported(&Source::Environment)
            .unwrap_err()
            .to_string();
        assert!(error.contains(super::super::window::RANGE), "{error}");
        assert!(error.contains("2026-05-01"), "{error}");
    }
}
