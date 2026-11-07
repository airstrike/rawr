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

    /// A generic type's `&self` method that gives an opaque iterator of
    /// values borrowing it, as `slice.iter().copied()` gives.
    impl<T: Copy> Holder<T> {
        pub fn items(&self) -> impl Iterator<Item = T> + '_ {
            self.0.iter().copied()
        }
    }

    /// A function's opaque iterator, as `(0..n).rev()` behind `impl
    /// Iterator<Item = u32>` gives.
    pub fn countdown(from: u32) -> impl Iterator<Item = u32> {
        (1..=from).rev()
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

/// A type with operators on both sides of a number, as iced's `Radians`:
/// `impl Mul<Radians> for f32` is the crate's impl of a std trait for a
/// std type, beside the type's own `Mul<f32>`, `Mul` and `Rem`.
pub mod angle {
    use std::ops::{Add, Div, Mul, Rem, Sub};

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
