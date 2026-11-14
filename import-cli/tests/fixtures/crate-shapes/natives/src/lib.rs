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

    /// A generic type's `&mut self` method that gives an opaque iterator
    /// borrowing it, as polars' `ChunkedArray::downcast_iter_mut` does. Its
    /// opaque type has no visibility in the crate's metadata.
    pub struct Holder<T>(pub Vec<T>);

    impl<T> Holder<T> {
        pub fn items_mut(&mut self) -> impl Iterator<Item = &mut T> + '_ {
            self.0.iter_mut()
        }
    }

    /// A generic type's method that gives an opaque iterator of the values
    /// it owns, as `Vec::into_iter` behind `impl Iterator<Item = T>` does.
    impl<T> Holder<T> {
        pub fn into_items(self) -> impl Iterator<Item = T> {
            self.0.into_iter()
        }
    }

    /// A setting of a value, as swash's `Setting<T>`, whose impl for each
    /// value type gives an opaque iterator by the same name: two opaque
    /// types, each with its own `next`.
    #[derive(Debug)]
    pub struct Setting<T>(pub T);

    impl Setting<u16> {
        pub fn parse_list(text: &str) -> impl Iterator<Item = Self> + '_ + Clone {
            text.split(',')
                .filter_map(|part| part.trim().parse().ok().map(Setting))
        }
    }

    impl Setting<f32> {
        pub fn parse_list(text: &str) -> impl Iterator<Item = Self> + '_ + Clone {
            text.split(',')
                .filter_map(|part| part.trim().parse().ok().map(Setting))
        }
    }

    /// A function's opaque iterator, as `(0..n).rev()` behind `impl
    /// Iterator<Item = u32>` gives.
    pub fn countdown(from: u32) -> impl Iterator<Item = u32> {
        (1..=from).rev()
    }

    /// A function's opaque iterator that borrows the text it is given, as
    /// iced's `markdown::parse(&str) -> impl Iterator<Item = Item> + '_`.
    pub fn settings(text: &str) -> impl Iterator<Item = Setting<u16>> + '_ {
        text.split(',')
            .filter_map(|part| part.trim().parse().ok().map(Setting))
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

/// Items named as words the old engine read as keywords, which are names
/// in Rust: a function `print`, a module `import`, a field `this` and a
/// method `call`.
pub mod names {
    pub fn print(text: &str) -> String {
        format!("printed {text}")
    }

    pub mod import {
        pub fn switch(on: bool) -> bool {
            !on
        }
    }

    pub struct Handle {
        pub this: u32,
    }

    impl Handle {
        pub fn call(&self, step: u32) -> u32 {
            self.this + step
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
        /// Every look, as iced's `Theme::ALL`.
        pub const ALL: [Look; 2] = [Look::Light, Look::Dark];

        /// Every look's name, as a slice of strings (#1626).
        pub const NAMES: &'static [&'static str] = &["light", "dark"];

        pub fn all() -> Vec<Look> {
            vec![Look::Light, Look::Dark]
        }

        /// Every look, as an array.
        pub fn both() -> [Look; 2] {
            Look::ALL
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

/// A type with operators on both sides of a number, as iced's `Radians`:
/// `impl Mul<Radians> for f32` is the crate's impl of a std trait for a
/// std type, beside the type's own `Mul<f32>`, `Mul` and `Rem`.
pub mod angle {
    use std::ops::{Add, AddAssign, Div, Mul, Rem, Sub};

    #[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
    pub struct Turn(pub f32);

    impl Turn {
        pub const HALF: Self = Self(0.5);
    }

    impl Mul<Turn> for f32 {
        type Output = Turn;

        fn mul(self, rhs: Turn) -> Self::Output {
            Turn(self * rhs.0)
        }
    }

    impl Mul<f32> for Turn {
        type Output = Turn;

        fn mul(self, rhs: f32) -> Turn {
            Turn(self.0 * rhs * 10.0)
        }
    }

    impl Mul for Turn {
        type Output = Turn;

        fn mul(self, rhs: Turn) -> Turn {
            Turn(self.0 * rhs.0 * 100.0)
        }
    }

    impl Rem for Turn {
        type Output = Turn;

        fn rem(self, rhs: Turn) -> Turn {
            Turn(self.0 % rhs.0)
        }
    }

    impl Add for Turn {
        type Output = Turn;

        fn add(self, rhs: Turn) -> Turn {
            Turn(self.0 + rhs.0)
        }
    }

    /// A difference that is a number, not a turn.
    impl Sub<Turn> for f64 {
        type Output = f64;

        fn sub(self, rhs: Turn) -> Self::Output {
            self - f64::from(rhs.0)
        }
    }

    impl Div<Turn> for i32 {
        type Output = Turn;

        fn div(self, rhs: Turn) -> Turn {
            Turn(self as f32 / rhs.0)
        }
    }

    /// A number a turn changes in place, as `y += angle`.
    impl AddAssign<Turn> for f32 {
        fn add_assign(&mut self, rhs: Turn) {
            *self += rhs.0 * 2.0;
        }
    }
}

/// A method whose own lifetime its output borrows a format at, as jiff's
/// `DateTime::strftime<'f, F>`.
pub mod lifetimes {
    use std::fmt;

    /// A time of day, as jiff's `civil::DateTime`.
    #[derive(Clone, Copy, Debug)]
    pub struct Clock {
        hour: u8,
        minute: u8,
    }

    impl Clock {
        pub fn new(hour: u8, minute: u8) -> Self {
            Clock { hour, minute }
        }

        /// As jiff's `strftime`: the output borrows the format at an
        /// early-bound lifetime, which `F: 'f` makes early.
        pub fn strftime<'f, F: 'f + ?Sized + AsRef<[u8]>>(&self, format: &'f F) -> Shown<'f> {
            Shown {
                format: format.as_ref(),
                clock: *self,
            }
        }
    }

    /// A clock with the format it borrows, as jiff's `strtime::Display<'f>`.
    #[derive(Debug)]
    pub struct Shown<'f> {
        format: &'f [u8],
        clock: Clock,
    }

    impl fmt::Display for Shown<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let mut directive = false;
            for &byte in self.format {
                match (directive, byte) {
                    (false, b'%') => directive = true,
                    (false, byte) => write!(f, "{}", char::from(byte))?,
                    (true, b'H') => {
                        write!(f, "{:02}", self.clock.hour)?;
                        directive = false;
                    }
                    (true, b'M') => {
                        write!(f, "{:02}", self.clock.minute)?;
                        directive = false;
                    }
                    (true, byte) => {
                        write!(f, "%{}", char::from(byte))?;
                        directive = false;
                    }
                }
            }
            Ok(())
        }
    }
}

/// A generic type whose parameter's bound only one float type meets, as
/// iced's `Animation<T: Float>` is, whose `Float` is lilt's
/// `FloatRepresentable`, implemented for `bool` and `f32` alone.
pub mod animation {
    pub trait Float {
        fn float_value(&self) -> f32;
    }

