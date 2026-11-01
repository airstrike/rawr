//! Options in the shape of iced's `combo_box::State<T>`, whose
//! `options(&self) -> &[T]` lends the slice it holds for as long as the
//! state's borrow lasts.

use std::cell::Cell;
use std::fmt;

thread_local! {
    static CLONES: Cell<usize> = const { Cell::new(0) };
}

/// How many times a [`Family`] has been cloned, so a program that only
/// borrows the options shows it made no copy.
pub fn clones() -> usize {
    CLONES.with(Cell::get)
}

/// A crate struct an option can be, which counts its clones.
#[derive(Debug, PartialEq)]
pub struct Family {
    pub name: String,
    pub weight: u16,
}

impl Family {
    pub fn new(name: &str, weight: u16) -> Self {
        Self {
            name: name.to_owned(),
            weight,
        }
    }
}

impl Clone for Family {
    fn clone(&self) -> Self {
        CLONES.with(|clones| clones.set(clones.get() + 1));
        Self {
            name: self.name.clone(),
            weight: self.weight,
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.name, self.weight)
    }
}

/// The options a picker offers, as `combo_box::State<T>`.
#[derive(Debug, Clone)]
pub struct Choices<T> {
    options: Vec<T>,
}

impl<T: fmt::Display + Clone> Choices<T> {
    pub fn new(options: Vec<T>) -> Self {
        Self { options }
    }

    /// The options, borrowed from the state.
    pub fn options(&self) -> &[T] {
        &self.options
    }
}
