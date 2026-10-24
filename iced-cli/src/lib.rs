//! Command parsing and execution for the Rawr developer CLI.

#![deny(unsafe_code)]
// The public command error preserves its structured subcommand diagnostics.
// Boxing variants would change the CLI library's exhaustively matched API.
#![allow(clippy::result_large_err)]

mod base;
mod check;
pub mod command;
pub mod compile;
pub use rawr_cli::diagnostic;
pub mod doctor;
#[cfg(feature = "native-runner")]
mod import;
pub mod init;
pub mod layer;
mod mcp;
pub mod new;
pub mod preview;
pub mod run;
pub mod snapshot;
pub mod watch;

use std::fmt;
use std::path::Path;

/// Failure while executing a parsed CLI command.
#[derive(Debug)]
pub enum Error {
    /// Project discovery or application checking failed.
    Compile(compile::Error),
    /// Native application preparation or execution failed.
    Run(run::Error),
    /// Preview Catalog preparation or native execution failed.
    Preview(preview::Error),
    /// Deterministic snapshot preparation or storage failed.
    Snapshot(snapshot::Error),
    /// Conservative project creation failed.
    New(new::Error),
    /// `rawr cache clean` could not read or evict the store.
    Clean(String),
    /// `rawr mcp` lost its stdio.
    Mcp(std::io::Error),
    /// `rawr init` could not read or write an agent config.
    Init(init::Error),
}

/// Successful output from a non-interactive command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    message: String,
    warning: Option<String>,
    successful: bool,
}

impl Report {
    /// Return the concise human-readable command result.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return a prominent warning that must be written to stderr.
    #[must_use]
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    /// Return whether this completed command should exit successfully.
    #[must_use]
    pub const fn is_successful(&self) -> bool {
        self.successful
    }
}

