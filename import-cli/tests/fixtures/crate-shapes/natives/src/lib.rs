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
/// FnOnce(&mut Borrowed<Buffer>) -> T`) take, or whose closure gives a
/// future parameter, as warp's `Ws::on_upgrade` takes, beside the items a
/// script reaches.
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

        /// A closure that gives a future, as warp's `Ws::on_upgrade` takes.
        pub fn on_upgrade<F, U>(self, func: F) -> u32
        where
            F: FnOnce(Buffer) -> U + Send + 'static,
            U: std::future::Future<Output = ()> + Send + 'static,
        {
            drop(func(self));
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

/// A function generic over `T` that gives an opaque `impl Trait` whose
/// trait has a method generic over its own `T`, as warp's `form<T>()`
/// gives an `impl Filter` with `untuple_one<T>`.
pub mod form {
    pub trait Filter {
        fn value(self) -> u32;

        fn label<T: std::fmt::Display>(self, prefix: T) -> String
        where
            Self: Sized,
        {
            format!("{prefix} {}", self.value())
        }

        /// A type that holds `Self`, as warp's `Filter::and` gives
        /// `And<Self, F>`.
        fn and(self, more: u32) -> And<Self>
        where
            Self: Sized,
        {
            And(self, more)
        }
    }

    pub struct And<F>(F, u32);

    impl<F: Filter> Filter for And<F> {
        fn value(self) -> u32 {
            self.0.value() + self.1
        }
    }

    /// A type whose method gives an opaque type, as image's
    /// `Reader::into_decoder` gives `impl ImageDecoder`.
    pub struct Source(pub u32);

    impl Source {
        pub fn filter(self) -> impl Filter {
            Plain(self.0)
        }
    }

    struct Plain(u32);

    impl Filter for Plain {
        fn value(self) -> u32 {
            self.0
        }
    }

    pub fn form<T: Into<u32>>(value: T) -> impl Filter {
        Plain(value.into())
    }
}

/// Trait impls on a type with a const parameter, as palette's `impl<T>
/// Lookup<u8, T> for ArrayTable<256>` is, and on a type whose defaulted
/// parameter the impl leaves out, as `impl Size for Hsl` is `Hsl<Srgb>`.
pub mod table {
    pub enum Table<const N: usize> {}

    pub struct Srgb;

    pub struct Hsl<S = Srgb>(pub u32, std::marker::PhantomData<S>);

    pub trait Size {
        fn size(&self) -> usize;
    }

    impl Size for Table<4> {
        fn size(&self) -> usize {
            4
        }
    }

    impl Size for Hsl {
        fn size(&self) -> usize {
            self.0 as usize
        }
    }

    impl Srgb {
        /// A table the program reaches, as palette's `Srgb::get_u8_to_f32_lut`
        /// reaches `ArrayTable<256>`.
        pub fn table() -> Option<&'static Table<4>> {
            None
        }
    }

    pub fn hsl(value: u32) -> Hsl {
        Hsl(value, std::marker::PhantomData)
    }
}

/// Types that implement `Deref` to another of the crate's types, to a
/// `String` and to a `Vec`, as many crates' newtypes do: a method or field
/// the type lacks is its target's, and a `&Type` is a `&Target` where one
/// is taken.
pub mod deref {
    use std::ops::{Deref, DerefMut};

    /// `Clone`: rawr reads a crate type's `Deref` target as a borrow
    /// guard's, a copy made inside the call (D554).
    #[derive(Clone)]
    pub struct Counter {
        pub step: i64,
        count: i64,
    }

    impl Counter {
        pub fn count(&self) -> i64 {
            self.count
        }

        pub fn bump(&mut self) {
            self.count += self.step;
        }
    }

    pub struct Labeled {
        pub label: String,
        counter: Counter,
    }

    impl Labeled {
        pub fn new(label: &str, step: i64) -> Self {
            Labeled {
                label: label.to_string(),
                counter: Counter { step, count: 0 },
            }
        }
    }

    impl Deref for Labeled {
        type Target = Counter;

        fn deref(&self) -> &Counter {
            &self.counter
        }
    }

    impl DerefMut for Labeled {
        fn deref_mut(&mut self) -> &mut Counter {
            &mut self.counter
        }
    }

    pub struct Name(String);

    impl Name {
        pub fn new(text: &str) -> Self {
            Name(text.to_string())
        }
    }

    impl Deref for Name {
        type Target = String;

        fn deref(&self) -> &String {
            &self.0
        }
    }

    pub struct Scores(Vec<i64>);

    impl Scores {
        pub fn new(scores: Vec<i64>) -> Self {
            Scores(scores)
        }
    }

    impl Deref for Scores {
        type Target = Vec<i64>;

        fn deref(&self) -> &Vec<i64> {
            &self.0
        }
    }

    pub fn shout(text: &str) -> String {
        text.to_uppercase()
    }

    /// A byte buffer that derefs to `[u8]`, as iced's `Bytes` does.
    #[derive(Clone)]
    pub struct Bytes(Vec<u8>);

    impl Deref for Bytes {
        type Target = [u8];

        fn deref(&self) -> &[u8] {
            &self.0
        }
    }

    #[derive(Clone, Copy)]
    pub struct Size {
        pub width: u32,
        pub height: u32,
    }

    /// A struct with a `Deref` field and a `Copy` one, as iced's
    /// `Screenshot` has.
    pub struct Shot {
        pub rgba: Bytes,
        pub size: Size,
    }

    impl Shot {
        pub fn new() -> Self {
            Shot {
                rgba: Bytes(vec![1, 2, 3, 4]),
                size: Size {
                    width: 2,
                    height: 1,
                },
            }
        }
    }
}

/// A type whose methods a program names as function values, as iced's
/// `pick_list(selected, Theme::ALL, Theme::to_string)` names one: its own
/// methods, its `Display`'s `to_string` and a trait's method, and a function
/// that takes a labeller.
pub mod looks {
    use std::fmt;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Look {
        Light,
        Dark,
    }

    impl Look {
        pub fn all() -> Vec<Look> {
            vec![Look::Light, Look::Dark]
        }

        pub fn name(&self) -> &'static str {
            match self {
                Look::Light => "light",
                Look::Dark => "dark",
            }
        }

        pub fn shade(self) -> u8 {
            match self {
                Look::Light => 200,
                Look::Dark => 30,
            }
        }

        pub fn mix(&self, other: Look) -> u8 {
            self.shade() / 2 + other.shade() / 2
        }

        pub fn flip(&mut self) {
            *self = match self {
                Look::Light => Look::Dark,
                Look::Dark => Look::Light,
            };
        }
    }

    impl fmt::Display for Look {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(match self {
                Look::Light => "Light",
                Look::Dark => "Dark",
            })
        }
    }

    pub trait Tone {
        fn tone(&self) -> i32;
    }

    impl Tone for Look {
        fn tone(&self) -> i32 {
            i32::from(self.shade()) - 100
        }
    }

    /// Each look's label, as a pick list draws its options.
    pub fn labels(looks: &[Look], label: impl Fn(&Look) -> String) -> Vec<String> {
        looks.iter().map(label).collect()
    }
}
