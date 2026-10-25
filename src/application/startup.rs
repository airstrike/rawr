//! Sealed transport for the exact explicit-boot result.

use crate::{program, Dynamic, ImmutableString};
use std::fmt;

/// A validated explicit application startup result.
///
/// This proof bundle is produced only by [`crate::application::BoundLaunch`]
/// after it has opened the compiler-private boot envelope and validated both
/// members against the current application contract.
pub struct Startup {
    state: Dynamic,
    task: Option<Dynamic>,
}

/// A compiler-private explicit-boot envelope could not be opened.
#[derive(Debug)]
pub enum Error {
    /// The callback returned a value other than the sealed startup envelope.
    MissingEnvelope {
        /// Runtime type returned by the callback.
        actual: ImmutableString,
    },
    /// The envelope was minted by a different executable generation.
    GenerationMismatch,
}

#[derive(Clone)]
pub(crate) struct Envelope {
    generation: program::GenerationId,
    state: Dynamic,
    task: Dynamic,
}

impl Startup {
    pub(crate) const fn new(state: Dynamic, task: Option<Dynamic>) -> Self {
        Self { state, task }
    }

    /// Consume the proof into its validated State and optional durable Task value.
    #[must_use = "validated startup values must be consumed"]
    pub fn into_parts(self) -> (Dynamic, Option<Dynamic>) {
        (self.state, self.task)
    }
}

impl Envelope {
    pub(crate) const fn seal(
        generation: program::GenerationId,
        state: Dynamic,
        task: Dynamic,
    ) -> Self {
        Self {
            generation,
            state,
            task,
        }
    }

    pub(crate) fn open(
        value: Dynamic,
        generation: program::GenerationId,
    ) -> Result<(Dynamic, Dynamic), Error> {
        let actual = value.type_name().into();
        let envelope = value
            .try_cast_result::<Self>()
            .map_err(|_| Error::MissingEnvelope { actual })?;
        if envelope.generation != generation {
            return Err(Error::GenerationMismatch);
        }
        Ok((envelope.state, envelope.task))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvelope { actual } => write!(
                formatter,
                "explicit boot returned `{actual}`, expected its sealed `(State, iced::Task<Message>)` envelope"
            ),
            Self::GenerationMismatch => formatter.write_str(
                "explicit boot returned a startup envelope from a different program generation",
            ),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{Envelope, Error};
    use crate::{program::GenerationId, Dynamic};

    #[test]
    fn sealed_envelopes_reject_a_different_generation() {
        let first = GenerationId::issue();
        let second = GenerationId::issue();
        let envelope = Dynamic::from(Envelope::seal(first, 1_i64.into(), 2_i64.into()));

        assert!(matches!(
            Envelope::open(envelope, second),
            Err(Error::GenerationMismatch)
        ));
    }
}