/// Execute one already-parsed command.
pub fn execute(command: command::Command) -> Result<Option<Report>, Error> {
    // Exceedances belong to the command that produced them. They stay in the
    // import registry; the CLI does not print them.
    #[cfg(feature = "native-runner")]
    drop(rawr_import::exceedance::drain());
    let layer = |path: Option<&std::path::PathBuf>| {
        Ok(match base::layer(path)? {
            base::Program::Base(loaded) => Err(loaded),
            base::Program::Layered(loaded) => Ok(loaded),
        })
    };
    match command {
        command::Command::Run { path } => {
            let loaded = match layer(path.as_ref()).map_err(|report| {
                Error::Run(run::Error::Compile(compile::Error::from_report(report)))
            })? {
                Ok(loaded) => loaded,
                Err(base) => {
                    let cargo = rawr_cli::cargo::begin(&base.graph);
                    let failed = base::run(base).map_err(
                        |(base::Failed::Compile(report) | base::Failed::Panic(report))| {
                            Error::Run(run::Error::Application { report })
                        },
                    );
                    rawr_cli::cargo::after_run(cargo);
                    return Ok(failed?.map(|error| Report {
                        message: String::new(),
                        warning: Some(error),
                        successful: false,
                    }));
                }
            };
            let successful = run::execute_loaded(loaded).map_err(Error::Run)?;
            // A program that failed printed its own error as it ran.
            Ok((!successful).then(|| Report {
                message: String::new(),
                warning: None,
                successful: false,
            }))
        }
        command::Command::Preview { path, name } => {
            let path = preview::select(path, name).map_err(Error::Preview)?;
            preview::execute(path).map_err(Error::Preview)?;
            Ok(None)
        }
        command::Command::Snapshot {
            mode,
            verbose,
            path,
        } => {
            let outcome = snapshot::execute(mode, path).map_err(Error::Snapshot)?;
            let successful = outcome.is_successful();
            let message = if outcome.updated() {
                details(
                    format!("Updated {} snapshots.", outcome.artifacts()),
                    &outcome,
                    verbose,
                )
            } else if successful {
                details(
                    format!("{} snapshots match.", outcome.artifacts()),
                    &outcome,
                    verbose,
                )
            } else {
                let mut message = format!(
                    "Snapshot check found {} mismatch(es) across {} artifact(s):\n",
                    outcome.differences().len(),
                    outcome.artifacts(),
                );
                for difference in outcome.differences() {
                    message.push_str(&format!(
                        "\n[{}] {} ({})\n  expected: {}\n  actual:   {}\n  {}\n",
                        difference.class(),
                        difference.identity(),
                        difference.theme(),
                        visible(&outcome, difference.expected(), verbose).display(),
                        visible(&outcome, difference.actual(), verbose).display(),
                        difference.detail(),
                    ));
                }
                if verbose {
                    storage(&mut message, &outcome);
                }
                message
            };
            let warning = (!outcome.notices().is_empty()).then(|| {
                let mut warning = format!(
                    "WARNING: snapshot update committed with {} cleanup warning(s)",
                    outcome.notices().len(),
                );
                if verbose {
                    warning.push(':');
                    for notice in outcome.notices() {
                        warning.push_str(&format!("\n  {notice}"));
                    }
                } else {
                    warning.push_str("; rerun with `--verbose` for recoverable storage details.");
                }
                warning
            });
            Ok(Some(Report {
                message,
                warning,
                successful,
            }))
        }
        command::Command::Check { path } => {
            let program = layer(path.as_ref())
                .map_err(|report| Error::Compile(compile::Error::from_report(report)))?;
            let (Ok(loaded) | Err(loaded)) = &program;
            // rustc checks a Cargo crate of marked files while Rawr works.
            let cargo = rawr_cli::cargo::begin(&loaded.graph);
            let checked = match program {
                Ok(loaded) => check::execute_loaded(loaded),
                Err(base) => base::check(base).map_err(compile::Error::from_report),
            };
            if cargo.is_none() {
                let source = checked.map_err(Error::Compile)?;
                return Ok(Some(Report {
                    message: format!("Checked {}", source.display()),
                    warning: None,
                    successful: true,
                }));
            }
            let successful = rawr_cli::cargo::conclude(
                checked
                    .as_ref()
                    .map(std::path::PathBuf::as_path)
                    .map_err(compile::Error::report),
                cargo,
            );
            Ok(Some(Report {
                message: String::new(),
                warning: None,
                successful,
            }))
        }
        command::Command::New { path } => {
            let created = new::create(path).map_err(Error::New)?;
            Ok(Some(Report {
                message: format!(
                    "Created an iced crate at {root}\n\nNext:\n  cd {root}\n  rawr run",
                    root = created.root().display()
                ),
                warning: None,
                successful: true,
            }))
        }
        command::Command::CacheClean { all } => {
            let freed = clean(all).map_err(Error::Clean)?;
            Ok(Some(Report {
                message: format!("Freed {:.1} GB.", freed as f64 / 1e9),
                warning: None,
                successful: true,
            }))
        }
        command::Command::Mcp => {
            mcp::serve().map_err(Error::Mcp)?;
            Ok(None)
        }
        command::Command::Init {
            claude,
            codex,
            path,
        } => {
            let root = preview::root(path.as_ref()).map_err(Error::Preview)?;
            let mut lines = Vec::new();
            if claude {
                lines.push(init::claude(&root).map_err(Error::Init)?.to_string());
            }
            if codex {
                lines.push(init::codex(&root).map_err(Error::Init)?.to_string());
            }
            Ok(Some(Report {
                message: lines.join("\n"),
                warning: None,
                successful: true,
            }))
        }
        command::Command::Doctor { path } => {
            let report = doctor::Report::examine(path);
            let successful = !report.has_failures();
            Ok(Some(Report {
                message: report.to_string(),
                warning: None,
                successful,
            }))
        }
    }
}

