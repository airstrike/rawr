use std::fmt;
use std::path::{Component, Path};

use super::{index, Account, Buffer, Current};
use crate::cargo::{universe, Limits};
use crate::input::build::Evidence;
use crate::input::proof::protected::{canonical, normalize, text};
use crate::input::proof::{ProtectedEntry, TreeDirectory, TreeFile};
use crate::input::stored::{Error, Result};

impl Account for Current<'_, '_> {
    type Error = Error;
    type Index<K: Ord, V: Copy> = index::CurrentIndex<K, V>;

    fn check(&mut self) -> Result<()> {
        self.0.check()
    }
    fn comparison(&mut self, bytes: usize) -> Result<()> {
        self.0.decode(bytes)
    }
    fn hash_work(&mut self, bytes: usize) -> Result<()> {
        self.0.hash(bytes)
    }
    fn policy(&mut self, code: &'static str, _reason: impl fmt::Display) -> Error {
        Error::Encoding(code)
    }
    fn limited(&mut self, resource: &'static str, limit: usize, actual: usize) -> Result<()> {
        self.0.limited(resource, limit, actual)
    }
    fn join(&mut self) -> Result<()> {
        self.0.check()
    }
    fn roots<T>(&mut self, count: usize) -> Result<Vec<T>> {
        self.vector(count)
    }
    fn push(&mut self, entries: &mut Buffer<ProtectedEntry>, value: ProtectedEntry) -> Result<()> {
        if entries.values.len() == entries.admitted {
            let capacity = entries
                .admitted
                .checked_mul(2)
                .ok_or_else(|| self.0.overflow())?
                .max(1);
            let bytes = capacity
                .checked_mul(std::mem::size_of::<ProtectedEntry>())
                .ok_or_else(|| self.0.overflow())?;
            self.0.storage(capacity, bytes)?;
            entries
                .values
                .try_reserve_exact(capacity - entries.values.len())
                .map_err(|error| self.0.failure(Error::Allocation(error)))?;
            entries.admitted = capacity;
        }
        self.0.check()?;
        entries.values.push(value);
        Ok(())
    }
    fn token(&mut self, protected: &universe::MetadataProof, id: &str) -> Result<Option<String>> {
        self.0.check()?;
        protected
            .package(id)
            .map(|package| {
                let token = normalize::token(package, self.0)?;
                let token = std::str::from_utf8(&token)
                    .map_err(|_| Error::Encoding("protected token encoding differs"))?;
                text::format(format_args!("{token}"), self.0)
            })
            .transpose()
    }
    fn normalized(
        &mut self,
        value: &str,
        evidence: &Evidence,
        protected: &universe::MetadataProof,
    ) -> Result<String> {
        normalize::value(value, evidence, protected, self.0)
    }
    fn path(&mut self, owner: &str, relative: &Path, _source: &Path) -> Result<String> {
        self.0.check()?;
        for component in relative.components() {
            let Component::Normal(component) = component else {
                return Err(Error::Encoding("protected-build-package-path-shape"));
            };
            component
                .to_str()
                .ok_or(Error::Encoding("protected-build-package-path-encoding"))?;
        }
        text::format(
            format_args!("$PACKAGE/{owner}{}", Components(relative)),
            self.0,
        )
    }
    fn format(&mut self, arguments: fmt::Arguments<'_>) -> Result<String> {
        text::format(arguments, self.0)
    }
    fn boxed(&mut self, value: String) -> Result<Box<str>> {
        text::boxed(value, self.0)
    }
    fn text(&mut self, value: &str) -> Result<Box<str>> {
        self.0.owned_text(value)
    }
    fn strings(&mut self, value: &[Box<str>]) -> Result<Box<[Box<str>]>> {
        let mut output = self.vector(value.len())?;
        for value in value {
            output.push(self.text(value)?);
        }
        self.0.owned_box(output)
    }
    fn bytes(&mut self, value: &[u8]) -> Result<Box<[u8]>> {
        self.0.storage(1, value.len())?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(value.len())
            .map_err(|error| self.0.failure(Error::Allocation(error)))?;
        output.extend_from_slice(value);
        self.0.owned_box(output)
    }
    fn directories(&mut self, value: &[TreeDirectory]) -> Result<Box<[TreeDirectory]>> {
        let mut output = self.vector(value.len())?;
        for value in value {
            output.push(TreeDirectory {
                path: self.text(&value.path)?,
            });
        }
        self.0.owned_box(output)
    }
    fn files(&mut self, value: &[TreeFile]) -> Result<Box<[TreeFile]>> {
        let mut output = self.vector(value.len())?;
        for value in value {
            output.push(TreeFile {
                path: self.text(&value.path)?,
                bytes: value.bytes,
                digest: value.digest,
            });
        }
        self.0.owned_box(output)
    }
    fn canonical(
        &mut self,
        entries: &[ProtectedEntry],
        limits: &Limits,
    ) -> Result<universe::BuildProjection> {
        canonical::current(entries, limits, self.0)
    }
}

impl Current<'_, '_> {
    fn vector<T>(&mut self, count: usize) -> Result<Vec<T>> {
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| self.0.overflow())?;
        self.0.storage(count, bytes)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|error| self.0.failure(Error::Allocation(error)))?;
        Ok(values)
    }
}

struct Components<'a>(&'a Path);

impl fmt::Display for Components<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for component in self.0.components() {
            let Component::Normal(component) = component else {
                return Err(fmt::Error);
            };
            let component = component.to_str().ok_or(fmt::Error)?;
            write!(formatter, "/{component}")?;
        }
        Ok(())
    }
}