    impl Float for bool {
        fn float_value(&self) -> f32 {
            if *self {
                1.0
            } else {
                0.0
            }
        }
    }

    impl Float for f32 {
        fn float_value(&self) -> f32 {
            *self
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Animation<T: Float + Copy> {
        state: T,
    }

    impl<T: Float + Copy> Animation<T> {
        pub fn new(state: T) -> Self {
            Self { state }
        }

        pub fn slow(self) -> Self {
            self
        }

        /// The Rust name of the state's type, which tells `f32` from `f64`.
        pub fn kind(&self) -> &'static str {
            std::any::type_name::<T>()
        }

        pub fn value(&self) -> f32 {
            self.state.float_value()
        }
    }
}

/// A random source that isn't `Send`, as rand's `ThreadRng` holds an `Rc`,
/// and a distribution that samples it through `&mut R`, as rand's
/// `Distribution::sample` does.
pub mod random {
    use std::cell::Cell;
    use std::rc::Rc;

    pub trait Source {
        fn draw(&mut self) -> u32;
    }

    #[derive(Clone, Debug)]
    pub struct Thread {
        state: Rc<Cell<u32>>,
    }

    pub fn rng() -> Thread {
        Thread {
            state: Rc::new(Cell::new(7)),
        }
    }

    impl Source for Thread {
        fn draw(&mut self) -> u32 {
            let next = self.state.get().wrapping_mul(31).wrapping_add(11) % 1000;
            self.state.set(next);
            next
        }
    }

    impl Thread {
        pub fn below(&mut self, bound: u32) -> u32 {
            self.draw() % bound
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Weighted {
        weights: [u32; 3],
    }

    impl Weighted {
        pub fn new(first: u32, second: u32, third: u32) -> Self {
            Self {
                weights: [first, second, third],
            }
        }

        pub fn sample<R: Source + ?Sized>(&self, rng: &mut R) -> usize {
            let total: u32 = self.weights.iter().sum();
            let mut pick = rng.draw() % total;
            for (index, weight) in self.weights.iter().enumerate() {
                if pick < *weight {
                    return index;
                }
                pick -= weight;
            }
            self.weights.len() - 1
        }

        /// The same through a `&mut` of the concrete source.
        pub fn sample_thread(&self, rng: &mut Thread) -> usize {
            self.sample(rng)
        }
    }
}

/// A trait whose method takes a `&mut` of a type with a lifetime, as
/// iced's `Widget::update` takes `shell: &mut Shell<'_, Message>`: native
/// code lends the shell for the call, and it isn't `Clone`.
pub mod shell {
    pub struct Shell<'a, M> {
        messages: &'a mut Vec<M>,
        redraws: u32,
    }

    impl<M> Shell<'_, M> {
        pub fn request_redraw(&mut self) {
            self.redraws += 1;
        }

        pub fn is_empty(&self) -> bool {
            self.messages.is_empty()
        }
    }

    pub trait Widget<M> {
        fn update(&mut self, cursor: u32, shell: &mut Shell<'_, M>);
    }

    /// Each cursor's update, with one shell lent to them all: how many
    /// redraws they asked for.
    pub fn drive<W: Widget<u32>>(widget: &mut W, cursors: &[u32]) -> u32 {
        let mut messages = Vec::new();
        let mut shell = Shell {
            messages: &mut messages,
            redraws: 0,
        };
        for cursor in cursors {
            widget.update(*cursor, &mut shell);
        }
        shell.redraws
    }
}

pub mod weighted {
    use crate::random::Source;

    /// A number a weight may be, as rand's `SampleUniform`.
    pub trait Uniform: Copy + PartialOrd + std::ops::AddAssign + Default {
        fn below(self, draw: u32) -> Self;
    }

    impl Uniform for i32 {
        fn below(self, draw: u32) -> Self {
            (draw % 1000) as i32 % self
        }
    }

    impl Uniform for u32 {
        fn below(self, draw: u32) -> Self {
            draw % self
        }
    }

    impl Uniform for f64 {
        fn below(self, draw: u32) -> Self {
            f64::from(draw % 1000) / 1000.0 * self
        }
    }

    /// A value that lends a `B`, as rand's `SampleBorrow`: the number
    /// itself and a reference to it.
    pub trait SampleBorrow<B> {
        fn borrow(&self) -> &B;
    }

    impl<B: Uniform> SampleBorrow<B> for B {
        fn borrow(&self) -> &B {
            self
        }
    }

    impl<B: Uniform> SampleBorrow<B> for &B {
        fn borrow(&self) -> &B {
            self
        }
    }

    #[derive(Debug)]
    pub struct Error;

    /// What a distribution gives, as rand's `Distribution<T>`.
    pub trait Distribution<T> {
        fn sample<R: Source + ?Sized>(&self, rng: &mut R) -> T;
    }

