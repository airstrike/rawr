//! One original account and a first-cause terminal resource/source-failure latch.

use rawr_toml::document;

use super::{Diagnostic, Error, Source, Span, Step, Stop};

mod host;

pub(super) struct Session<'a, A: document::Admission> {
    admission: &'a mut A,
    failure: Option<Error<A::Error>>,
}

impl<'a, A: document::Admission> Session<'a, A> {
    pub(super) fn new(admission: &'a mut A) -> Self {
        Self {
            admission,
            failure: None,
        }
    }

    pub(super) fn check(&self) -> Step<()> {
        if self.failure.is_some() {
            Err(Stop::Refused)
        } else {
            Ok(())
        }
    }

    pub(super) fn admit(&mut self, request: document::Request) -> Step<()> {
        if self.failure.is_some() {
            return Err(Stop::Refused);
        }
        if let Err(error) = self.admission.admit(request) {
            return self.fail(Error::Admission(error));
        }
        Ok(())
    }

    pub(super) fn items(&mut self, amount: usize) -> Step<()> {
        self.admit(document::Request::Items(amount))
    }

    pub(super) fn string(&mut self, length: usize) -> Step<()> {
        self.admit(document::Request::String(length))
    }

    pub(super) fn fail<T>(&mut self, error: Error<A::Error>) -> Step<T> {
        if self.failure.is_none() {
            self.failure = Some(error);
        }
        Err(Stop::Refused)
    }

    pub(super) fn reject<T>(&mut self, diagnostic: Diagnostic) -> Step<T> {
        // The retained trial/final diagnostic is an actual logical item. Its
        // fixed tag and spans do not request a heap layout or rendered text.
        self.items(1)?;
        Err(Stop::Rejected(diagnostic))
    }

    /// Called only after the actual pinned spec reparse fails semantically.
    /// Its fixed diagnostic is admitted before retention; prior failure wins.
    pub(super) fn source_reparse<T>(&mut self, span: Option<Span>) -> Step<T> {
        self.items(1)?;
        self.fail(Error::Source {
            cause: Source::SpecReparse,
            span,
        })
    }

    /// Only the reached pinned partial-fence advance, not arbitrary panic.
    pub(super) fn source_frontmatter<T>(&mut self, cursor: usize, bytes: u8) -> Step<T> {
        self.items(1)?;
        self.fail(Error::Source {
            cause: Source::FrontmatterPartialAdvance { bytes },
            span: Some(Span {
                start: cursor,
                end: cursor,
            }),
        })
    }

    /// Only a reached file definition's absent second parent.
    pub(super) fn source_config_parent<T>(&mut self) -> Step<T> {
        self.items(1)?;
        self.fail(Error::Source {
            cause: Source::ConfigDefinitionSecondParent,
            span: None,
        })
    }

    /// Only after the actual joined dependency path fails to_str.
    pub(super) fn source_joined_dependency_path<T>(&mut self) -> Step<T> {
        self.items(1)?;
        self.fail(Error::Source {
            cause: Source::JoinedDependencyPathUtf8,
            span: None,
        })
    }

    /// Only actual None from the pinned build-script stem validator.
    pub(super) fn source_build_script_stem<T>(&mut self) -> Step<T> {
        self.items(1)?;
        self.fail(Error::Source {
            cause: Source::BuildScriptStem,
            span: None,
        })
    }

    pub(super) fn finish<T>(self, result: Step<T>) -> Result<T, Error<A::Error>> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        result.map_err(|stop| match stop {
            Stop::Rejected(diagnostic) => Error::Invalid(diagnostic),
            Stop::Refused => Error::State,
        })
    }
}
