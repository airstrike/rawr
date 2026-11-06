//! Traits with generic methods, as serde's `Serialize::serialize` and
//! `Deserialize::deserialize` are.

use std::fmt;

/// What a value is shown to.
pub trait Visitor {
    type Out;
    fn int(self, value: i64) -> Self::Out;
}

/// A value that shows itself to any visitor.
pub trait Visit {
    fn visit<V: Visitor>(&self, visitor: V) -> V::Out;

    /// Shown to a visitor there may be none of.
    fn maybe<V: Visitor>(&self, visitor: Option<V>) -> Option<V::Out>;
}

/// Doubles what it is shown.
pub struct Doubler;

impl Visitor for Doubler {
    type Out = i64;
    fn int(self, value: i64) -> i64 {
        value * 2
    }
}

/// Spells what it is shown.
pub struct Speller;

impl Visitor for Speller {
    type Out = String;
    fn int(self, value: i64) -> String {
        format!("<{value}>")
    }
}

/// `value` shown to both visitors, and to one there may be none of.
pub fn show<T: Visit>(value: T) -> String {
    let doubled = value.visit(Doubler);
    let spelled = value.visit(Speller);
    let some = value.maybe(Some(Doubler));
    let none = value.maybe::<Speller>(None);
    format!("{doubled} {spelled} {some:?} {none:?}")
}

/// `value`, borrowed, shown to a visitor.
pub fn shown<T: Visit>(value: &T) -> String {
    value.visit(Speller)
}

/// std's items re-exported, as serde's hidden `__private` module
/// re-exports them for its derives.
#[doc(hidden)]
pub mod reexports {
    pub use core::fmt::{self, Formatter};
    pub use core::option::Option::{self, None, Some};
}

/// A problem a reader reports.
pub trait Problem: Sized {
    fn custom(message: &str) -> Self;
    fn describe(&self) -> String;

    /// Too few numbers, where `expected` says what was, as serde's
    /// `de::Error::invalid_length` takes a `&dyn Expected`.
    fn short(count: usize, expected: &dyn Expected) -> Self {
        Self::custom(&format!("{count} read, expected {expected}"))
    }
}

/// What a reader expected, as serde's `de::Expected` is.
pub trait Expected {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result;
}

impl Expected for &str {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(self)
    }
}

impl fmt::Display for dyn Expected + '_ {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        Expected::fmt(self, formatter)
    }
}

/// A count of numbers, expected.
pub struct Count(usize);

impl Count {
    pub fn new(numbers: usize) -> Self {
        Count(numbers)
    }
}

impl Expected for Count {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{} numbers", self.0)
    }
}

/// What `expected` says.
pub fn expecting(expected: &dyn Expected) -> String {
    format!("expected {expected}")
}

/// What adds numbers up.
pub trait Tally {
    fn add(&mut self, number: i64);
}

/// A sum of numbers.
#[derive(Default)]
pub struct Sum(i64);

impl Sum {
    pub fn total(&self) -> i64 {
        self.0
    }
}

impl Tally for Sum {
    fn add(&mut self, number: i64) {
        self.0 += number;
    }
}

/// Each of `items` added to `tally`.
pub fn tally(items: &[i64], tally: &mut dyn Tally) {
    for item in items {
        tally.add(*item);
    }
}

/// Numbers read one at a time from what it borrows.
pub trait Reader<'a> {
    type Error: Problem;
    fn number(&mut self) -> Option<i64>;
}

/// A value read from any reader.
pub trait Read<'a>: Sized {
    fn read<R: Reader<'a>>(reader: R) -> Result<Self, R::Error>;
}

/// A problem's text.
pub struct Text(String);

impl Problem for Text {
    fn custom(message: &str) -> Self {
        Text(message.to_owned())
    }
    fn describe(&self) -> String {
        self.0.clone()
    }
}

/// The numbers of a borrowed slice, which is no `'static` value.
pub struct Numbers<'a> {
    items: &'a [i64],
}

impl<'a> Reader<'a> for Numbers<'a> {
    type Error = Text;
    fn number(&mut self) -> Option<i64> {
        let (first, rest) = self.items.split_first()?;
        self.items = rest;
        Some(*first)
    }
}

/// A `T` read from `items`, or the problem its reader reports.
pub fn read<'a, T: Read<'a> + std::fmt::Debug>(items: &'a [i64]) -> String {
    match T::read(Numbers { items }) {
        Ok(value) => format!("{value:?}"),
        Err(problem) => problem.describe(),
    }
}

/// What draws in a font that compares with itself, as iced's
/// `text::Renderer::Font` is `Copy + PartialEq`: the font's `PartialEq` is
/// `PartialEq<Self>`.
pub trait Painter {
    type Font: Copy + PartialEq;
    fn font(&self) -> Self::Font;
    fn fits(&self, font: Self::Font) -> bool;
}

/// A value that paints itself with any painter.
pub trait Paint {
    fn paint<P: Painter>(&self, painter: P) -> bool;
}

/// A painter whose font is a size.
pub struct Pen;

impl Painter for Pen {
    type Font = u8;
    fn font(&self) -> u8 {
        12
    }
    fn fits(&self, font: u8) -> bool {
        font == 12
    }
}

/// Whether `value` paints with a pen.
pub fn painted<T: Paint>(value: T) -> bool {
    value.paint(Pen)
}
