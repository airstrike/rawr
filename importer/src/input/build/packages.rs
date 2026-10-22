//! Ordered historical package inventory with unchanged cold map ownership.

use std::collections::{btree_map, BTreeMap};
use std::slice;

use super::Package;
use crate::input::stored::{Error, Result};

#[cfg_attr(test, derive(Clone))]
pub(in crate::input) struct Table {
    storage: Storage,
}

#[cfg_attr(test, derive(Clone))]
enum Storage {
    Cold(BTreeMap<Box<str>, Package>),
    Decoded(Box<[(Box<str>, Package)]>),
}

pub(in crate::input) enum Iter<'table> {
    Cold(btree_map::Iter<'table, Box<str>, Package>),
    Decoded(slice::Iter<'table, (Box<str>, Package)>),
}

impl Table {
    /// Move the map produced by the existing cold parser without reallocation.
    pub(in crate::input) fn from_cold(packages: BTreeMap<Box<str>, Package>) -> Self {
        Self {
            storage: Storage::Cold(packages),
        }
    }

    /// Transfer storage already admitted by the decoder. Reject noncanonical
    /// order without sorting, copying, allocating map nodes or conferring proof.
    pub(in crate::input) fn from_decoded(packages: Box<[(Box<str>, Package)]>) -> Result<Self> {
        if packages.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err(Error::Encoding(
                "Build package inventory is not strictly ordered",
            ));
        }
        Ok(Self {
            storage: Storage::Decoded(packages),
        })
    }

    pub(in crate::input) fn get(&self, id: &str) -> Option<&Package> {
        match &self.storage {
            Storage::Cold(packages) => packages.get(id),
            Storage::Decoded(packages) => packages
                .binary_search_by(|(key, _)| key.as_ref().cmp(id))
                .ok()
                .map(|index| &packages[index].1),
        }
    }

    pub(in crate::input) fn contains_key(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    pub(in crate::input) fn len(&self) -> usize {
        match &self.storage {
            Storage::Cold(packages) => packages.len(),
            Storage::Decoded(packages) => packages.len(),
        }
    }

    #[cfg(test)]
    pub(in crate::input) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(in crate::input) fn iter(&self) -> Iter<'_> {
        match &self.storage {
            Storage::Cold(packages) => Iter::Cold(packages.iter()),
            Storage::Decoded(packages) => Iter::Decoded(packages.iter()),
        }
    }

    #[cfg(test)]
    pub(in crate::input) fn first_key_value(&self) -> Option<(&str, &Package)> {
        self.iter().next().map(|(key, value)| (&**key, value))
    }

    #[cfg(test)]
    pub(in crate::input) fn get_mut(&mut self, id: &str) -> Option<&mut Package> {
        match &mut self.storage {
            Storage::Cold(packages) => packages.get_mut(id),
            Storage::Decoded(packages) => packages
                .binary_search_by(|(key, _)| key.as_ref().cmp(id))
                .ok()
                .map(|index| &mut packages[index].1),
        }
    }

    #[cfg(test)]
    pub(in crate::input) fn insert(&mut self, id: Box<str>, package: Package) -> Option<Package> {
        match &mut self.storage {
            Storage::Cold(packages) => packages.insert(id, package),
            Storage::Decoded(_) => panic!("decoded package fixtures are immutable"),
        }
    }
}

impl<'table> IntoIterator for &'table Table {
    type Item = (&'table Box<str>, &'table Package);
    type IntoIter = Iter<'table>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'table> Iterator for Iter<'table> {
    type Item = (&'table Box<str>, &'table Package);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Cold(iter) => iter.next(),
            Self::Decoded(iter) => iter.next().map(|(id, package)| (id, package)),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let count = self.len();
        (count, Some(count))
    }
}

impl ExactSizeIterator for Iter<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Cold(iter) => iter.len(),
            Self::Decoded(iter) => iter.len(),
        }
    }
}

#[cfg(test)]
impl Default for Table {
    fn default() -> Self {
        Self::from_cold(BTreeMap::new())
    }
}

#[cfg(test)]
impl<const N: usize> From<[(Box<str>, Package); N]> for Table {
    fn from(value: [(Box<str>, Package); N]) -> Self {
        Self::from_cold(BTreeMap::from(value))
    }
}

#[cfg(test)]
impl From<BTreeMap<Box<str>, Package>> for Table {
    fn from(value: BTreeMap<Box<str>, Package>) -> Self {
        Self::from_cold(value)
    }
}

#[cfg(test)]
mod tests;
