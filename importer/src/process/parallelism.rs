//! Opaque authority for the exact Cargo worker count.

use std::num::NonZeroUsize;

use crate::{Error, Result};

const SCHEMA: u32 = 1;
const MAXIMUM_JOBS: usize = 12;

/// A discovered host capability transformed into one bounded Cargo worker count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Proof {
    jobs: NonZeroUsize,
}

impl Proof {
    pub(crate) fn discover() -> Result<Self> {
        let available = std::thread::available_parallelism().map_err(|source| Error::Request {
            field: "Cargo parallelism",
            reason: format!("failed to discover available parallelism: {source}").into_boxed_str(),
        })?;
        Self::from_available(available)
    }

    fn from_available(available: NonZeroUsize) -> Result<Self> {
        let jobs = available.get().min(MAXIMUM_JOBS);
        let jobs = NonZeroUsize::new(jobs).ok_or_else(|| Error::Request {
            field: "Cargo parallelism",
            reason: "available parallelism produced a zero worker count".into(),
        })?;
        Ok(Self { jobs })
    }

    pub(crate) const fn schema(self) -> u32 {
        SCHEMA
    }

    pub(crate) const fn jobs(self) -> NonZeroUsize {
        self.jobs
    }

    pub(crate) fn revalidate(self, command: &'static str) -> Result<()> {
        let current = Self::discover()?;
        if current == self {
            Ok(())
        } else {
            Err(Error::ResolutionDrift {
                command,
                field: "Cargo parallelism proof",
            })
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture(available: NonZeroUsize) -> Result<Self> {
        Self::from_available(available)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::Proof;

    #[test]
    fn discovered_parallelism_is_nonzero_and_capped() {
        let one = Proof::fixture(NonZeroUsize::MIN).expect("one worker is valid");
        let many = Proof::fixture(NonZeroUsize::new(64).expect("fixture is nonzero"))
            .expect("large availability is bounded");
        assert_eq!(one.jobs().get(), 1);
        assert_eq!(many.jobs().get(), 12);
    }
}
