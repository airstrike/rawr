//! Opaque authority for the exact Cargo worker count.

use std::num::NonZeroUsize;

use crate::{Error, Result};

const SCHEMA: u32 = 1;
const MAXIMUM_JOBS: NonZeroUsize = NonZeroUsize::new(12).unwrap();

/// A discovered host capability transformed into one bounded Cargo worker count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Proof {
    jobs: NonZeroUsize,
}

impl Proof {
    pub(crate) fn discover() -> Result<Self> {
        let available = std::thread::available_parallelism().map_err(|source| Error::Request {
            reason: format!("could not determine the number of CPUs: {source}").into_boxed_str(),
        })?;
        Self::from_available(available)
    }

    fn from_available(available: NonZeroUsize) -> Result<Self> {
        Ok(Self {
            jobs: available.min(MAXIMUM_JOBS),
        })
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
