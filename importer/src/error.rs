//! Structured importer failures.

use std::path::PathBuf;

use thiserror::Error;

/// A failure produced while transforming an import request.
#[derive(Debug, Error)]
pub enum Error {
    /// A content-addressed importer node failed inside the legacy product edge.
    #[error("content-addressed {operation} node failed: {source}")]
    Node {
        /// The product transformation being migrated.
        operation: &'static str,
        /// Exact typed-node failure.
        #[source]
        source: Box<crate::node::Error>,
    },

    /// The process-wide or cross-process Cargo writer authority is unavailable.
    #[error("Cargo resolution owner is unavailable: {state}")]
    Owner {
        /// The structured unavailable-owner state.
        state: &'static str,
    },

    /// A named request field could not be promoted into valid importer input.
    #[error("invalid import request field `{field}`: {reason}")]
    Request {
        /// The rejected request field.
        field: &'static str,
        /// The reason the field was rejected.
        reason: Box<str>,
    },

    /// A filesystem path could not be promoted into canonical importer input.
    #[error("invalid {field} path `{path}`: {reason}")]
    Path {
        /// The request field that supplied the path.
        field: &'static str,
        /// The rejected path.
        path: PathBuf,
        /// Why the path could not be sealed.
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
        "the future `{function}` returns is not `Send` ({location}). iced runs a task's \
         future on its thread pool, which takes only a `Send` future, as iced's own \
         `Task::future` does. Hold `Send` values across each `.await`, such as `Arc` \
         in place of `Rc`."
    )]
    NonSendFuture {
        /// The Rust function whose future is not `Send`.
        function: Box<str>,
        /// Where its body is, as rustc reports it.
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

    /// A pinned Cargo schema could not be decoded exactly.
    #[error("invalid {document} schema: {reason}")]
    Schema {
        /// The bounded document being decoded.
        document: &'static str,
        /// The structured decoding failure.
        reason: Box<str>,
    },

    /// Cargo changed the sealed resolution universe during a command.
    #[error("Cargo resolution drift after {command}: {field} changed")]
    ResolutionDrift {
        /// The command whose postcondition failed.
        command: &'static str,
        /// The exact equality field that changed.
        field: &'static str,
    },

    /// Content or saved-output evidence could not become an exact freshness proof.
    #[error("normal-build input proof rejected `{code}`: {reason}")]
    Input {
        /// Stable machine-readable rejection code.
        code: &'static str,
        /// Path-, package-, or schema-bearing rejection detail.
        reason: Box<str>,
    },

    /// A previous resource refusal permanently closed the input allocation account.
    #[error("input allocation account has already refused work")]
    InputAccountRefused,

    /// A compiler sidecar could not be transformed into canonical API evidence.
    #[error("Rust API extraction rejected `{code}`: {reason}")]
    Extraction {
        /// Stable machine-readable rejection code.
        code: &'static str,
        /// Positioned or identity-bearing rejection detail.
        reason: Box<str>,
    },
}
