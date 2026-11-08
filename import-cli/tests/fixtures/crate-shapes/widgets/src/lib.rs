//! A widget crate a component crate builds on, as iced_widget is:
//! `Text<'a, Theme, Renderer = Plain>`, generic over the theme, with a
//! renderer its users leave to the default, an `Element` any text converts
//! into, and `TextLike`, of the same shape, that no component comes from.

use std::marker::PhantomData;

pub struct Plain;

pub struct Text<'a, Theme, Renderer = Plain> {
    content: String,
    size: u32,
    marker: PhantomData<(&'a str, Theme, Renderer)>,
}

impl<'a, Theme, Renderer> Text<'a, Theme, Renderer> {
    pub fn size(self, size: u32) -> Self {
        Text { size, ..self }
    }

    pub fn describe(&self) -> String {
        format!("{} {}", self.content, self.size)
    }
}

pub fn text<'a, Theme>(content: &str) -> Text<'a, Theme> {
    Text {
        content: content.to_owned(),
        size: 16,
        marker: PhantomData,
    }
}

/// Any widget, which text converts into at any theme, as iced's `Element`
/// is.
pub struct Element<'a, Message, Theme, Renderer = Plain> {
    pub describe: String,
    marker: PhantomData<(&'a str, Message, Theme, Renderer)>,
}

impl<'a, Message, Theme, Renderer> From<Text<'a, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(text: Text<'a, Theme, Renderer>) -> Self {
        Element {
            describe: text.describe(),
            marker: PhantomData,
        }
    }
}

impl<'a, Message, Theme, Renderer> Element<'a, Message, Theme, Renderer> {
    pub fn described(describe: String) -> Self {
        Element {
            describe,
            marker: PhantomData,
        }
    }
}

pub struct TextLike<'a, Theme> {
    pub content: &'a str,
    marker: PhantomData<Theme>,
}

pub fn text_like<'a, Theme>(content: &'a str) -> TextLike<'a, Theme> {
    TextLike {
        content,
        marker: PhantomData,
    }
}

/// What a derive macro's expansion names `std::error::Error` by, as
/// thiserror's `::thiserror::__private::Error` re-exports core's.
#[doc(hidden)]
pub mod __private {
    #[doc(hidden)]
    pub use core::error::Error;
}
