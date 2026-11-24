//! A paint of a tint, whose `From` impl is apart from the module whose
//! generic code converts into it, as iced_core's `Background` and its
//! `From<Color>` are apart from iced_widget's `container`.

use crate::tinted::Tint;

/// A tint laid on in coats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paint {
    pub tint: Tint,
    pub coats: u8,
}

impl From<Tint> for Paint {
    fn from(tint: Tint) -> Self {
        Self { tint, coats: 2 }
    }
}

impl From<u8> for Paint {
    fn from(gray: u8) -> Self {
        Self {
            tint: Tint::from(gray),
            coats: 1,
        }
    }
}
