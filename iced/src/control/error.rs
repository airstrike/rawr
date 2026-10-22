//! Typed failures from the in-process preview controls.

/// A rejected preview control or deterministic snapshot request.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The current surface is the application rather than a preview.
    #[error("no preview is selected")]
    Application,
    /// The requested preview transaction failed with canonical evidence.
    #[error("{0}")]
    Rejected(rawr::diagnostic::Report),
    /// The checked source did not retain a validated filesystem root.
    #[error("snapshot requests require a validated project or source-file root")]
    Root,
    /// The deterministic snapshot generation failed.
    #[error(transparent)]
    Snapshot(crate::snapshot::Error),
}

impl Error {
    /// Return the canonical rejection report, when the host retained one.
    #[must_use]
    pub const fn report(&self) -> Option<&rawr::diagnostic::Report> {
        match self {
            Self::Rejected(report) => Some(report),
            Self::Application | Self::Root | Self::Snapshot(_) => None,
        }
    }
}
