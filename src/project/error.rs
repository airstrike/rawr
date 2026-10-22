//! Structured project-discovery failures.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Filesystem resource involved in project discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resource {
    /// The path explicitly supplied by the caller.
    Input,
    /// A Rawr source file.
    Source,
    /// A `rawr.toml` file.
    Config,
    /// A VCS marker used as an ancestor-search boundary.
    Boundary,
}

impl fmt::Display for Resource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input => formatter.write_str("input path"),
            Self::Source => formatter.write_str("Rawr source"),
            Self::Config => formatter.write_str("Rawr config"),
            Self::Boundary => formatter.write_str("project boundary"),
        }
    }
}

/// Failure to promote a filesystem path into a validated Rawr target.
#[derive(Debug)]
pub enum Error {
    /// The process current directory could not be read.
    CurrentDirectory(io::Error),
    /// An explicit path does not exist, including a broken symlink.
    InputNotFound {
        /// Path as supplied by the caller.
        path: PathBuf,
        /// Filesystem failure.
        source: io::Error,
    },
    /// No canonical project marker was found inside the bounded search.
    NotFound {
        /// Canonical directory where discovery started.
        start: PathBuf,
        /// Inclusive VCS or filesystem boundary where discovery stopped.
        boundary: PathBuf,
    },
    /// More than one project root was found before the search boundary.
    Ambiguous {
        /// Canonical directory where discovery started.
        start: PathBuf,
        /// Candidate roots ordered from nearest to farthest.
        candidates: Box<[PathBuf]>,
    },
    /// A required filesystem resource could not be inspected or read.
    Unreadable {
        /// Kind of resource being accessed.
        resource: Resource,
        /// Positioned filesystem path.
        path: PathBuf,
        /// Filesystem failure.
        source: io::Error,
    },
    /// An explicit input is neither a directory, `.rawr` source, nor `rawr.toml`.
    UnsupportedInput {
        /// Canonical unsupported path.
        path: PathBuf,
    },
    /// A project root does not contain its canonical source file.
    MissingSource {
        /// Canonical project root.
        root: PathBuf,
        /// Expected `src/main.rawr` path.
        expected: PathBuf,
        /// Manifest that made the directory an authoritative project, if any.
        config: Option<PathBuf>,
    },
    /// Explicit manifest selection did not resolve to a project manifest.
    MissingConfig {
        /// Canonical project root.
        root: PathBuf,
        /// Expected `rawr.toml` path.
        expected: PathBuf,
    },
    /// A project-owned symlink resolves outside its canonical root.
    EscapesRoot {
        /// Kind of resource that escaped.
        resource: Resource,
        /// Project-relative path containing the symlink.
        path: PathBuf,
        /// Canonical target reached through the symlink.
        resolved: PathBuf,
        /// Canonical project root the target must remain under.
        root: PathBuf,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentDirectory(source) => {
                write!(formatter, "cannot read the current directory: {source}")
            }
            Self::InputNotFound { path, .. } => write!(
                formatter,
                "cannot discover a Rawr target from `{}`: the path does not exist",
                path.display()
            ),
            Self::NotFound { start, boundary } => write!(
                formatter,
                "no Rawr project found from `{}` through `{}`; expected `src/main.rawr` or `rawr.toml`",
                start.display(),
                boundary.display()
            ),
            Self::Ambiguous { start, candidates } => {
                write!(
                    formatter,
                    "multiple Rawr projects found from `{}`; pass an exact project directory or `.rawr` file:",
                    start.display()
                )?;
                for candidate in candidates {
                    write!(formatter, " `{}`", candidate.display())?;
                }
                Ok(())
            }
            Self::Unreadable {
                resource,
                path,
                source,
            } => write!(
                formatter,
                "cannot read {resource} `{}`: {source}",
                path.display()
            ),
            Self::UnsupportedInput { path } => write!(
                formatter,
                "unsupported Rawr input `{}`; pass a directory, `.rawr` source file, or `rawr.toml`",
                path.display()
            ),
            Self::MissingSource {
                root,
                expected,
                config,
            } => {
                write!(
                    formatter,
                    "Rawr project `{}` is missing `{}`",
                    root.display(),
                    expected.display()
                )?;
                if let Some(config) = config {
                    write!(
                        formatter,
                        "; `{}` marks this directory as a project boundary",
                        config.display()
                    )?;
                }
                Ok(())
            }
            Self::MissingConfig { root, expected } => write!(
                formatter,
                "Rawr project `{}` is missing explicit manifest `{}`",
                root.display(),
                expected.display()
            ),
            Self::EscapesRoot {
                resource,
                path,
                resolved,
                root,
            } => write!(
                formatter,
                "{resource} `{}` resolves to `{}`, outside project root `{}`",
                path.display(),
                resolved.display(),
                root.display()
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDirectory(source)
            | Self::InputNotFound { source, .. }
            | Self::Unreadable { source, .. } => Some(source),
            Self::NotFound { .. }
            | Self::Ambiguous { .. }
            | Self::UnsupportedInput { .. }
            | Self::MissingSource { .. }
            | Self::MissingConfig { .. }
            | Self::EscapesRoot { .. } => None,
        }
    }
}
