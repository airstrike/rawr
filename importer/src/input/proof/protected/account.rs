use std::fmt;
use std::path::Path;

use crate::cargo::{universe, Limits};
use crate::input::build::Evidence;
use crate::input::proof::{ProtectedEntry, TreeDirectory, TreeFile};
use crate::input::{stored, Allocation};

mod current;
pub(super) mod index;
mod legacy;

pub(super) struct Legacy<'a>(
    pub(super) &'a mut Allocation,
    pub(super) &'a crate::node::GraphBudget,
);
pub(super) struct Current<'a, 'b>(pub(super) &'a mut stored::Budget<'b>);

pub(super) struct Buffer<T> {
    pub(super) values: Vec<T>,
    pub(super) admitted: usize,
}

impl<T> Buffer<T> {
    pub(super) fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: 0,
        }
    }
}

/// The two actual projection ownership schedules. No caller can add an adapter
/// outside this private module or obtain freshness from a projection alone.
pub(super) trait Account: sealed::Sealed + Sized {
    type Error;
    type Index<K: Ord, V: Copy>: index::Index<K, V, Self>;

    fn check(&mut self) -> Result<(), Self::Error>;
    fn comparison(&mut self, bytes: usize) -> Result<(), Self::Error>;
    fn hash_work(&mut self, bytes: usize) -> Result<(), Self::Error>;
    fn policy(&mut self, code: &'static str, reason: impl fmt::Display) -> Self::Error;
    fn limited(
        &mut self,
        resource: &'static str,
        limit: usize,
        actual: usize,
    ) -> Result<(), Self::Error>;
    fn join(&mut self) -> Result<(), Self::Error>;
    fn roots<T>(&mut self, count: usize) -> Result<Vec<T>, Self::Error>;
    fn push(
        &mut self,
        entries: &mut Buffer<ProtectedEntry>,
        value: ProtectedEntry,
    ) -> Result<(), Self::Error>;
    fn token(
        &mut self,
        protected: &universe::MetadataProof,
        id: &str,
    ) -> Result<Option<String>, Self::Error>;
    fn normalized(
        &mut self,
        value: &str,
        evidence: &Evidence,
        protected: &universe::MetadataProof,
    ) -> Result<String, Self::Error>;
    fn path(&mut self, owner: &str, relative: &Path, source: &Path) -> Result<String, Self::Error>;
    fn format(&mut self, arguments: fmt::Arguments<'_>) -> Result<String, Self::Error>;
    fn boxed(&mut self, value: String) -> Result<Box<str>, Self::Error>;
    fn text(&mut self, value: &str) -> Result<Box<str>, Self::Error>;
    fn strings(&mut self, value: &[Box<str>]) -> Result<Box<[Box<str>]>, Self::Error>;
    fn bytes(&mut self, value: &[u8]) -> Result<Box<[u8]>, Self::Error>;
    fn directories(&mut self, value: &[TreeDirectory])
        -> Result<Box<[TreeDirectory]>, Self::Error>;
    fn files(&mut self, value: &[TreeFile]) -> Result<Box<[TreeFile]>, Self::Error>;
    fn canonical(
        &mut self,
        entries: &[ProtectedEntry],
        limits: &Limits,
    ) -> Result<universe::BuildProjection, Self::Error>;
}

mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for Legacy<'_> {}
impl sealed::Sealed for Current<'_, '_> {}
