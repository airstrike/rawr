// Copyright 2013 The rust-url developers. MIT OR Apache-2.0.
//! Allocation-free iteration over validated, uniquely positioned insertions.

use super::{decoder, input};

/// A complete internal decode borrowing both its source and admitted scratch.
#[derive(Debug)]
pub(in crate::input::membership::manifest::spec::url) struct Decode<'scratch, 'input> {
    base: input::Input<'input>,
    insertions: &'scratch [decoder::Insertion],
    inserted: usize,
    position: usize,
    length: usize,
}

impl<'scratch, 'input> Decode<'scratch, 'input> {
    pub(super) fn assembled(
        base: input::Input<'input>,
        insertions: &'scratch [decoder::Insertion],
    ) -> Self {
        Self {
            base,
            insertions,
            inserted: 0,
            position: 0,
            length: base.len() + insertions.len(),
        }
    }
}

impl Iterator for Decode<'_, '_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(insertion) = self.insertions.get(self.inserted)
            && insertion.position == self.position
        {
            self.inserted += 1;
            self.position += 1;
            return Some(insertion.c);
        }
        // Every position below length belongs to either base or one insertion.
        // No retry loop is needed when the base is exhausted.
        let c = self.base.next()?;
        self.position += 1;
        Some(c)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len();
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Decode<'_, '_> {
    fn len(&self) -> usize {
        self.length - self.position
    }
}

impl std::iter::FusedIterator for Decode<'_, '_> {}
