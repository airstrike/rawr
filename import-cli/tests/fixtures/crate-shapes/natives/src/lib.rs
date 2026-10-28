//! Shapes of a crate's public API that Rust reaches as written, each in
//! its own module.

/// A builder on a generic type whose methods take `impl Fn(..) + 'a`.
pub mod button {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Status {
        Active,
        Hovered,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Style {
        pub width: u32,
    }

    #[derive(Debug, Clone, Copy, Default)]
    pub struct Theme {
        pub scale: u32,
    }

    pub struct Button<'a, Message> {
        on_press: Box<dyn Fn() -> Message + 'a>,
        style: Box<dyn Fn(&Theme, Status) -> Style + 'a>,
    }

    impl<'a, Message: Clone + 'a> Button<'a, Message> {
        pub fn new(on_press: Message) -> Self {
            Button {
                on_press: Box::new(move || on_press.clone()),
                style: Box::new(|theme: &Theme, _| Style { width: theme.scale }),
            }
        }

        pub fn on_press_with(mut self, on_press: impl Fn() -> Message + 'a) -> Self {
            self.on_press = Box::new(on_press);
            self
        }

        pub fn style(mut self, style: impl Fn(&Theme, Status) -> Style + 'a) -> Self {
            self.style = Box::new(style);
            self
        }

        pub fn press(&self) -> Message {
            (self.on_press)()
        }

        /// The width it has in each status, as a renderer that takes it
        /// draws it.
        pub fn widths(self, theme: &Theme) -> (u32, u32) {
            (
                (self.style)(theme, Status::Active).width,
                (self.style)(theme, Status::Hovered).width,
            )
        }
    }
}

/// An enum whose variant the crate root re-exports, as iced's
/// `pub use Alignment::Center;`.
pub mod alignment {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Alignment {
        Start,
        Center,
        End,
    }

    pub fn describe(alignment: Alignment) -> String {
        format!("{alignment:?}")
    }
}

pub use alignment::Alignment;
pub use alignment::Alignment::Center;

/// Type aliases: one of std's `Result` with fixed arguments, as
/// `iced::Result` is, and one of a generic type with a default parameter,
/// as `iced::Element` is.
pub mod element {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Plain;

    #[derive(Debug, Clone, PartialEq)]
    pub struct Element<'a, Message, Theme> {
        message: Message,
        theme: Theme,
        label: &'a str,
    }

    impl<'a, Message: Clone, Theme: Default> Element<'a, Message, Theme> {
        pub fn new(message: Message) -> Self {
            Element {
                message,
                theme: Theme::default(),
                label: "element",
            }
        }

        pub fn message(&self) -> Message {
            self.message.clone()
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Error {
        Failed(String),
    }
}

pub type Element<'a, Message, Theme = element::Plain> = element::Element<'a, Message, Theme>;

pub type Result = std::result::Result<(), element::Error>;

pub fn run(succeed: bool) -> Result {
    if succeed {
        Ok(())
    } else {
        Err(element::Error::Failed("no".to_string()))
    }
}

/// A trait's provided method, which a type gets by implementing the trait,
/// called as a method and through the trait's path, as iced's `column!`
/// calls `Widget::_boxed`.
pub mod widget {
    pub use crate::base::*;

    /// Generic over its message, as iced's `Widget<Message, Theme,
    /// Renderer>` is.
    pub trait Widget<Message> {
        fn width(&self) -> u32;

        fn boxed<'a>(self) -> Boxed<'a, Message>
        where
            Self: Sized + 'a,
        {
            Boxed {
                width: self.width(),
                message: std::marker::PhantomData,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Boxed<'a, Message> {
        pub width: u32,
        message: std::marker::PhantomData<&'a Message>,
    }

    impl<Message> Boxed<'_, Message> {
        pub fn width(&self) -> u32 {
            self.width
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Space {
        pub width: u32,
    }

    impl<Message> Widget<Message> for Space {
        fn width(&self) -> u32 {
            self.width
        }
    }
}

/// A module and a function of the same name, as iced_widget's `text`
/// module and its `text` helper, which a glob re-export brings in, and
/// which `widget` re-exports again with a glob, as `iced::widget` does.
pub mod base {
    pub mod text {
        #[derive(Debug, Clone, PartialEq)]
        pub struct Text {
            content: String,
            size: u32,
        }

        impl Text {
            pub fn new(content: impl ToString) -> Self {
                Text {
                    content: content.to_string(),
                    size: 16,
                }
            }

            pub fn size(mut self, size: u32) -> Self {
                self.size = size;
                self
            }

            pub fn describe(&self) -> String {
                format!("{} at {}", self.content, self.size)
            }
        }
    }

    pub mod helpers {
        pub fn text(content: impl ToString) -> super::text::Text {
            super::text::Text::new(content)
        }
    }

    pub use helpers::*;
}

/// Functions whose callable parameter is a named type parameter that no
/// script closure fills, as tokio's `MappedMutexGuard::map` (`F:
/// FnOnce(&mut T) -> &mut U`) and cosmic-text's `with_buffer_mut` (`F:
/// FnOnce(&mut Borrowed<Buffer>) -> T`) take, beside the items a script
/// reaches.
pub mod guard {
    pub struct Guard<'a, T> {
        value: &'a mut T,
    }

    impl<'a, T> Guard<'a, T> {
        pub fn new(value: &'a mut T) -> Self {
            Guard { value }
        }

        pub fn map<U, F>(this: Self, f: F) -> Guard<'a, U>
        where
            F: FnOnce(&mut T) -> &mut U,
        {
            Guard {
                value: f(this.value),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Buffer {
        pub lines: u32,
    }

    impl Buffer {
        pub fn new(lines: u32) -> Self {
            Buffer { lines }
        }

        pub fn lines(&self) -> u32 {
            self.lines
        }
    }

    pub struct Borrowed<'a, E> {
        inner: &'a mut E,
    }

    impl<'a, E> Borrowed<'a, E> {
        pub fn with_buffer_mut<F: FnOnce(&mut Borrowed<Buffer>) -> T, T>(&mut self, f: F) -> T {
            let _ = &self.inner;
            let mut buffer = Buffer::default();
            f(&mut Borrowed { inner: &mut buffer })
        }
    }
}
