//! tracing-core's value set: values of a trait lent as objects in a slice
//! of `Option`s, which the set a field set's method makes borrows for as
//! long as it lives, and a function that takes the set, as
//! `Event::dispatch` does. Its edition is 2021, older than 2024 as
//! tracing's is, for its `values!` block.

pub trait Value {
    fn record(&self) -> String;
}

impl Value for i64 {
    fn record(&self) -> String {
        format!("{self}")
    }
}

impl Value for bool {
    fn record(&self) -> String {
        format!("{self}")
    }
}

impl Value for std::fmt::Arguments<'_> {
    fn record(&self) -> String {
        format!("{self}")
    }
}

pub struct Point {
    pub x: i64,
    pub y: i64,
}

impl Value for Point {
    fn record(&self) -> String {
        format!("({}, {})", self.x, self.y)
    }
}

pub struct FieldSet {
    names: &'static [&'static str],
}

impl FieldSet {
    pub const fn new(names: &'static [&'static str]) -> Self {
        Self { names }
    }

    /// The set of `values`, one for each field.
    pub fn value_set_all<'v>(&'v self, values: &'v [Option<&'v (dyn Value + 'v)>]) -> ValueSet<'v> {
        ValueSet {
            values,
            fields: self,
        }
    }
}

pub struct ValueSet<'a> {
    values: &'a [Option<&'a (dyn Value + 'a)>],
    fields: &'a FieldSet,
}

impl ValueSet<'_> {
    pub fn render(&self) -> String {
        self.fields
            .names
            .iter()
            .zip(self.values)
            .map(|(name, value)| match value {
                Some(value) => format!("{name}={}", value.record()),
                None => format!("{name}=-"),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// One value, as a subscriber's visitor records it.
pub fn one(value: &dyn Value) -> String {
    value.record()
}

/// The set of `values`, each lent as a `&dyn Value`, made in a block whose
/// value is the set, as tracing's `valueset_all!` makes one: the block's
/// temporaries live to the end of its caller's statement in this crate's
/// edition.
#[macro_export]
macro_rules! values {
    ($fields:expr, [$($value:expr),*]) => {{
        $fields.value_set_all(&[$(Some(&$value as &dyn $crate::Value)),*])
    }};
}

/// What a subscriber is given, as `Event::dispatch(meta, &value_set)`
/// gives it.
pub fn dispatch(name: &str, set: &ValueSet<'_>) -> String {
    format!("{name} {}", set.render())
}
