//! A widget through the core trait of `parts`, in the shape of iced's
//! `Widget`: `Shown` names the crate's theme and renderer as its defaults
//! and holds for every `parts::show::Show` through one blanket impl, as
//! iced_widget's `Widget` does for each `core::Widget`. A `Badge` shows at
//! any theme and renderer, so only a bound fixes them.

use parts::show::Show;

/// A widget at a theme and renderer that default to the crate's.
pub trait Shown<Message, Theme = crate::Theme, Renderer = crate::widgets::Renderer>:
    Show<Message, Theme, Renderer>
{
}

impl<W, Message, Theme, Renderer> Shown<Message, Theme, Renderer> for W where
    W: Show<Message, Theme, Renderer>
{
}

/// A label that shows at any theme and renderer.
pub struct Badge<Message> {
    text: String,
    message: std::marker::PhantomData<Message>,
}

/// A badge showing `text`.
pub fn badge<Message>(text: &str) -> Badge<Message> {
    Badge {
        text: text.to_owned(),
        message: std::marker::PhantomData,
    }
}

impl<Message, Theme, Renderer> Show<Message, Theme, Renderer> for Badge<Message> {
    fn shown(&self) -> String {
        format!("badge {}", self.text)
    }
}
