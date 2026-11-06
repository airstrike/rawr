//! Traits with generic methods, as serde's `Serialize::serialize` and
//! `Deserialize::deserialize` are.

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

/// A problem a reader reports.
pub trait Problem {
    fn custom(message: &str) -> Self;
    fn describe(&self) -> String;
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
