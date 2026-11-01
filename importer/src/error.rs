//! Structured importer failures.

use std::path::PathBuf;

use thiserror::Error;

/// A failure produced while transforming an import request.
#[derive(Debug, Error)]
pub enum Error {
    /// A content-addressed importer node failed inside the legacy product edge.
    #[error("{source}")]
    Node {
        /// The product transformation being migrated.
        operation: &'static str,
        /// Exact typed-node failure.
        #[source]
        source: Box<crate::node::Error>,
    },

    /// Another build holds the build directory, as Cargo's "Blocking
    /// waiting for file lock on build directory" says of its own.
    #[error("the build directory is locked by another build; run again when it finishes")]
    Owner,

    /// Something in the user's Cargo setup or environment that rawr can't
    /// build with, said as Cargo or rustc would say it. A condition only a
    /// rawr bug could cause is a panic, never this.
    #[error("{reason}")]
    Request {
        /// The whole message, naming the dependency, file or variable.
        reason: Box<str>,
    },

    /// A path rawr can't build from, as Cargo names one: the path, then
    /// what is wrong with it.
    #[error("`{}` {reason}", path.display())]
    Path {
        /// Which path this is, for tests. Never shown.
        field: &'static str,
        /// The rejected path.
        path: PathBuf,
        /// What is wrong with the path, in words that follow it.
        reason: Box<str>,
    },

    /// A filesystem operation failed while producing importer evidence.
    #[error("failed to {operation} `{path}`: {source}")]
    Io {
        /// The attempted operation.
        operation: &'static str,
        /// The affected path.
        path: PathBuf,
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },

    /// Owned replay or capture storage could not be allocated.
    #[error("failed to allocate {operation}: {source}")]
    Allocation {
        /// The owned value whose capacity could not be reserved.
        operation: &'static str,
        /// Original allocator or capacity-overflow failure.
        #[source]
        source: std::collections::TryReserveError,
    },

    /// A bounded importer resource exceeded its sealed limit.
    #[error("{resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        /// The bounded resource.
        resource: &'static str,
        /// The configured maximum.
        limit: usize,
        /// The observed size.
        actual: usize,
    },

    /// A Rust future a task runs is not `Send`, so the runner cannot run it.
    #[error(
        "the future `{function}` returns is not `Send` ({location}). Its runtime runs it \
         on whichever thread polls it, so it takes only a `Send` future, as iced's \
         `Task::future` and tokio's `spawn` do. Hold `Send` values across each `.await`, \
         such as `Arc` in place of `Rc`."
    )]
    NonSendFuture {
        /// The Rust function whose future is not `Send`.
        function: Box<str>,
        /// Where its body is, as rustc reports it.
        location: Box<str>,
    },

    /// A generic Rust item bounds a type parameter that a script type fills
    /// by a trait `rawr::Value` lacks (D513.2).
    #[error(
        "a generic Rust item needs `{bound}` of the script type it is called with \
         ({location}). Rawr passes a script value to native generic code as \
         `rawr::Value`, which carries it but can't give native code a trait that \
         picks behaviour by type alone, such as `Default`, `Copy` or `Deserialize`. \
         Call it with a Rust type there."
    )]
    ErasedBound {
        /// The bound `rawr::Value` doesn't meet, as rustc wrote it.
        bound: Box<str>,
        /// Where the generated runner calls the item.
        location: Box<str>,
    },

    /// A child command failed without publishing resolution evidence.
    #[error("{program} command failed with {status}: {stderr}")]
    Process {
        /// The logical pinned program.
        program: &'static str,
        /// The exit status or launch failure classification.
        status: Box<str>,
        /// The bounded diagnostic tail.
        stderr: Box<str>,
    },

    /// The installed toolchain does not match the required pin.
    #[error("toolchain {field} mismatch: expected `{expected}`, found `{actual}`")]
    Toolchain {
        /// The mismatched fact.
        field: &'static str,
        /// The required value.
        expected: Box<str>,
        /// The observed value.
        actual: Box<str>,
    },

    /// rawr could not read what Cargo, rustup or its own cache wrote: a bug
    /// in rawr, never in the program, so it says so as rustc's ICEs do.
    #[error("internal error: rawr misread {document}: {reason}. This is a rawr bug")]
    Schema {
        /// The bounded document being decoded.
        document: &'static str,
        /// The structured decoding failure.
        reason: Box<str>,
    },

    /// Cargo answered differently for the same project within one build:
    /// a Cargo.toml, Cargo.lock or Cargo configuration file changed while
    /// it ran, or rawr misread Cargo.
    #[error(
        "Cargo.toml, Cargo.lock or the Cargo configuration changed during the build; \
         run it again. If none changed, this is a rawr bug ({field} differs after {command})"
    )]
    ResolutionDrift {
        /// The command whose postcondition failed.
        command: &'static str,
        /// The exact equality field that changed.
        field: &'static str,
    },

    /// A file the build read changed, or can't be read as Cargo wrote it.
    /// `code` names the condition for tests and is shown only for a bug.
    #[error("{}", input_message(code, reason))]
    Input {
        /// Stable machine-readable rejection code.
        code: &'static str,
        /// Path-, package-, or schema-bearing rejection detail.
        reason: Box<str>,
    },

    /// A previous resource refusal permanently closed the input allocation account.
    #[error("input allocation account has already refused work")]
    InputAccountRefused,

    /// rawr could not read what its rustc driver wrote: a bug in rawr,
    /// never in the program, so it says so as rustc's own ICEs do.
    #[error("internal error: rawr misread rustc's output (`{code}`): {reason}")]
    Extraction {
        /// Stable machine-readable rejection code.
        code: &'static str,
        /// Positioned or identity-bearing rejection detail.
        reason: Box<str>,
    },
}