    /// An index drawn by weight, as rand's `WeightedIndex<X>`.
    #[derive(Clone, Debug)]
    pub struct WeightedIndex<X: Uniform> {
        cumulative: Vec<X>,
        total: X,
    }

    impl<X: Uniform> WeightedIndex<X> {
        /// `X` is fixed only by `I::Item: SampleBorrow<X>`, as rand's
        /// `WeightedIndex::new` has it.
        pub fn new<I>(weights: I) -> Result<WeightedIndex<X>, Error>
        where
            I: IntoIterator,
            I::Item: SampleBorrow<X>,
        {
            let mut total = X::default();
            let mut cumulative = Vec::new();
            for weight in weights {
                total += *weight.borrow();
                cumulative.push(total);
            }
            if cumulative.is_empty() {
                return Err(Error);
            }
            Ok(Self { cumulative, total })
        }

        /// The Rust name of the weights' type, which tells `i32` from `i64`.
        pub fn kind(&self) -> &'static str {
            std::any::type_name::<X>()
        }
    }

    impl<X: Uniform> Distribution<usize> for WeightedIndex<X> {
        fn sample<R: Source + ?Sized>(&self, rng: &mut R) -> usize {
            let pick = self.total.below(rng.draw());
            self.cumulative
                .iter()
                .position(|bound| pick < *bound)
                .unwrap_or(self.cumulative.len() - 1)
        }
    }
}

/// `form` again under another module, as ratatui's `macros` re-exports
/// `ratatui_core`: `Holder` and its methods have a second path, and so does
/// the opaque iterator `into_items` gives, whose `next` is its member.
pub mod again {
    pub use crate::form;
}

/// A function's opaque `impl Number` whose trait's supertrait binds an
/// associated type to `Self`, as num-traits' `Num: NumOps<Self, Self>`
/// binds `Add::Output`: the opaque type's `Output` is the opaque type.
pub mod numbers {
    use std::ops::Add;

    pub trait Number: Add<Output = Self> + Sized {
        fn get(&self) -> u32;
    }

    struct Seven(u32);

    impl Add for Seven {
        type Output = Self;

        fn add(self, other: Self) -> Self {
            Seven(self.0 + other.0)
        }
    }

    impl Number for Seven {
        fn get(&self) -> u32 {
            self.0
        }
    }

    pub fn seven() -> impl Number {
        Seven(7)
    }
}

/// A generic trait's provided method that gives an opaque `impl Total`,
/// whose supertrait binds `Add::Output` to `Self`, reached as a member of
/// a generic type and of a plain one: the opaque type's `Output` is the
/// opaque type, not the type the method is a member of (#1278).
pub mod doubling {
    use std::ops::Add;

    pub trait Total: Add<Output = Self> + Sized {
        fn get(&self) -> u32;
    }

    struct Sum(u32);

    impl Add for Sum {
        type Output = Self;

        fn add(self, other: Self) -> Self {
            Sum(self.0 + other.0)
        }
    }

    impl Total for Sum {
        fn get(&self) -> u32 {
            self.0
        }
    }

    pub trait Counted<M>: Sized {
        fn count(&self) -> u32;

        fn doubled(self) -> impl Total {
            Sum(self.count() * 2)
        }
    }

    pub struct Tally(pub u32);

    impl Counted<u8> for Tally {
        fn count(&self) -> u32 {
            self.0
        }
    }

    pub struct Bag<T>(pub Vec<T>);

    impl<T> Counted<T> for Bag<T> {
        fn count(&self) -> u32 {
            self.0.len() as u32
        }
    }
}

pub mod pages {
    /// One block of a page, whose group holds more blocks of its own type.
    #[derive(Debug)]
    pub enum Block {
        Md(&'static str),
        Example {
            id: &'static str,
            description: Option<&'static str>,
        },
        Group {
            title: &'static str,
            children: &'static [Self],
        },
    }

    /// How many blocks `blocks` holds, each group's children counted.
    pub fn count(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|block| match block {
                Block::Group { children, .. } => 1 + count(children),
                Block::Md(_) | Block::Example { .. } => 1,
            })
            .sum()
    }

    /// What a block is called: a group's title, an example's id, or its
    /// text.
    pub fn heading(block: &Block) -> &'static str {
        match block {
            Block::Md(text) => text,
            Block::Example { id, .. } => id,
            Block::Group { title, .. } => title,
        }
    }
}

/// futures_util's forwarding impls: `Wrap<S>`'s `Sink<Item>` declares an
/// `Item` its self type doesn't name, and `Immediate<F>`'s `Stream`, as
/// futures_util's `PollImmediate`, binds `<F as Future>::Output` to a `T`
/// only its where clause fixes. Each future the crate declares is a case
/// of that bound, and `Forward<S, A>`'s `Output` names an `E` that only its
/// own where clause fixes, as futures_util's `Forward` and `SendAll` do
/// (#1295).
pub mod forwarding {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    pub trait Sink<Item> {
        type Error;

        fn send(&mut self, item: Item) -> Result<(), Self::Error>;
    }

    pub trait Stream {
        type Item;

        fn next(&mut self) -> Option<Self::Item>;
    }

    pub struct Ready(pub u32);

    impl Future for Ready {
        type Output = u32;

        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Ready(self.0)
        }
    }

    pub struct Drain(pub u32);

    impl Sink<u32> for Drain {
        type Error = ();

        fn send(&mut self, item: u32) -> Result<(), ()> {
            self.0 += item;
            Ok(())
        }
    }

    pub struct Forward<S, A>(pub S, pub Option<A>);

