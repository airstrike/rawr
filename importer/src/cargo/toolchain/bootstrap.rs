//! The selected nightly, installed or completed through rustup the first
//! time an import needs it.
//!
//! rustup records each toolchain's installed components in
//! `lib/rustlib/components`, one per line, with the host triple after every
//! component built per host (`rustc-dev-aarch64-apple-darwin`) and bare for
//! the others (`rust-src`). Reading that file decides whether anything is
//! missing without running rustup, so a machine that has the toolchain pays
//! one small read per process.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use super::{Selection, HOST, REQUIRED_COMPONENTS};
use crate::{Error, Result};

/// The selected toolchain's root under rustup's home, with every required
/// component installed.
#[derive(Debug)]
pub(crate) struct Installed {
    root: PathBuf,
}

/// What the selected toolchain lacks, read from its components file.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Missing {
    Nothing,
    Toolchain,
    Components(Box<[&'static str]>),
}

/// The toolchain root this process has seen complete, so a later import in
/// the same process reads nothing.
static SEEN: OnceLock<PathBuf> = OnceLock::new();

impl Installed {
    /// The selected toolchain in `rustup_home`, installed or completed with
    /// `rustup` first when it is missing or lacks a required component.
    pub(crate) fn ensure(
        rustup: &Path,
        rustup_home: &Path,
        selection: &Selection,
        environment: &[(OsString, OsString)],
    ) -> Result<Self> {
        let root = selection.root(rustup_home, HOST);
        if SEEN.get() == Some(&root) {
            return Ok(Self { root });
        }
        let components = match fs::read_to_string(root.join("lib/rustlib/components")) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(Error::Io {
                    operation: "read the selected toolchain's components",
                    path: root.join("lib/rustlib/components"),
                    source,
                });
            }
        };
        let missing = Missing::of(components.as_deref(), HOST);
        let channel = selection.name();
        if let Some(arguments) = missing.arguments(channel) {
            crate::progress::say(&missing.message(channel));
            install(rustup, &arguments, environment)?;
        }
        let _ = SEEN.set(root.clone());
        Ok(Self { root })
    }

    /// The toolchain's root directory.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

impl Missing {
    /// What a toolchain whose components file reads `components` lacks.
    /// `None` is a toolchain rustup has not installed.
    fn of(components: Option<&str>, host: &str) -> Self {
        let Some(components) = components else {
            return Self::Toolchain;
        };
        let installed = |required: &str| {
            components.lines().map(str::trim).any(|line| {
                line == required
                    || line
                        .strip_prefix(required)
                        .and_then(|rest| rest.strip_prefix('-'))
                        == Some(host)
            })
        };
        if !installed("rustc") || !installed("cargo") {
            return Self::Toolchain;
        }
        let absent: Box<[&'static str]> = REQUIRED_COMPONENTS
            .into_iter()
            .filter(|required| !installed(required))
            .collect();
        if absent.is_empty() {
            Self::Nothing
        } else {
            Self::Components(absent)
        }
    }

    /// The `rustup` arguments that install what `channel` is missing, if
    /// anything is.
    fn arguments(&self, channel: &str) -> Option<Vec<String>> {
        match self {
            Self::Nothing => None,
            Self::Toolchain => Some(vec![
                "toolchain".into(),
                "install".into(),
                channel.into(),
                "--profile".into(),
                "minimal".into(),
                "--component".into(),
                REQUIRED_COMPONENTS.join(","),
                "--no-self-update".into(),
            ]),
            Self::Components(absent) => {
                let mut arguments = vec![
                    "component".into(),
                    "add".into(),
                    "--toolchain".into(),
                    channel.into(),
                ];
                arguments.extend(absent.iter().map(|component| (*component).to_owned()));
                Some(arguments)
            }
        }
    }
}

impl Missing {
    /// The line printed before rustup runs.
    fn message(&self, channel: &str) -> String {
        match self {
            Self::Nothing => format!("Rust {channel} is installed"),
            Self::Toolchain => format!(
                "Installing Rust {channel} with {} to read crate APIs (once per machine)",
                REQUIRED_COMPONENTS.join(" and ")
            ),
            Self::Components(absent) => format!(
                "Adding {} to Rust {channel} to read crate APIs (once per machine)",
                absent.join(" and ")
            ),
        }
    }
}

/// Run `rustup` with the import's environment, its progress on stderr.
fn install(
    rustup: &Path,
    arguments: &[String],
    environment: &[(OsString, OsString)],
) -> Result<()> {
    let status = Command::new(rustup)
        .args(arguments)
        .env_clear()
        .envs(environment.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|source| Error::Io {
            operation: "run rustup",
            path: rustup.to_path_buf(),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Request {
            reason: format!(
                "`rustup {}` failed ({status}); run it yourself, then run rawr again",
                arguments.join(" ")
            )
            .into_boxed_str(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Missing;

    const HOST: &str = "aarch64-apple-darwin";

    #[test]
    fn an_absent_components_file_installs_the_toolchain() {
        let missing = Missing::of(None, HOST);
        assert_eq!(missing, Missing::Toolchain);
        assert_eq!(
            missing.arguments(super::super::CHANNEL).unwrap(),
            [
                "toolchain",
                "install",
                super::super::CHANNEL,
                "--profile",
                "minimal",
                "--component",
                "rustc-dev,rust-src",
                "--no-self-update",
            ]
        );
    }

    #[test]
    fn a_complete_toolchain_runs_nothing() {
        let components = "cargo-aarch64-apple-darwin\nrustc-aarch64-apple-darwin\n\
            rust-std-aarch64-apple-darwin\nrustc-dev-aarch64-apple-darwin\nrust-src\n\
            clippy-preview-aarch64-apple-darwin\n";
        let missing = Missing::of(Some(components), HOST);
        assert_eq!(missing, Missing::Nothing);
        assert_eq!(missing.arguments(super::super::CHANNEL), None);
    }

    #[test]
    fn a_default_profile_toolchain_adds_only_the_absent_components() {
        let components = "cargo-aarch64-apple-darwin\nrustc-aarch64-apple-darwin\n\
            rust-std-aarch64-apple-darwin\nrust-src\n";
        let missing = Missing::of(Some(components), HOST);
        assert_eq!(missing, Missing::Components(Box::from(["rustc-dev"])));
        assert_eq!(
            missing.arguments(super::super::CHANNEL).unwrap(),
            [
                "component",
                "add",
                "--toolchain",
                super::super::CHANNEL,
                "rustc-dev"
            ]
        );
    }

    #[test]
    fn a_component_for_another_host_does_not_count() {
        let components = "cargo-aarch64-apple-darwin\nrustc-aarch64-apple-darwin\n\
            rustc-dev-x86_64-unknown-linux-gnu\nrust-src\n";
        assert_eq!(
            Missing::of(Some(components), HOST),
            Missing::Components(Box::from(["rustc-dev"]))
        );
    }

    #[test]
    fn a_toolchain_without_rustc_is_installed_again() {
        assert_eq!(Missing::of(Some("rust-src\n"), HOST), Missing::Toolchain);
    }
}