impl Error {
    /// The invariant this failure breaks, when only a rawr bug can cause it.
    /// The cache check reads these errors as an entry to rebuild; a fresh
    /// build panics with this (`node::Error::fresh`, #522).
    #[must_use]
    pub fn invariant(&self) -> Option<String> {
        match self {
            Self::Node { source, .. } => source.invariant(),
            Self::Schema { document, reason } => {
                Some(format!("rawr reads {document} as it was written: {reason}"))
            }
            Self::Input { code, reason } if input_condition(code, reason).is_none() => Some(
                format!("rawr keeps track of each build input (`{code}`): {reason}"),
            ),
            Self::Extraction { code, reason } => Some(format!(
                "rawr reads what its rustc driver writes (`{code}`): {reason}"
            )),
            _ => None,
        }
    }
}

/// An input failure in Cargo's words: a file that changed during the build,
/// a path or registry entry that can't be read, or else a rawr bug, said as
/// rustc's ICEs say one.
fn input_message(code: &str, reason: &str) -> String {
    input_condition(code, reason).unwrap_or_else(|| {
        format!(
            "internal error: rawr lost track of a build input (`{code}`): {reason}. \
             This is a rawr bug"
        )
    })
}

/// An input failure the program's files or setup can cause, in Cargo's
/// words. Any other code is a rawr bug.
fn input_condition(code: &str, reason: &str) -> Option<String> {
    Some(match code {
        "input-changed" => "a file the build read changed during the build; run it again".into(),
        "input-changed-during-read" | "package-file-set-changed" | "registry-marker-changed" => {
            format!("`{reason}` changed during the build; run it again")
        }
        "package-file-set-utf8"
        | "dep-info-utf8"
        | "saved-output-utf8"
        | "toolchain-executable-path-encoding"
        | "protected-build-package-path-encoding"
        | "runner-harness-fixture-path"
        | "runner-harness-fixture-root" => format!("a path is not valid UTF-8: {reason}"),
        "symlink-input" => {
            format!("`{reason}` is reached through a symlink, which rawr can't build from yet")
        }
        "cargo-commit-skew" => format!(
            "this Cargo (commit `{reason}`) is not the one rawr builds with; \
             install the toolchain rawr names"
        ),
        "registry-checksum-authority-absent" => {
            format!("`{reason}` has no checksum in Cargo.lock")
        }
        "registry-root-outside-cargo-home" => format!(
            "`{reason}` is outside CARGO_HOME: a replaced or vendored registry source, \
             which rawr can't build from yet"
        ),
        "registry-marker-malformed" | "registry-marker-schema" => format!(
            "Cargo's registry copy of a package is damaged ({reason}); \
             delete it under CARGO_HOME/registry/src and run again"
        ),
        "missing-cargo-home-authority" => {
            format!("CARGO_HOME is unset and there is no home directory, so Cargo can't fetch `{reason}`")
        }
        "cargo-env-config-unproved" => "a build script's `cargo::rerun-if-env-changed` names a \
             variable that `[env]` in .cargo/config.toml sets, which rawr can't build with yet"
            .into(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::Error;

    /// Each message names the user's files and setup in Cargo's words, and
    /// a condition only rawr can cause says it's a rawr bug (#483, D664).
    #[test]
    fn messages_name_the_project_in_cargos_words() {
        let cases = [
            (
                Error::Owner,
                "the build directory is locked by another build; run again when it finishes",
            ),
            (
                Error::Path {
                    field: "Cargo config candidate",
                    path: "/project/.cargo/config.toml".into(),
                    reason: "is reached through a symlink".into(),
                },
                "`/project/.cargo/config.toml` is reached through a symlink",
            ),
            (
                Error::ResolutionDrift {
                    command: "metadata",
                    field: "selected whole lock",
                },
                "Cargo.toml, Cargo.lock or the Cargo configuration changed during the build; \
                 run it again. If none changed, this is a rawr bug (selected whole lock differs \
                 after metadata)",
            ),
            (
                Error::Schema {
                    document: "Cargo unit graph",
                    reason: "missing field `units`".into(),
                },
                "internal error: rawr misread Cargo unit graph: missing field `units`. \
                 This is a rawr bug",
            ),
            (
                Error::Input {
                    code: "input-changed-during-read",
                    reason: "/project/src/main.rs".into(),
                },
                "`/project/src/main.rs` changed during the build; run it again",
            ),
            (
                Error::Input {
                    code: "registry-checksum-authority-absent",
                    reason: "crunchy 0.2.4".into(),
                },
                "`crunchy 0.2.4` has no checksum in Cargo.lock",
            ),
            (
                Error::Input {
                    code: "protected-build-package-owner-missing",
                    reason: "crunchy 0.2.4".into(),
                },
                "internal error: rawr lost track of a build input \
                 (`protected-build-package-owner-missing`): crunchy 0.2.4. This is a rawr bug",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
        }
    }
}