    impl<S, A, E> Future for Forward<S, A>
    where
        S: Sink<A, Error = E>,
    {
        type Output = Result<(), E>;

        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            // SAFETY: neither field is structurally pinned.
            let this = unsafe { self.get_unchecked_mut() };
            Poll::Ready(match this.1.take() {
                Some(item) => this.0.send(item),
                None => Ok(()),
            })
        }
    }

    pub struct Wrap<S>(pub S);

    impl<S> Wrap<S> {
        pub fn new(inner: S) -> Self {
            Wrap(inner)
        }

        pub fn depth(&self) -> u32 {
            1
        }

        pub fn forward<A>(self, item: A) -> Forward<Self, A> {
            Forward(self, Some(item))
        }
    }

    impl<S, Item> Sink<Item> for Wrap<S>
    where
        S: Sink<Item>,
    {
        type Error = S::Error;

        fn send(&mut self, item: Item) -> Result<(), Self::Error> {
            self.0.send(item)
        }
    }

    pub struct Immediate<F>(pub F);

    impl<F> Immediate<F> {
        pub fn new(future: F) -> Self {
            Immediate(future)
        }

        pub fn polls(&self) -> u32 {
            2
        }
    }

    impl<T, F> Stream for Immediate<F>
    where
        F: Future<Output = T>,
    {
        type Item = T;

        fn next(&mut self) -> Option<T> {
            None
        }
    }
}

/// Unit structs as style markers, as iced's and a component crate's
/// `badge::Secondary` are: defined in one module, re-exported by another
/// and converted by `From` where a method takes `impl Into<StyleFn>`. A
/// tuple struct's constructor is a function value.
pub mod markers {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Secondary;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Ghost;
}

pub mod badge {
    pub use crate::markers::{Ghost, Secondary};

    pub struct StyleFn(fn(u32) -> String);

    impl From<Secondary> for StyleFn {
        fn from(_: Secondary) -> Self {
            StyleFn(|size| format!("secondary {size}"))
        }
    }

    impl From<Ghost> for StyleFn {
        fn from(_: Ghost) -> Self {
            StyleFn(|size| format!("ghost {size}"))
        }
    }

    pub struct Badge {
        label: String,
        style: StyleFn,
    }

