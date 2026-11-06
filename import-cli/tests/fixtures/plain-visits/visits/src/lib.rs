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

/// Keys read one at a time, each a value of the caller's type, as serde's
/// `MapAccess::next_key` reads them.
pub trait Keys<'a> {
    type Error: Problem;
    fn next_key<K: Read<'a>>(&mut self) -> Result<Option<K>, Self::Error>;
}

/// A value made from keys, as a derive's `visit_map` makes one.
pub trait FromKeys<'a>: Sized {
    fn from_keys<M: Keys<'a>>(keys: M) -> Result<Self, M::Error>;
}

/// The keys of a borrowed slice, one number each.
pub struct Slots<'a> {
    items: &'a [i64],
}

impl<'a> Keys<'a> for Slots<'a> {
    type Error = Text;
    fn next_key<K: Read<'a>>(&mut self) -> Result<Option<K>, Text> {
        let Some((first, rest)) = self.items.split_first() else {
            return Ok(None);
        };
        self.items = rest;
        K::read(Numbers {
            items: std::slice::from_ref(first),
        })
        .map(Some)
    }
}

/// A `T` made from the keys of `items`, or the problem its keys report.
pub fn from_keys<'a, T: FromKeys<'a> + std::fmt::Debug>(items: &'a [i64]) -> String {
    match T::from_keys(Slots { items }) {
        Ok(value) => format!("{value:?}"),
        Err(problem) => problem.describe(),
    }
}

/// The names a reader expects, kept for as long as the program runs, as
/// serde's `deserialize_struct` takes its `fields`.
pub fn named(fields: &'static [&'static str]) -> String {
    fields.join(" ")
}

/// What reads bytes native code lends it, as serde's `visit_bytes` does.
pub trait Bytes {
    fn bytes(&self, data: &[u8]) -> usize;
}

/// What `bytes` makes of three bytes.
pub fn measure<T: Bytes>(bytes: &T) -> usize {
    bytes.bytes(&[1, 2, 3])
}
