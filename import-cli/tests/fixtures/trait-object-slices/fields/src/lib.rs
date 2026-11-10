//! tracing-core's value set: values of a trait lent as objects in a slice
//! of `Option`s, which the set a field set's method makes borrows for as
//! long as it lives, and a function that takes the set, as
//! `Event::dispatch` does.

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

/// What a subscriber is given, as `Event::dispatch(meta, &value_set)`
/// gives it.
pub fn dispatch(name: &str, set: &ValueSet<'_>) -> String {
    format!("{name} {}", set.render())
}
