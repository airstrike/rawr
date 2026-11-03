//! A result that borrows the slice it was made from (#846): `view` and
//! `pick` give an `impl Show + 'a` that reads the items each time it is
//! shown, so the items stay borrowed for as long as it lives. An `Item`
//! has no `Clone`, and each counts the times it was shown, so the counts
//! tell the one value of each item from a copy.

use std::cell::Cell;

pub struct Item {
    name: String,
    shown: Cell<u32>,
}

impl Item {
    pub fn new(name: &str) -> Self {
        Item {
            name: name.to_string(),
            shown: Cell::new(0),
        }
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Counts one more show of this item.
    pub(crate) fn count(&self) {
        self.shown.set(self.shown.get() + 1);
    }

    /// How many times a result showed this item.
    pub fn shown(&self) -> u32 {
        self.shown.get()
    }
}

pub trait Show {
    fn show(&self) -> String;
}

/// A result covariant in `'a`.
struct View<'a> {
    items: &'a [Item],
}

impl Show for View<'_> {
    fn show(&self) -> String {
        let names: Vec<String> = self
            .items
            .iter()
            .map(|item| {
                item.shown.set(item.shown.get() + 1);
                item.name.clone()
            })
            .collect();
        format!("[{}]", names.join(" "))
    }
}

/// The items as one view, borrowed for as long as the view lives.
pub fn view<'a>(items: &'a [Item]) -> impl Show + 'a {
    View { items }
}

/// A result invariant in `'a`: it moves on to the next item at each show.
struct Picked<'a> {
    items: &'a [Item],
    at: Cell<Option<&'a Item>>,
}

impl Show for Picked<'_> {
    fn show(&self) -> String {
        let next = match self.at.get() {
            None => self.items.first(),
            Some(at) => {
                let index = self.items.iter().position(|item| std::ptr::eq(item, at));
                index.and_then(|index| self.items.get(index + 1))
            }
        };
        self.at.set(next);
        match next {
            Some(item) => {
                item.shown.set(item.shown.get() + 1);
                format!("<{}>", item.name)
            }
            None => "<>".to_string(),
        }
    }
}

/// A cursor over the items, borrowed for as long as it lives.
pub fn pick<'a>(items: &'a [Item]) -> impl Show + 'a {
    Picked {
        items,
        at: Cell::new(None),
    }
}

/// The items' names and how many times each was shown.
pub fn names(items: &[Item]) -> String {
    let names: Vec<String> = items
        .iter()
        .map(|item| format!("{}:{}", item.name, item.shown.get()))
        .collect();
    names.join(",")
}

/// A trait whose generic method keeps it from being a trait object.
pub trait Tagged {
    fn tag<T: std::fmt::Display>(&self, tag: T) -> String;
}

impl Tagged for View<'_> {
    fn tag<T: std::fmt::Display>(&self, tag: T) -> String {
        format!("{tag}{}", self.items.len())
    }
}

/// The items under a tag, borrowed for as long as the result lives.
pub fn tagged<'a>(items: &'a [Item]) -> impl Tagged + 'a {
    View { items }
}
