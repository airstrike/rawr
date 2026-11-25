//! Structured generated-bridge request failures.

/// Failure before a generated manifest can be sealed. rawr writes the
/// runner's source, Cargo.toml and lock itself, so a runner it can't read
/// back or that disagrees with the crates is a rawr bug and panics
/// ([`drift`], [`misread`], [`unwritable`]), never one of these (#538, D664).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Node(#[source] Box<crate::node_parts::Error>),
    #[error("{0}")]
    Native(#[from] crate::Error),
    #[error(transparent)]
    Surface(#[from] crate::surface::Error),
    #[error("{resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("out of memory while generating the program's Rust")]
    Allocation,
    /// What a [`trial`] render met that the runner can't write.
    #[error("the runner can't write {0}")]
    Unwritten(&'static str),
}

impl Error {
    /// Whether a cache entry this request read went stale before it was
    /// used (`crate::Error::is_stale`), so the run prepares again (#613).
    #[must_use]
    pub fn is_stale(&self) -> bool {
        match self {
            Self::Node(error) => error.is_stale(),
            Self::Native(error) => error.is_stale(),
            Self::Surface(_) | Self::Limit { .. } | Self::Allocation | Self::Unwritten(_) => false,
        }
    }
}

/// The generated Rust disagrees with what rawr read of the crates: only a
/// rawr bug can cause it, so it panics naming the invariant.
/// Inside a [`trial`], it is that render's [`Error::Unwritten`] instead.
#[track_caller]
pub(crate) fn drift(field: &'static str) -> Error {
    if TRIAL.get() {
        return Error::Unwritten(field);
    }
    panic!("the generated Rust disagrees with the crates on {field}")
}

thread_local! {
    /// A [`trial`] render runs on this thread.
    static TRIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `render`, of what the runner may have no Rust for, as a crate trait's
/// stand-in whose methods' types no program chose (#2698): a type it
/// can't write is its `Err(Error::Unwritten(..))`, not a rawr bug.
pub(crate) fn trial<T>(render: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    let outer = TRIAL.replace(true);
    let rendered = render();
    TRIAL.set(outer);
    rendered
}

/// rawr can't read back the runner's Cargo.toml or Cargo.lock that it and
/// Cargo wrote: only a rawr bug can cause it.
#[track_caller]
pub(crate) fn misread(reason: impl std::fmt::Display) -> Error {
    panic!("cannot read back the runner's Cargo.toml or Cargo.lock: {reason}")
}

/// Writing the runner's source in memory failed, which only a rawr bug
/// can cause.
#[track_caller]
pub(crate) fn unwritable(error: impl std::fmt::Display) -> Error {
    panic!("cannot write the runner's generated source in memory: {error}")
}
