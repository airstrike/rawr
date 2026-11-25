//! A crate's concrete holder of any `Shown`, in the shape of iced's
//! `Element`: its generic constructor boxes the item as the trait's
//! object, and its concrete code calls it through that object.

use crate::framed::Shown;

/// An item held as its trait object.
pub struct Held<'a>(Box<dyn Shown + 'a>);

impl<'a> Held<'a> {
    /// `item`, held as its trait object.
    pub fn new(item: impl Shown + 'a) -> Self {
        Self(Box::new(item))
    }

    /// The item's text, in parentheses.
    #[must_use]
    pub fn shown(&self) -> String {
        format!("({})", self.0.shown())
    }
}
