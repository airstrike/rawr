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
    /// A `Cargo.toml` file.
    Manifest,
    /// A VCS marker used as an ancestor-search boundary.
    Boundary,
}

impl fmt::Display for Resource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input => formatter.write_str("input path"),
            Self::Source => formatter.write_str("Rawr source"),
            Self::Manifest => formatter.write_str("Cargo manifest"),
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
        /// A `rawr.toml` or `src/main.rawr` seen on the way, which Rawr no
        /// longer reads.
        legacy: Option<PathBuf>,
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
    /// An explicit input is neither a directory, a `.rs` file nor a `Cargo.toml`.
    UnsupportedInput {
        /// Canonical unsupported path.
        path: PathBuf,
    },
    /// An explicit `.rawr` file or `rawr.toml`, which Rawr no longer reads.
    Legacy {
        /// Canonical path of the `.rawr` file or `rawr.toml`.
        path: PathBuf,
    },
    /// A Cargo crate has neither `src/main.rs` nor `src/lib.rs`.
    MissingSource {
        /// Canonical crate root.
        root: PathBuf,
        /// Expected `src/main.rs` path.
        expected: PathBuf,
    },
    /// A Cargo package's crate root does not start with the `// rawr` marker.
    Unmarked {
        /// Canonical project root.
        root: PathBuf,
        /// The unmarked `src/main.rs` or `src/lib.rs`.
        entry: PathBuf,
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
            Self::NotFound {
                start,
                boundary,
                legacy,
            } => {
                write!(
                    formatter,
                    "no Rawr project found from `{}` through `{}`. \
                     A Rawr project is a Cargo crate whose `src/main.rs` starts with `// rawr`.",
                    start.display(),
                    boundary.display()
                )?;
                if let Some(legacy) = legacy {
                    write!(
                        formatter,
                        " `{}` is no longer read. \
                         Rawr does not read rawr.toml or `.rawr` files anymore.",
                        legacy.display()
                    )?;
                }
                Ok(())
            }
            Self::Ambiguous { start, candidates } => {
                write!(
                    formatter,
                    "multiple Rawr projects found from `{}`; pass an exact crate directory or `.rs` file:",
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
                "unsupported Rawr input `{}`. Pass a directory, a `.rs` file or a `Cargo.toml`.",
                path.display()
            ),
            Self::Legacy { path } if path.extension() == Some(std::ffi::OsStr::new("rawr")) => {
                write!(
                    formatter,
                    "`{}` is a `.rawr` file. Rawr reads `.rs` files now. \
                     Rename the file to `.rs` and put `// rawr` on its first line. \
                     A project goes in a Cargo crate whose `src/main.rs` starts with `// rawr`.",
                    path.display()
                )
            }
            Self::Legacy { path } => write!(
                formatter,
                "`{}` is no longer read. Rawr reads `.rs` files in a Cargo crate now. \
                 Put the project in a Cargo crate whose `src/main.rs` starts with `// rawr`. \
                 Rename each `.rawr` file to `.rs` and put `// rawr` on its first line. \
                 Move the dependencies to the `[dependencies]` of `Cargo.toml`.",
                path.display()
            ),
            Self::MissingSource { root, expected } => write!(
                formatter,
                "Cargo crate `{}` is missing `{}`",
                root.display(),
                expected.display()
            ),
            Self::Unmarked { entry, .. } => write!(
                formatter,
                "`{}` is not a Rawr file. Its first line is not `// rawr`.",
                entry.display()
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
            | Self::Legacy { .. }
            | Self::MissingSource { .. }
            | Self::Unmarked { .. }
            | Self::EscapesRoot { .. } => None,
        }
    }
}