    /// A badge of slots, as a component crate's `badge!` makes one: a
    /// row of its children, styled, and itself a slot of a row.
    pub struct Slotted<'a, M> {
        row: parts::Row<'a, M>,
        style: StyleFn,
    }

    impl<'a, M: 'a> Slotted<'a, M> {
        pub fn new(row: parts::Row<'a, M>) -> Self {
            Slotted {
                row,
                style: StyleFn(|size| format!("default {size}")),
            }
        }

        pub fn style(mut self, style: impl Into<StyleFn>) -> Self {
            self.style = style.into();
            self
        }
    }

    impl<'a, M: 'a> From<Slotted<'a, M>> for parts::Slot<'a, M> {
        fn from(badge: Slotted<'a, M>) -> Self {
            let style = (badge.style.0)(badge.row.gap());
            parts::Slot::from(format!("badge {style} [{}]", badge.row.draw()))
        }
    }

    /// A badge of any slot, as a component crate's `badge(content: impl
    /// Into<Slot<'a, Message>>)` makes one.
    pub struct Composed<'a, M> {
        content: parts::Slot<'a, M>,
        style: StyleFn,
    }

    pub fn composed<'a, M: 'a>(content: impl Into<parts::Slot<'a, M>>) -> Composed<'a, M> {
        Composed {
            content: content.into(),
            style: StyleFn(|size| format!("default {size}")),
        }
    }

    impl<'a, M: 'a> Composed<'a, M> {
        pub fn style(mut self, style: impl Into<StyleFn>) -> Self {
            self.style = style.into();
            self
        }
    }

    impl<'a, M: 'a> From<Composed<'a, M>> for parts::Slot<'a, M> {
        fn from(badge: Composed<'a, M>) -> Self {
            let style = (badge.style.0)(2);
            parts::Slot::from(format!("badge {style} [{}]", badge.content.label()))
        }
    }

    /// A badge as icedxdy's is: any slot, a style and a message on press,
    /// a slot of a row and a widget of its own when its message is `Clone`.
    pub struct Pill<'a, M> {
        content: parts::Slot<'a, M>,
        style: StyleFn,
        on_press: Option<M>,
    }

    pub fn pill<'a, M: 'a>(content: impl Into<parts::Slot<'a, M>>) -> Pill<'a, M> {
        Pill {
            content: content.into(),
            style: StyleFn(|size| format!("default {size}")),
            on_press: None,
        }
    }

    impl<'a, M: 'a> Pill<'a, M> {
        pub fn style(mut self, style: impl Into<StyleFn>) -> Self {
            self.style = style.into();
            self
        }
    }

    impl<'a, M: Clone + 'a> Pill<'a, M> {
        pub fn on_press(mut self, message: M) -> Self {
            self.on_press = Some(message);
            self
        }
    }

    impl<'a, M: Clone + 'a> Pill<'a, M> {
        fn describe(&self) -> String {
            let style = (self.style.0)(2);
            let press = if self.on_press.is_some() {
                " pressed"
            } else {
                ""
            };
            format!("pill {style}{press} [{}]", self.content.label())
        }
    }

    impl<'a, M: Clone + 'a> From<Pill<'a, M>> for widgets::Element<'a, M, parts::Theme> {
        fn from(pill: Pill<'a, M>) -> Self {
            widgets::Element::described(pill.describe())
        }
    }

    impl<'a, M: Clone + 'a> From<Pill<'a, M>> for parts::Slot<'a, M> {
        fn from(pill: Pill<'a, M>) -> Self {
            parts::Slot::from(pill.describe())
        }
    }

    impl<'a, M: Clone + 'a> parts::Compose<M> for Pill<'a, M> {
        fn compose(&self) -> String {
            self.describe()
        }
    }

    pub fn badge(label: &str) -> Badge {
        Badge {
            label: label.to_string(),
            style: StyleFn(|size| format!("default {size}")),
        }
    }

    impl Badge {
        pub fn style(mut self, style: impl Into<StyleFn>) -> Self {
            self.style = style.into();
            self
        }

        pub fn draw(&self) -> String {
            let size = self.label.len() as u32;
            format!("{}: {}", self.label, (self.style.0)(size))
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Wrap(pub i64);
}

/// A component crate's slots, re-exported from the crate of its parts as
/// icedxdy re-exports icedxdy_widget's `Slot`, with the widget crate's
/// text beside them: `slots!` gathers children that each convert through
/// `Into::<$crate::compose::Slot<'_, _>>::into` in an array a row takes as
/// `impl IntoIterator`, as iced's `Row::with_children` does.
pub mod compose {
    pub use parts::{
        badge_check, icon, label, rule, wrap, Compose, Icon, Label, Row, Rule, Slot, Theme,
    };
    pub use widgets::{text, text_like, Text, TextLike};
}

/// A row of `compose::Slot`s, each child converted as a component crate's
/// `badge!` and `row!` convert theirs.
#[macro_export]
macro_rules! slots {
    ($($child:expr),* $(,)?) => {
        $crate::compose::Row::with_children([
            $(Into::<$crate::compose::Slot<'_, _>>::into($child)),*
        ])
    };
}

/// A badge of `compose::Slot`s, as a component crate's `badge!` wraps
/// its children's row.
#[macro_export]
macro_rules! badge {
    ($($child:expr),* $(,)?) => {
        $crate::badge::Slotted::new(
            $crate::compose::Row::with_children([
                $(Into::<$crate::compose::Slot<'_, _>>::into($child)),*
            ])
            .spacing(4),
        )
    };
}

/// A component of `compose::Slot`s, as a component crate's `badge!` makes
/// one: its children's row wrapped in `Slot::new`, whose message only
/// `impl Compose<Message> for Row` ties to the row's, passed on as `impl
/// Into<Slot<'a, Message>>`.
#[macro_export]
macro_rules! component {
    ($($child:expr),* $(,)?) => {
        $crate::badge::composed(
            $crate::compose::Slot::new(
                $crate::compose::Row::with_children([
                    $(Into::<$crate::compose::Slot<'_, _>>::into($child)),*
                ])
                .spacing(2),
            ),
        )
    };
}

/// A pill of one child, or of a row of several, as icedxdy's `badge!`
/// makes one: a leading bare identifier is the caller's `icon::$first()`.
#[macro_export]
macro_rules! pill {
    ($single:expr $(,)?) => {
        $crate::badge::pill($single)
    };
    ($first:ident, $($rest:expr),+ $(,)?) => {
        $crate::badge::pill(
            $crate::compose::Slot::new(
                $crate::compose::Row::with_children([
                    Into::<$crate::compose::Slot<'_, _>>::into(icon::$first()),
                    $(Into::<$crate::compose::Slot<'_, _>>::into($rest)),+
                ])
                .spacing(4)
            )
        )
    };
    ($($child:expr),+ $(,)?) => {
        $crate::badge::pill(
            $crate::compose::Slot::new(
                $crate::compose::Row::with_children([
                    $(Into::<$crate::compose::Slot<'_, _>>::into($child)),+
                ])
                .spacing(4)
            )
        )
    };
}

/// Functions whose signatures have array types, as a component crate's
/// `Row::with_children<const N: usize>([Element; N])` takes its children
/// and a pixel's channels are filled through `&mut [u8; 4]`.
pub mod arrays {
    #[derive(Debug, Clone, PartialEq)]
    pub struct Slot<'a, M> {
        message: M,
        label: &'a str,
    }

    impl<'a, M> Slot<'a, M> {
        pub fn new(message: M) -> Self {
            Slot {
                message,
                label: "slot",
            }
        }
    }

    pub struct Row<'a, M> {
        children: Vec<Slot<'a, M>>,
    }

    impl<'a, M: std::fmt::Debug> Row<'a, M> {
        pub fn with_children<const N: usize>(children: [Slot<'a, M>; N]) -> Self {
            Row {
                children: children.into(),
            }
        }

        pub fn describe(&self) -> String {
            let messages: Vec<String> = self
                .children
                .iter()
                .map(|child| format!("{}={:?}", child.label, child.message))
                .collect();
            format!("{} children: {}", self.children.len(), messages.join(" "))
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Pixel(pub [u8; 4]);

    pub fn fill(channels: &mut [u8; 4], value: u8) {
        channels[1] = value;
        channels[3] = value + 1;
    }

    pub fn sum(values: &[i64; 3]) -> i64 {
        values.iter().sum()
    }

    pub fn corners() -> [u8; 4] {
        [1, 2, 4, 8]
    }
}

/// A simulation that owns its building and lends it to a program's
/// functions, as elevato's `Simulation` lends `&Building` to the program's
/// `update` and `view`. The building isn't `Clone`: a script function
/// borrows it for each call, and passes the borrow on to the crate's
/// functions that take `&Building`.
pub mod simulation {
    use std::fmt;
    use std::marker::PhantomData;

    #[derive(Debug, PartialEq)]
    pub struct Building {
        floors: usize,
    }

    impl Building {
        pub fn new(floors: usize) -> Self {
            Self { floors }
        }

        pub fn floors(&self) -> usize {
            self.floors
        }
    }

    impl fmt::Display for Building {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}-storey building", self.floors)
        }
    }

    pub fn summary(building: &Building) -> String {
        format!("summary of {} floors", building.floors)
    }

    /// What a view gives, at the lifetime of what it borrows, as iced's
    /// `Element<'a, Message>`.
    pub struct Picture<'a, M> {
        title: &'a str,
        floors: usize,
        message: PhantomData<M>,
    }

    impl<M> Picture<'_, M> {
        pub fn describe(&self) -> String {
            format!("{} of {} floors", self.title, self.floors)
        }
    }

    /// The building drawn, as elevato's `view<'a, Message: 'a>(&Building)
    /// -> Element<'a, Message>`: the picture borrows nothing of it.
    pub fn picture<'a, M: 'a>(building: &Building) -> Picture<'a, M> {
        Picture {
            title: "picture",
            floors: building.floors,
            message: PhantomData,
        }
    }

    /// A simulation that keeps its program's `update`.
    pub struct Running<P, M> {
        simulation: Simulation<P>,
        update: Box<dyn FnMut(&mut P, M, &Building) -> Command<M>>,
    }

    impl<P, M> Running<P, M> {
        /// Delivers `message` to the kept `update`, then each message the
        /// commands it answers deliver, as elevato's `frame` dispatches.
        /// How many messages it delivered.
        pub fn deliver(&mut self, message: M) -> usize {
            let Self { simulation, update } = self;
            simulation.run(message, update)
        }
    }

    /// What an `update` answers, as iced's `Task<Message>`: the messages
    /// it delivers next. It borrows nothing of the building.
    pub struct Command<M> {
        messages: Vec<M>,
    }

    impl<M> Command<M> {
        pub fn none() -> Self {
            Self {
                messages: Vec::new(),
            }
        }

        pub fn done(message: M) -> Self {
            Self {
                messages: vec![message],
            }
        }

        pub fn map<N>(self, f: impl Fn(M) -> N) -> Command<N> {
            Command {
                messages: self.messages.into_iter().map(f).collect(),
            }
        }
    }

    pub struct Simulation<P> {
        program: P,
        building: Building,
    }

    impl<P> Simulation<P> {
        pub fn new(program: P, floors: usize) -> Self {
            Self {
                program,
                building: Building::new(floors),
            }
        }

        /// `update` of the program, `message` and the building, which
        /// gains a floor after.
        pub fn step<M>(&mut self, message: M, mut update: impl FnMut(&mut P, M, &Building)) {
            update(&mut self.program, message, &self.building);
            self.building.floors += 1;
        }

        /// Runs the program from `first`: each message goes to `update`
        /// with the building, and the command it answers is run, its
        /// messages delivered in turn, as elevato's simulation runs the
        /// `Task` its `update` gives. How many messages it delivered.
        pub fn run<M>(
            &mut self,
            first: M,
            mut update: impl FnMut(&mut P, M, &Building) -> Command<M>,
        ) -> usize {
            let mut queue = vec![first];
            let mut delivered = 0;
            while let Some(message) = queue.pop() {
                delivered += 1;
                let command = update(&mut self.program, message, &self.building);
                queue.extend(command.messages);
                self.building.floors += 1;
            }
            delivered
        }

        /// The simulation with `update` kept to run its program from then
        /// on, as elevato's `Simulation::new` keeps the program's `update`
        /// for its `frame`.
        pub fn keep<M>(
            self,
            update: impl FnMut(&mut P, M, &Building) -> Command<M> + 'static,
        ) -> Running<P, M> {
            Running {
                simulation: self,
                update: Box::new(update),
            }
        }

        /// What the program's `view` of the building draws.
        pub fn draw<M, V>(&self, view: V) -> String
        where
            V: for<'a> Fn(&'a P, &'a Building) -> Picture<'a, M>,
        {
            view(&self.program, &self.building).describe()
        }
    }
}

/// A widget of iced's `Button<'a, Message, W, Theme>` shape: generic over
/// its content, which a `&str` fills, holding a closure at `'a` and its
/// theme's class at `'a`, an associated type that makes it invariant in
/// `'a`. Its destructor runs at `'a`, so whatever its label is pinned in
/// has to outlive it (#1396).
pub mod pressable {
    pub trait Catalog {
        type Class<'a>;

        fn default<'a>() -> Self::Class<'a>;
    }

    pub struct Plain;

    impl Catalog for Plain {
        type Class<'a> = Box<dyn Fn(&Plain) -> u32 + 'a>;

        fn default<'a>() -> Self::Class<'a> {
            Box::new(|_| 1)
        }
    }

    pub struct Pressable<'a, Message, W, Theme: Catalog = Plain> {
        content: W,
        on_press: Option<Box<dyn Fn() -> Message + 'a>>,
        class: Theme::Class<'a>,
    }

    pub fn pressable<'a, Message, W, Theme>(content: W) -> Pressable<'a, Message, W, Theme>
    where
        Theme: Catalog + 'a,
    {
        Pressable {
            content,
            on_press: None,
            class: Theme::default(),
        }
    }

    /// One whose own signature borrows its label, so its `let` opens a
    /// scope frame for it.
    pub fn labeled<'a, Message, Theme>(label: &'a str) -> Pressable<'a, Message, &'a str, Theme>
    where
        Theme: Catalog + 'a,
    {
        pressable(label)
    }

    impl<'a, Message, W: std::fmt::Display> Pressable<'a, Message, W, Plain> {
        pub fn describe(&self) -> String {
            let pressable = if self.on_press.is_some() { "on" } else { "off" };
            format!("{} {pressable} {}", self.content, (self.class)(&Plain))
        }
    }
}

