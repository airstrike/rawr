//! iced's `border`: a function named as the `tint` module and macro are.

use crate::Tint;

/// An outline, as iced's `Border`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Outline {
    /// Its tint.
    pub tint: Tint,
    /// Its width.
    pub width: u32,
}

/// An outline of `tint`, as iced's `border::color`.
pub fn tint(tint: impl Into<Tint>) -> Outline {
    Outline::default().tint(tint)
}

/// An outline `width` wide, as iced's `border::width`.
pub fn wide(width: u32) -> Outline {
    Outline::default().width(width)
}

impl Outline {
    /// The outline with `tint`.
    pub fn tint(self, tint: impl Into<Tint>) -> Self {
        Self {
            tint: tint.into(),
            ..self
        }
    }

    /// The outline `width` wide.
    pub fn width(self, width: u32) -> Self {
        Self { width, ..self }
    }
}
