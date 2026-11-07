//! Scalar equality against supplied owner inputs, without current proof authority.

use std::path::{Path, PathBuf};

use super::{Budget, Error, Result};
use crate::input::scalar;
use scalar::account::{self, Account};

impl account::sealed::Sealed for Budget<'_> {}

impl Account for Budget<'_> {
    type Error = Error;

    fn check(&self) -> Result<()> {
        Budget::check(self)
    }

    fn storage(&mut self, bytes: usize) -> Result<()> {
        Budget::storage(self, 1, bytes)
    }

    fn package_path(&mut self, _path: &Path) -> Result<()> {
        // The current comparison borrows this path; no path buffer is created.
        self.check()
    }

    fn declared_path(&mut self, root: &Path, value: &str) -> Result<PathBuf> {
        self.check()?;
        super::observe::path::normalized(Path::new(value), root, self)
    }

    fn path_text<'path>(&mut self, _field: &'static str, path: &'path Path) -> Result<&'path str> {
        let value = path
            .to_str()
            .ok_or(Error::Encoding("owner scalar path is not Unicode"))?;
        self.check()?;
        Ok(value)
    }

    fn executable_path<'path>(&mut self, path: &'path Path) -> Result<&'path str> {
        path.to_str()
            .ok_or(Error::Encoding("owner executable path is not Unicode"))
    }

    fn failure(&mut self, failure: account::Failure) -> Error {
        match failure {
            account::Failure::Length => self.overflow(),
            account::Failure::Allocation(error) | account::Failure::ProvenanceAllocation(error) => {
                Budget::failure(self, Error::Allocation(error))
            }
            account::Failure::Render | account::Failure::ProvenanceRender(_) => {
                Error::Encoding("owner scalar formatting failed")
            }
        }
    }
}