/// A terminal that draws into a page, as ratzilla's is: its trait gives
/// another crate's generic `Terminal<B>` methods by a generic impl, and
/// its error converts into an `io::Error` and is a `std::error::Error` by
/// the impl a derive writes.
pub mod web {
    use parts::terminal::{Backend, Frame, Terminal};

    pub mod error {
        /// The crate's error, whose `std::error::Error` and `Display`
        /// impls are what thiserror's derive expands to.
        #[derive(Debug)]
        pub enum Error {
            UnableToRetrieveElementById(String),
        }

        #[allow(unused_qualifications)]
        #[automatically_derived]
        impl ::widgets::__private::Error for Error {}

        #[allow(unused_qualifications)]
        #[automatically_derived]
        impl ::core::fmt::Display for Error {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                match self {
                    Error::UnableToRetrieveElementById(id) => {
                        formatter.write_fmt(format_args!("Unable to retrieve element: {id}"))
                    }
                }
            }
        }

        impl From<Error> for std::io::Error {
            fn from(error: Error) -> Self {
                std::io::Error::other(error.to_string())
            }
        }
    }

    use error::Error;

    /// A key pressed.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Key {
        Char(char),
        Enter,
    }

    /// A page's element, which hears the keys `typed` holds.
    pub struct DomBackend {
        id: String,
        typed: Vec<Key>,
    }

    impl DomBackend {
        /// The element of id `id`; an empty id names none.
        pub fn new_by_id(id: &str) -> Result<Self, Error> {
            if id.is_empty() {
                return Err(Error::UnableToRetrieveElementById(id.to_owned()));
            }
            let typed = vec![Key::Char('h'), Key::Char('i'), Key::Enter];
            Ok(Self {
                id: id.to_owned(),
                typed,
            })
        }
    }

    impl Backend for DomBackend {
        fn name(&self) -> String {
            self.id.clone()
        }
    }

    /// What hands a terminal's keys to a program.
    pub trait EventHandler {
        fn on_key_event<F>(&mut self, callback: F) -> Result<(), Error>
        where
            F: FnMut(Key) + 'static;
    }

    impl EventHandler for DomBackend {
        fn on_key_event<F>(&mut self, mut callback: F) -> Result<(), Error>
        where
            F: FnMut(Key) + 'static,
        {
            for key in self.typed.drain(..) {
                callback(key);
            }
            Ok(())
        }
    }

    /// A terminal that draws on the page's frames, as ratzilla's
    /// `WebRenderer` is.
    pub trait WebRenderer {
        fn draw_web<F>(self, render: F)
        where
            F: FnMut(&mut Frame) + 'static;

        fn on_key_event<F>(&mut self, callback: F) -> Result<(), Error>
        where
            F: FnMut(Key) + 'static;
    }

    impl<B> WebRenderer for Terminal<B>
    where
        B: Backend + EventHandler + 'static,
    {
        fn draw_web<F>(mut self, mut render: F)
        where
            F: FnMut(&mut Frame) + 'static,
        {
            self.draw(|frame| render(frame));
        }

        fn on_key_event<F>(&mut self, callback: F) -> Result<(), Error>
        where
            F: FnMut(Key) + 'static,
        {
            self.backend_mut().on_key_event(callback)
        }
    }
}

