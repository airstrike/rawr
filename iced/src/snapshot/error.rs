//! Structured deterministic snapshot failures.

use std::path::PathBuf;

/// Class of a baseline comparison mismatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Class {
    /// A rendered artifact has no committed baseline.
    Missing,
    /// Expected and actual pixels differ at equal dimensions.
    Pixel,
    /// Expected and actual image dimensions differ.
    Size,
    /// A prior baseline no longer belongs to the rendered generation.
    Stale,
    /// Manifest inputs or build facts differ.
    Input,
}

impl std::fmt::Display for Class {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Missing => "missing baseline",
            Self::Pixel => "pixel drift",
            Self::Size => "size mismatch",
            Self::Stale => "stale baseline",
            Self::Input => "snapshot input mismatch",
        })
    }
}

/// Failure while proving, rendering, or storing a snapshot generation.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Preview compilation or runtime binding failed.
    #[error(transparent)]
    Preview(#[from] crate::preview::Error),
    /// Named in-memory sources do not own a validated filesystem root.
    #[error("snapshot commands require a discovered Rawr file or project root")]
    MissingRoot,
    /// No preview opted into deterministic snapshots.
    #[error("the preview Catalog has no entries with `snapshot = true`")]
    Empty,
    /// A compiler-private preview proof lacked stable digest evidence.
    #[error("snapshot `{identity}` cannot produce canonical digest evidence: {source}")]
    Digest {
        /// Stable preview identity.
        identity: String,
        /// Exact compiler invariant failure.
        #[source]
        source: rawr::preview::digest::Error,
    },
    /// A compiler-proven preview callback failed at runtime.
    #[error("snapshot `{identity}` failed during {stage}: {source}")]
    Runtime {
        /// Stable preview identity.
        identity: String,
        /// Callback stage.
        stage: &'static str,
        /// Exact runtime failure.
        #[source]
        source: rawr::preview::RuntimeError,
    },
    /// A theme callback returned the wrong runtime value.
    #[error("snapshot `{identity}` theme `{theme}` returned an invalid value: {source}")]
    Theme {
        /// Stable preview identity.
        identity: String,
        /// Declared theme label.
        theme: String,
        /// Exact conversion failure.
        #[source]
        source: crate::theme::Error,
    },
    /// A preview subject returned the wrong runtime value.
    #[error("snapshot `{identity}` returned an invalid widget value: {source}")]
    Widget {
        /// Stable preview identity.
        identity: String,
        /// Exact conversion failure.
        #[source]
        source: crate::widget::Error,
    },
    /// The pinned software renderer could not initialize.
    #[error("the pinned tiny-skia snapshot renderer could not initialize")]
    Renderer,
    /// The suite's renderer was poisoned by a prior panic.
    #[error("the deterministic snapshot renderer is unavailable after a prior panic")]
    RendererLock,
    /// The renderer returned an invalid RGBA buffer.
    #[error("renderer returned {actual} RGBA bytes for {width}x{height}; expected {expected}")]
    Buffer {
        /// Physical image width.
        width: u32,
        /// Physical image height.
        height: u32,
        /// Required byte count.
        expected: usize,
        /// Returned byte count.
        actual: usize,
    },
    /// Two semantic snapshot identities resolved to one path.
    #[error("snapshot paths collide at {path}: `{first}` and `{second}`")]
    Collision {
        /// Colliding relative path.
        path: PathBuf,
        /// First identity.
        first: String,
        /// Second identity.
        second: String,
    },
    /// A snapshot path escaped its generation root.
    #[error("snapshot artifact path is not a safe relative path: {path}")]
    UnsafePath {
        /// Rejected path.
        path: PathBuf,
    },
    /// A claimed baseline image could not be read or decoded for comparison.
    #[error(
        "snapshot `{identity}` theme {theme} could not compare expected {expected} with actual {actual}: {source}"
    )]
    Image {
        /// Stable preview identity.
        identity: String,
        /// Deterministic theme input.
        theme: String,
        /// Committed expected artifact path.
        expected: PathBuf,
        /// Fresh actual artifact path.
        actual: PathBuf,
        /// Exact typed image failure.
        #[source]
        source: Box<Self>,
    },
    /// Filesystem work failed at an exact artifact path.
    #[error("snapshot filesystem operation failed at {path}: {source}")]
    Io {
        /// Exact affected path.
        path: PathBuf,
        /// Operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// A PNG could not be encoded.
    #[error("could not encode snapshot PNG {path}: {source}")]
    Encode {
        /// Exact artifact path.
        path: PathBuf,
        /// PNG failure.
        #[source]
        source: png::EncodingError,
    },
    /// A baseline PNG could not be decoded.
    #[error("could not decode snapshot PNG {path}: {source}")]
    Decode {
        /// Exact artifact path.
        path: PathBuf,
        /// PNG failure.
        #[source]
        source: png::DecodingError,
    },
    /// A PNG uses a format outside the deterministic RGBA8 contract.
    #[error("snapshot PNG {path} is {color:?}/{depth:?}; expected Rgba/Eight")]
    Format {
        /// Exact artifact path.
        path: PathBuf,
        /// Actual color type.
        color: png::ColorType,
        /// Actual bit depth.
        depth: png::BitDepth,
    },
    /// A baseline manifest is malformed.
    #[error("snapshot manifest {path} is invalid at line {line}: {message}")]
    Manifest {
        /// Exact manifest path.
        path: PathBuf,
        /// One-based line number.
        line: usize,
        /// Concise parse failure.
        message: String,
    },
    /// Transactional replacement failed and rollback also failed.
    #[error(
        "snapshot swap failed at {target}: {source}; rollback from {backup} also failed: {rollback}; fresh generation remains at {stage}"
    )]
    Rollback {
        /// Intended generation root.
        target: PathBuf,
        /// Recoverable prior generation.
        backup: PathBuf,
        /// Recoverable freshly rendered generation.
        stage: PathBuf,
        /// Failed install operation.
        source: std::io::Error,
        /// Failed rollback operation.
        rollback: std::io::Error,
    },
    /// An operation and the cleanup of its recoverable staging data both failed.
    #[error(
        "snapshot operation failed at {target}: {operation}; recoverable data remains at {recoverable} because cleanup also failed: {cleanup}"
    )]
    Remains {
        /// Intended generation root.
        target: PathBuf,
        /// Exact recoverable staging data left on disk.
        recoverable: PathBuf,
        /// Original operation failure.
        #[source]
        operation: Box<Self>,
        /// Cleanup failure.
        cleanup: std::io::Error,
    },
    /// A later operation failed after prior commits retained cleanup warnings.
    #[error("snapshot operation failed after {commit}: {operation}")]
    Following {
        /// Exact earlier commit and its cleanup evidence.
        commit: super::Commit,
        /// Later operation failure.
        #[source]
        operation: Box<Self>,
    },
}

impl Error {
    pub(super) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
