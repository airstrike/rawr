//! A GUI library in iced's shape: an `Element` in a `core` module that
//! the crate root re-exports, a `Text` widget whose `From` impl into
//! `Element` is generic over the message and the theme, as iced's is, and
//! an extension trait that boxes any widget.

pub mod core {
    //! The crate's core types, as `iced_core`.

    pub mod element {
        //! The type-erased widget.

        use crate::Widget;

        /// A widget of any type, as iced's `Element<'a, Message, Theme>`.
        pub struct Element<'a, M, T> {
            widget: Box<dyn Widget<M, T> + 'a>,
        }

        impl<'a, M, T> Element<'a, M, T> {
            pub fn new(widget: impl Widget<M, T> + 'a) -> Self {
                Self {
                    widget: Box::new(widget),
                }
            }

            /// What the widget draws, against `theme`.
            pub fn describe(&self, theme: &T) -> String {
                self.widget.describe(theme)
            }
        }
    }

    pub use element::Element;
}

pub use core::Element;

/// A theme's text styles, as iced's `text::Catalog`.
pub trait Catalog {
    fn ink(&self) -> String;
}

/// A widget, drawn against a theme.
pub trait Widget<M, T> {
    fn describe(&self, theme: &T) -> String;
}

/// Any widget, with the methods iced's `iced::Widget` extension trait
/// gives it, as `.boxed()`.
pub trait WidgetExt<M, T>: Widget<M, T> {
    /// The widget, boxed into an element.
    fn boxed<'a>(self) -> Element<'a, M, T>
    where
        Self: Sized + 'a,
    {
        Element::new(self)
    }
}

impl<M, T, W> WidgetExt<M, T> for W where W: Widget<M, T> {}

/// Some text, as iced's `Text<'a, Theme>`.
pub struct Text<'a, T> {
    content: &'a str,
    size: u32,
    theme: std::marker::PhantomData<T>,
}

/// Text of `content`, as iced's `text`.
pub fn text<'a, T>(content: &'a str) -> Text<'a, T> {
    Text {
        content,
        size: 16,
        theme: std::marker::PhantomData,
    }
}

impl<T> Text<'_, T> {
    pub fn size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }
}

impl<M, T: Catalog> Widget<M, T> for Text<'_, T> {
    fn describe(&self, theme: &T) -> String {
        format!("{}@{} in {}", self.content, self.size, theme.ink())
    }
}

/// As iced's `From<Text<'a, Theme, Renderer>> for Element<'a, Message,
/// Theme, Renderer>`: generic over the message, bounded on the theme.
impl<'a, M, T> From<Text<'a, T>> for Element<'a, M, T>
where
    T: Catalog + 'a,
{
    fn from(text: Text<'a, T>) -> Self {
        Element::new(text)
    }
}