/// Struct literals and patterns through a crate's type aliases, as
/// wgpu's `DeviceDescriptor<'a> = wgt::DeviceDescriptor<Option<&'a str>>`.
pub mod described {
    #[derive(Debug, Clone, PartialEq)]
    pub struct Described<L> {
        pub label: L,
        pub count: u32,
    }

    impl<L: Default> Default for Described<L> {
        fn default() -> Self {
            Self {
                label: L::default(),
                count: 1,
            }
        }
    }

    pub type Owned = Described<u32>;
    pub type Labeled<'a> = Described<Option<&'a str>>;

    pub fn describe(descriptor: &Labeled<'_>) -> String {
        format!("{:?} {}", descriptor.label, descriptor.count)
    }

    /// Takes a borrow of an alias's struct and gives a future, as wgpu's
    /// `Adapter::request_device(&self, desc: &DeviceDescriptor<'_>) ->
    /// impl Future<..> + WasmNotSend` does.
    pub struct Adapter;

    pub trait NotSend {}

    impl<T> NotSend for T {}

    impl Adapter {
        pub fn request(
            &self,
            descriptor: &Labeled<'_>,
        ) -> impl std::future::Future<Output = Result<(u32, u32), ()>> + NotSend + use<> {
            let count = descriptor.count;
            async move { Ok((count, count * 2)) }
        }
    }

    /// The future's output, polled to its end on this thread.
    pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
        }
    }
}

pub use described::Labeled;

/// A struct whose only public name is an alias of one instance of it, as
/// wgpu's `SurfaceConfiguration = wgt::SurfaceConfiguration<Vec<
/// TextureFormat>>` is.
mod surface {
    #[derive(Debug, Clone, PartialEq)]
    pub struct Configuration<V> {
        pub width: u32,
        pub views: V,
    }
}

pub type Configuration = surface::Configuration<Vec<u32>>;

pub fn configure(configuration: &Configuration) -> u32 {
    configuration.width + configuration.views.iter().sum::<u32>()
}

/// A struct whose fields are collections of crate values, as wgpu's
/// `SurfaceCapabilities::formats: Vec<TextureFormat>` is: a `&self`
/// method reads such a field where it is, and the struct stays whole.
pub mod capabilities {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Format {
        Plain,
        Srgb,
    }

    impl Format {
        pub fn is_srgb(&self) -> bool {
            matches!(self, Format::Srgb)
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    pub struct Tag {
        pub name: String,
    }

    #[derive(Debug, Clone)]
    pub struct Capabilities {
        pub formats: Vec<Format>,
        pub tag: Option<Tag>,
        pub count: u32,
    }

    pub fn capabilities() -> Capabilities {
        Capabilities {
            formats: vec![Format::Plain, Format::Srgb],
            tag: Some(Tag {
                name: "main".to_string(),
            }),
            count: 2,
        }
    }
}

/// A value that keeps a shared and an exclusive borrow of what its
/// associated function takes at one early-bound lifetime, as iced's
/// integration `Scene::clear(&'a TextureView, &'a mut CommandEncoder, Color)
/// -> RenderPass<'a>` gives, and records into the place it borrows when it
/// is dropped.
pub mod passes {
    pub struct Target {
        name: String,
    }

    pub struct Encoder {
        log: Vec<String>,
    }

    pub struct Pass<'a> {
        target: &'a Target,
        encoder: &'a mut Encoder,
        layers: Vec<&'a Layer>,
    }

