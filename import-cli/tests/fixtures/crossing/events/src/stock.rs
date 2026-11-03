//! A result that borrows its slice, of a trait that can't be a trait object
//! (#849): `Stocked` has a generic method, a method that gives `Self`, an
//! associated type, an associated const and a supertrait. Each item counts
//! the times it was shown, and each result says when it drops, so the
//! output tells the one value of each item and each result from a copy.

use std::fmt::Display;

use crate::shelf::Item;

/// What every stock can say of itself.
pub trait Named {
    fn label(&self) -> String;
}

/// A stock of items.
pub trait Stocked: Named {
    /// What `first` gives.
    type Item;

    /// The most items a stock shows.
    const LIMIT: usize;

    /// The first item past the skipped ones.
    fn first(&self) -> Self::Item;

    /// The count of items under a tag of any type.
    fn tag<T: Display>(&self, tag: T) -> String;

    /// The same items, one more skipped.
    fn again(&self) -> Self;

    /// The trait's limit, as a method.
    fn limit(&self) -> usize {
        Self::LIMIT
    }
}

struct Stock<'a> {
    items: &'a [Item],
    skip: usize,
}

impl Named for Stock<'_> {
    fn label(&self) -> String {
        let names: Vec<String> = self
            .items
            .iter()
            .skip(self.skip)
            .take(Self::LIMIT)
            .map(|item| {
                item.count();
                item.name()
            })
            .collect();
        format!("{}[{}]", self.skip, names.join(" "))
    }
}

impl Stocked for Stock<'_> {
    type Item = String;

    const LIMIT: usize = 2;

    fn first(&self) -> String {
        self.items
            .get(self.skip)
            .map_or_else(String::new, Item::name)
    }

    fn tag<T: Display>(&self, tag: T) -> String {
        format!(
            "{tag}{}",
            self.items.len() - self.skip.min(self.items.len())
        )
    }

    fn again(&self) -> Self {
        Stock {
            items: self.items,
            skip: self.skip + 1,
        }
    }
}

impl Drop for Stock<'_> {
    fn drop(&mut self) {
        println!("drop {}", self.skip);
    }
}

/// The items as a stock, borrowed for as long as it lives.
pub fn stock<'a>(items: &'a [Item]) -> impl Stocked<Item = String> + 'a {
    Stock { items, skip: 0 }
}
