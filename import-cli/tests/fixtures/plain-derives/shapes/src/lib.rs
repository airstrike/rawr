//! A trait and its derive under one name, re-exported from a proc-macro
//! crate as serde re-exports serde_derive's.

pub use shapes_derive::Shape;

/// A type that knows its own name.
pub trait Shape {
    fn name(&self) -> &'static str;
}

/// The name of `shape`.
pub fn name_of(shape: &impl Shape) -> &'static str {
    shape.name()
}
