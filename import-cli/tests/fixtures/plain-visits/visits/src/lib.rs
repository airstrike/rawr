//! A trait with a generic method, as serde's `Serialize::serialize` is.

/// What a value is shown to.
pub trait Visitor {
    type Out;
    fn int(self, value: i64) -> Self::Out;
}

/// A value that shows itself to any visitor.
pub trait Visit {
    fn visit<V: Visitor>(&self, visitor: V) -> V::Out;
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

/// `value` shown to both visitors.
pub fn show<T: Visit>(value: T) -> String {
    let doubled = value.visit(Doubler);
    let spelled = value.visit(Speller);
    format!("{doubled} {spelled}")
}
