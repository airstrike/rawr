//! Reload planning at the sealed preview-host owner boundary.

use rawr::reload::compatibility;

/// One committed control-host replacement.
pub struct Outcome {
    evidence: Box<[compatibility::Evidence]>,
    inputs: rawr::reload::scheduler::publication::Retired,
}

/// A preview-host replacement could not be planned or installed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Static hosts and closed ownerless hosts have no reload proof boundary.
    #[error("preview host has no reloadable live owner")]
    Unavailable,
    /// Snapshot suites retain an executable catalog and cannot cross generations.
    #[error("snapshot-enabled preview hosts cannot be reloaded")]
    Snapshots,
    /// Candidate native binding failed before host preparation.
    #[error(transparent)]
    Binding(rawr::reload::installation::Error),
    /// Replacement evidence did not exactly match the live host owners.
    #[error("replacement evidence does not match the live preview host owners")]
    Owner,
    /// A required live owner is absent from the candidate.
    #[error("replacement is missing one retained preview host owner")]
    Missing,
    /// Candidate boot, view, theme, or subject evaluation failed.
    #[error("{0}")]
    Candidate(rawr::diagnostic::Report),
    /// Candidate application preparation failed.
    #[error(transparent)]
    Application(crate::program::install::Error),
    /// Candidate startup work could not be rebound.
    #[error(transparent)]
    Task(crate::task::Error),
    /// Exact source or manifest contents changed after candidate compilation.
    #[error(transparent)]
    Source(rawr::reload::source::Error),
    /// The exact host identity, state stamp, or generation became stale.
    #[error("replacement no longer matches the exact preview host state")]
    Stale,
}

impl Outcome {
    pub(crate) const fn new(
        evidence: Box<[compatibility::Evidence]>,
        inputs: rawr::reload::scheduler::publication::Retired,
    ) -> Self {
        Self { evidence, inputs }
    }

    /// Return the exact owner actions committed together.
    #[must_use = "compatibility evidence explains the committed owner actions"]
    pub const fn evidence(&self) -> &[compatibility::Evidence] {
        &self.evidence
    }

    /// Consume this outcome into the retired exact input publication.
    #[must_use = "retired exact inputs must be reconciled"]
    pub fn into_inputs(self) -> rawr::reload::scheduler::publication::Retired {
        self.inputs
    }
}

impl Error {
    pub(crate) fn host(error: crate::preview::host::install::Error) -> Self {
        match error {
            crate::preview::host::install::Error::Unavailable => Self::Unavailable,
            crate::preview::host::install::Error::Binding(error) => Self::Binding(error),
            crate::preview::host::install::Error::Owner => Self::Owner,
            crate::preview::host::install::Error::Missing => Self::Missing,
            crate::preview::host::install::Error::Candidate(report) => Self::Candidate(report),
            crate::preview::host::install::Error::Diagnostic(report) => Self::Candidate(report),
            crate::preview::host::install::Error::Application(error) => Self::Application(error),
            crate::preview::host::install::Error::Task(error) => Self::Task(error),
            crate::preview::host::install::Error::Source(error) => Self::Source(error),
            crate::preview::host::install::Error::Stale => Self::Stale,
        }
    }
}
