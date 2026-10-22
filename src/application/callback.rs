//! Retained source evidence for compiler-proven application callbacks.

use crate::{diagnostic, program, Span};

use super::bound::RuntimeError;

/// Exact retained declaration evidence for one compiler-proven callback.
pub struct Evidence<'a> {
    evidence: program::Evidence<'a>,
}

impl<'a> Evidence<'a> {
    pub(super) const fn new(evidence: program::Evidence<'a>) -> Self {
        Self { evidence }
    }

    /// Return the immutable source snapshot containing the callback.
    #[must_use]
    pub const fn source(&self) -> &diagnostic::Source {
        self.evidence.source()
    }

    /// Return the exact callback declaration span within the retained source.
    #[must_use]
    pub const fn declaration(&self) -> Span {
        self.evidence.declaration()
    }

    /// Transform a callback invocation error using exact retained source evidence.
    #[must_use = "diagnostic adaptation failures must be handled"]
    pub fn program_report(
        &self,
        error: &program::Error,
    ) -> Result<diagnostic::Report, diagnostic::Error> {
        match self.evidence.translate(error) {
            Some(original) => {
                diagnostic::Report::from_program_mapped(self.source().clone(), original, error)
            }
            None => diagnostic::Report::from_program_at(
                self.source().clone(),
                self.declaration(),
                error,
            ),
        }
    }

    /// Transform a bound callback failure using exact retained source evidence.
    #[must_use = "diagnostic adaptation failures must be handled"]
    pub fn runtime_report(
        &self,
        error: &RuntimeError,
    ) -> Result<diagnostic::Report, diagnostic::Error> {
        let original = match error {
            RuntimeError::Invocation(error) | RuntimeError::Task(error) => {
                self.evidence.translate(error)
            }
            _ => None,
        };
        match original {
            Some(original) => diagnostic::Report::from_application_runtime_mapped(
                self.source().clone(),
                original,
                error,
            ),
            None => diagnostic::Report::from_application_runtime_at(
                self.source().clone(),
                self.declaration(),
                error,
            ),
        }
    }
}