/// Retire other installs' idle Rawr hosts, the only Rawr-wide cache;
/// `all` also removes the current project's `target/rawr/cache` and its
/// `<target>/rawr`, which `cargo clean` removes too. They are the same
/// directory unless the target directory is moved. Returns the bytes freed.
fn clean(all: bool) -> Result<u64, String> {
    let mut freed = 0;
    #[cfg(feature = "native-runner")]
    {
        let home = std::env::var_os("CARGO_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cargo")))
            .ok_or("rawr cache clean requires CARGO_HOME or HOME")?;
        let hosts = rawr_import::store::home(&home).join("hosts");
        let before = directory_bytes(&hosts);
        rawr_import::host::clean(&home);
        freed += before.saturating_sub(directory_bytes(&hosts));
    }
    if all {
        let project = std::env::current_dir().map_err(|error| error.to_string())?;
        let mut owned = vec![project.join("target/rawr/cache")];
        #[cfg(feature = "native-runner")]
        owned.push(rawr_import::location::rawr_directory(
            &project,
            &rawr_import::cargo::config::Environment::capture()
                .map_err(|error| error.to_string())?,
        ));
        for cache in owned {
            freed += directory_bytes(&cache);
            match std::fs::remove_dir_all(&cache) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(format!("remove {}: {error}", cache.display()));
                }
                _ => {}
            }
        }
    }
    Ok(freed)
}

fn directory_bytes(path: &Path) -> u64 {
    std::fs::symlink_metadata(path).map_or(0, |metadata| {
        if metadata.is_dir() {
            std::fs::read_dir(path).map_or(0, |entries| {
                entries
                    .flatten()
                    .map(|entry| directory_bytes(&entry.path()))
                    .sum()
            })
        } else {
            metadata.len()
        }
    })
}

fn details(mut message: String, outcome: &rawr_iced::snapshot::Outcome, verbose: bool) -> String {
    if verbose {
        storage(&mut message, outcome);
    }
    message
}

fn storage(message: &mut String, outcome: &rawr_iced::snapshot::Outcome) {
    message.push_str(&format!(
        "\nBaseline: {}\nRendered: {}",
        outcome.baseline().display(),
        outcome.actual().display(),
    ));
}

fn visible<'a>(
    outcome: &'a rawr_iced::snapshot::Outcome,
    path: &'a Path,
    verbose: bool,
) -> &'a Path {
    if verbose {
        return path;
    }
    outcome
        .baseline()
        .parent()
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path)
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.report().fmt(formatter)
    }
}

impl Error {
    /// Transform every command failure into the canonical shared report.
    #[must_use]
    pub fn report(&self) -> rawr::diagnostic::Report {
        match self {
            Self::Compile(error) => error.report().clone(),
            Self::Run(error) => error.report().clone(),
            Self::Preview(error) => error.report().clone(),
            Self::Snapshot(error) => error.report().clone(),
            Self::New(error) => rawr::diagnostic::Report::error(
                rawr::diagnostic::code::ENVIRONMENT_CREATE,
                error.to_string(),
            )
            .help("choose an absent or verified empty target directory")
            .finish(),
            Self::Clean(error) => rawr::diagnostic::Report::error(
                rawr::diagnostic::code::ENVIRONMENT_UNREADABLE,
                error.clone(),
            )
            .finish(),
            Self::Mcp(error) => rawr::diagnostic::Report::error(
                rawr::diagnostic::code::ENVIRONMENT_UNREADABLE,
                format!("rawr mcp: {error}"),
            )
            .finish(),
            Self::Init(error) => rawr::diagnostic::Report::error(
                rawr::diagnostic::code::ENVIRONMENT_UNREADABLE,
                format!("rawr init: {error}"),
            )
            .finish(),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Compile(error) => Some(error),
            Self::Run(error) => Some(error),
            Self::Preview(error) => Some(error),
            Self::Snapshot(error) => Some(error),
            Self::New(error) => Some(error),
            Self::Clean(_) => None,
            Self::Mcp(error) => Some(error),
            Self::Init(error) => Some(error),
        }
    }
}

/// Writes `relative` in the Cargo crate at `root`, marked `// rawr` unless
/// `source` already is, and the crate's `Cargo.toml` when it has none.
#[cfg(test)]
pub(crate) fn write_marked(root: &std::path::Path, relative: &str, source: impl AsRef<str>) {
    let manifest = root.join("Cargo.toml");
    if !manifest.exists() {
        std::fs::write(
            manifest,
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
    }
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let source = source.as_ref();
    if source.starts_with("// rawr\n") {
        std::fs::write(path, source).unwrap();
    } else {
        std::fs::write(path, format!("// rawr\n{source}")).unwrap();
    }
}