    /// What a pass keeps a shared borrow of for its own life once it is
    /// drawn into it, as integration's `Scene::draw<'a>(&'a self, &mut
    /// RenderPass<'a>)` gives the pass its pipeline.
    pub struct Layer {
        name: String,
    }

    impl Layer {
        pub fn new(name: &str) -> Self {
            Self {
                name: name.to_owned(),
            }
        }

        pub fn draw<'a>(&'a self, pass: &mut Pass<'a>) {
            pass.layers.push(self);
        }

        pub fn rename(&mut self, name: &str) {
            name.clone_into(&mut self.name);
        }

        pub fn name(&self) -> &str {
            &self.name
        }
    }

    impl Target {
        pub fn new(name: &str) -> Self {
            Self {
                name: name.to_owned(),
            }
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn clear<'a>(target: &'a Target, encoder: &'a mut Encoder, color: u32) -> Pass<'a> {
            encoder
                .log
                .push(format!("clear {} to {color}", target.name));
            Pass {
                target,
                encoder,
                layers: Vec::new(),
            }
        }
    }

    impl Encoder {
        pub fn new() -> Self {
            Self { log: Vec::new() }
        }

        pub fn finish(self) -> String {
            self.log.join(", ")
        }
    }

    impl Default for Encoder {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Pass<'_> {
        pub fn draw(&mut self, count: u32) {
            let line = format!("draw {count} on {}", self.target.name);
            self.encoder.log.push(line);
        }
    }

    impl Drop for Pass<'_> {
        fn drop(&mut self) {
            for layer in &self.layers {
                let line = format!("layer {}", layer.name);
                self.encoder.log.push(line);
            }
            self.encoder.log.push("end".to_owned());
        }
    }
}

/// A trait object a crate takes of any value whose type implements its
/// supertraits, as iced's `update(window: &dyn Window, ..)` takes winit's
/// `Arc<Window>` through a blanket impl.
pub mod windows {
    use std::sync::Arc;

    pub trait HasHandle {
        fn handle(&self) -> u32;
    }

    pub trait Window: HasHandle + std::fmt::Debug {}

    impl<T> Window for T where T: HasHandle + std::fmt::Debug {}

    #[derive(Debug)]
    pub struct Surface {
        id: u32,
    }

    impl Surface {
        pub fn new(id: u32) -> Self {
            Self { id }
        }
    }

    impl HasHandle for Surface {
        fn handle(&self) -> u32 {
            self.id
        }
    }

    impl<T: HasHandle + ?Sized> HasHandle for Arc<T> {
        fn handle(&self) -> u32 {
            (**self).handle()
        }
    }

    pub fn handle_of(window: &dyn Window) -> u32 {
        window.handle()
    }
}

/// A crate's type a `for` loops over by its `IntoIterator` impl alone, as
/// integration's `for message in messages` loops over iced's `shell::Bus`.
pub mod bags {
    pub struct Bag<T> {
        items: Vec<(T, u32)>,
    }

    impl<T> Bag<T> {
        pub fn new() -> Self {
            Self { items: Vec::new() }
        }

        pub fn push(&mut self, item: T) {
            let receipt = u32::try_from(self.items.len()).unwrap_or(u32::MAX);
            self.items.push((item, receipt));
        }
    }

    impl<T> Default for Bag<T> {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Pushes one item into `bag` and gives how many it holds, as iced's
    /// `update(.., messages: &mut Bus<_>)` fills a bus a program may make
    /// for the call alone (`&mut Bus::new()`).
    pub fn counted(bag: &mut Bag<u32>, item: u32) -> usize {
        bag.push(item);
        bag.items.len()
    }

    pub struct IntoIter<T> {
        iter: std::vec::IntoIter<(T, u32)>,
    }

    impl<T> Iterator for IntoIter<T> {
        type Item = T;

        fn next(&mut self) -> Option<T> {
            self.iter.next().map(|(item, _)| item)
        }
    }

    impl<T> IntoIterator for Bag<T> {
        type Item = T;
        type IntoIter = IntoIter<T>;

        fn into_iter(self) -> Self::IntoIter {
            IntoIter {
                iter: self.items.into_iter(),
            }
        }
    }
}

/// A struct the crate implements `Drop` for, whose public field a program
/// reads, as integration's `frame.texture.create_view(..)` reads wgpu's
/// `SurfaceTexture::texture`: rustc never moves the field out (E0509).
pub mod frames {
    #[derive(Clone, Debug)]
    pub struct Texture {
        id: u32,
    }

    impl Texture {
        pub fn id(&self) -> u32 {
            self.id
        }
    }

    pub struct SurfaceTexture {
        pub texture: Texture,
    }

    impl Drop for SurfaceTexture {
        fn drop(&mut self) {}
    }

    pub fn acquire(id: u32) -> SurfaceTexture {
        SurfaceTexture {
            texture: Texture { id },
        }
    }
}

/// A `&self` method whose opaque output captures the receiver's lifetime
/// under edition 2024's rules, as integration's `Controls::view(&self) ->
/// impl Widget<..>` does: the value borrows the receiver for as long as it
/// lives.
pub mod panels {
    pub trait Show {
        fn show(&self) -> String;
    }

    struct Shown<'a> {
        label: &'a str,
    }

    impl Show for Shown<'_> {
        fn show(&self) -> String {
            format!("[{}]", self.label)
        }
    }

    pub struct Panel {
        label: String,
    }

    impl Panel {
        pub fn new(label: &str) -> Self {
            Self {
                label: label.to_owned(),
            }
        }

        pub fn view(&self) -> impl Show {
            Shown { label: &self.label }
        }

        pub fn rename(&mut self, label: &str) {
            label.clone_into(&mut self.label);
        }
    }

    /// Takes a value of any type that shows, as iced's
    /// `UserInterface::build(root: impl Into<Element<..>>, ..)` takes the
    /// view.
    pub fn framed(shown: impl Show) -> String {
        format!("|{}|", shown.show())
    }
}
