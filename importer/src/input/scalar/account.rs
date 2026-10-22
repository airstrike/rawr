//! Closed storage admission for the shared scalar producers.

use std::collections::TryReserveError;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::input::Allocation;
use crate::Error;

pub(crate) mod sealed {
    pub(crate) trait Sealed {}
}

pub(crate) enum Failure {
    Length,
    Allocation(TryReserveError),
    Render,
    ProvenanceAllocation(TryReserveError),
    ProvenanceRender(fmt::Error),
    CargoEnvironmentOverrides,
}

/// The two original account paths; neither implementation owns another ledger.
pub(crate) trait Account: sealed::Sealed {
    type Error;

    fn check(&self) -> Result<(), Self::Error>;
    fn storage(&mut self, bytes: usize) -> Result<(), Self::Error>;
    fn package_path(&mut self, path: &Path) -> Result<(), Self::Error>;
    fn declared_path(&mut self, root: &Path, value: &str) -> Result<PathBuf, Self::Error>;
    fn path_text<'path>(
        &mut self,
        field: &'static str,
        path: &'path Path,
    ) -> Result<&'path str, Self::Error>;
    fn executable_path<'path>(&mut self, path: &'path Path) -> Result<&'path str, Self::Error>;
    fn bounded(
        &mut self,
        resource: &'static str,
        limit: usize,
        actual: usize,
    ) -> Result<(), Self::Error>;
    fn failure(&mut self, failure: Failure) -> Self::Error;
}

impl sealed::Sealed for Allocation {}

impl Account for Allocation {
    type Error = Error;

    fn check(&self) -> Result<(), Error> {
        Allocation::check(self)
    }

    fn storage(&mut self, bytes: usize) -> Result<(), Error> {
        self.items(1)?;
        self.bytes(bytes)
    }

    fn package_path(&mut self, path: &Path) -> Result<(), Error> {
        self.path(path)
    }

    fn declared_path(&mut self, root: &Path, value: &str) -> Result<PathBuf, Error> {
        crate::input::declared_path(root, value)
    }

    fn path_text<'path>(
        &mut self,
        field: &'static str,
        path: &'path Path,
    ) -> Result<&'path str, Error> {
        crate::input::path_text(field, path)
    }

    fn executable_path<'path>(&mut self, path: &'path Path) -> Result<&'path str, Error> {
        path.to_str().ok_or_else(|| Error::Input {
            code: "toolchain-executable-path-encoding",
            reason: path.display().to_string().into_boxed_str(),
        })
    }

    /// Every scalar `bounded` comparison is an accounting target
    /// (`string_bytes`); record a crossing on this input account and continue.
    fn bounded(
        &mut self,
        resource: &'static str,
        limit: usize,
        actual: usize,
    ) -> Result<(), Error> {
        Allocation::check(self)?;
        self.exceeded(resource, limit, actual);
        Ok(())
    }

    fn failure(&mut self, failure: Failure) -> Error {
        match failure {
            Failure::Length => {
                self.refused = true;
                Error::Input {
                    code: "input-format-length",
                    reason: "formatted freshness string length overflowed".into(),
                }
            }
            Failure::Allocation(error) => {
                self.refused = true;
                Error::Input {
                    code: "input-format-allocation",
                    reason: error.to_string().into_boxed_str(),
                }
            }
            Failure::Render => Error::Input {
                code: "input-format-render",
                reason: "formatted freshness string rendering failed".into(),
            },
            Failure::ProvenanceAllocation(error) => Error::Input {
                code: "config-provenance-allocation",
                reason: error.to_string().into_boxed_str(),
            },
            Failure::ProvenanceRender(error) => Error::Input {
                code: "config-provenance-encoding",
                reason: error.to_string().into_boxed_str(),
            },
            Failure::CargoEnvironmentOverrides => Error::Input {
                code: "cargo-env-config-unproved",
                reason: "rerun-if-env-changed with Cargo [env] configuration".into(),
            },
        }
    }
}

pub(in crate::input) fn formatted<A: Account>(
    account: &mut A,
    arguments: fmt::Arguments<'_>,
) -> Result<String, A::Error> {
    account.check()?;
    struct Counter(usize);
    impl fmt::Write for Counter {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            self.0 = self.0.checked_add(value.len()).ok_or(fmt::Error)?;
            Ok(())
        }
    }
    let mut counter = Counter(0);
    fmt::write(&mut counter, arguments).map_err(|_| account.failure(Failure::Length))?;
    account.storage(counter.0)?;
    let mut value = String::new();
    value
        .try_reserve_exact(counter.0)
        .map_err(|error| account.failure(Failure::Allocation(error)))?;
    fmt::write(&mut value, arguments).map_err(|_| account.failure(Failure::Render))?;
    Ok(value)
}

pub(super) fn provenance<A: Account>(
    account: &mut A,
    provenance: &[u8; 32],
) -> Result<String, A::Error> {
    account.storage(64)?;
    let mut text = String::new();
    text.try_reserve_exact(64)
        .map_err(|error| account.failure(Failure::ProvenanceAllocation(error)))?;
    for byte in provenance {
        use fmt::Write as _;
        write!(text, "{byte:02x}")
            .map_err(|error| account.failure(Failure::ProvenanceRender(error)))?;
    }
    Ok(text)
}
