//! Opaque application-bearing executable candidate.

use crate::{application, item, program};
use std::fmt;

/// An intact executable promoted by requiring one restricted application root.
///
/// A candidate proves root shape and stable symbolic selection only. It is not
/// the final application descriptor; callback/type/capability unification must
/// consume it in the next compiler transformation.
///
/// ```compile_fail
/// let ast = rawr::Engine::new().compile("40 + 2").unwrap();
/// let _ = ast.require_application();
/// ```
///
/// ```compile_fail
/// use rawr::application::Candidate;
/// use rawr::Engine;
/// let executable = Engine::new().compile_executable("40 + 2").unwrap();
/// let _ = Candidate { executable };
/// ```
///
/// ```compile_fail
/// use rawr::{Engine, Shared};
/// let engine = Shared::new(Engine::new());
/// let candidate = engine.compile_application("40 + 2").unwrap();
/// let _ = engine.launch_application(candidate);
/// ```
pub struct Candidate {
    pub(super) executable: program::Executable,
}

impl fmt::Debug for Candidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Candidate")
            .field("entry", self.entry())
            .field("executable", &"<private>")
            .finish()
    }
}

impl Candidate {
    pub(crate) fn from_executable(
        executable: program::Executable,
    ) -> Result<Self, application::Error> {
        if executable.application().is_none() {
            return Err(application::Error::MissingEntry);
        }
        Ok(Self { executable })
    }

    /// Return the restricted application-root proof paired with this executable.
    #[must_use]
    pub fn entry(&self) -> &application::Entry {
        self.executable
            .application()
            .expect("application candidates are born only from executables with one entry")
    }

    /// Return this candidate's opaque executable generation identifier.
    #[must_use]
    pub const fn generation(&self) -> program::GenerationId {
        self.executable.generation()
    }

    /// Iterate over stable symbolic callables in the paired executable.
    pub fn items(&self) -> impl ExactSizeIterator<Item = &item::Value> {
        self.executable.items()
    }

    /// Consume this root candidate by validating its complete application contract.
    pub fn validate(self) -> Result<application::Descriptor, application::Error> {
        application::Descriptor::from_candidate(self)
    }

    pub(crate) fn validate_package(
        self,
    ) -> Result<application::Descriptor, (program::source::Id, application::Error)> {
        application::Descriptor::from_package_candidate(self)
    }
}
