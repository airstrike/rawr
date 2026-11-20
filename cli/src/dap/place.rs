//! Where a value the client reads is, as Rust that names it in its frame:
//! a binding, an expression the client evaluated, or a part of either, as
//! the inspector splits a value's `{:?}` text. A place is what the client
//! adds to its watches, and what a new value is assigned to, through the
//! inspector's `eval`, so the program's compiler decides whether Rust
//! allows the write there.

/// Rust naming one value in a paused frame.
#[derive(Clone, Debug)]
pub struct Place(String);

impl Place {
    /// The frame's binding `name`.
    #[must_use]
    pub fn binding(name: &str) -> Self {
        Self(name.to_owned())
    }

    /// The value of the expression `code`, which is evaluated again each
    /// time the place is read or assigned.
    #[must_use]
    pub fn expression(code: &str) -> Self {
        Self(format!("({code})"))
    }

    /// The part `name` of this place's value, whose `{:?}` text is `text`.
    /// A map's entry is named by its key's `{:?}` text, which is Rust for
    /// the key, and is reached through `get_mut` by a reference to it: a
    /// string literal is one already, as `str` is what a `String` key
    /// borrows as. `[i]` is a sequence's item and any other name a field.
    /// Where Rust can't reach the part so, as an enum variant's field or a
    /// set's item, the compiler says why.
    #[must_use]
    pub fn part(&self, text: &str, name: &str) -> Self {
        let Self(place) = self;
        if text.starts_with('{') {
            let key = if name.starts_with('"') {
                name.to_owned()
            } else {
                format!("&{name}")
            };
            Self(format!("(*{place}.get_mut({key}).unwrap())"))
        } else if name.starts_with('[') {
            Self(format!("{place}{name}"))
        } else {
            Self(format!("{place}.{name}"))
        }
    }

    /// The place as Rust.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Code that assigns `value` to the place, its value then the block's.
    #[must_use]
    pub fn assign(&self, value: &str) -> String {
        let Self(place) = self;
        format!("{place} = {value}; {place}")
    }
}

#[cfg(test)]
mod tests {
    use super::Place;

    #[test]
    fn a_part_is_named_as_rust_reaches_it() {
        let counter = Place::binding("counter");
        assert_eq!(
            counter.part("Counter { count: 1 }", "count").as_str(),
            "counter.count"
        );
        let last = counter.part("Counter { last: (1, 2) }", "last");
        assert_eq!(last.part("(1, 2)", "0").as_str(), "counter.last.0");
        assert_eq!(
            Place::binding("items").part("[1, 2]", "[1]").as_str(),
            "items[1]"
        );
        assert_eq!(
            Place::binding("names")
                .part(r#"{"a": 1}"#, r#""a""#)
                .as_str(),
            r#"(*names.get_mut("a").unwrap())"#
        );
        assert_eq!(
            Place::binding("ids").part("{3: 1}", "3").as_str(),
            "(*ids.get_mut(&3).unwrap())"
        );
        assert_eq!(
            Place::expression("a.b()").part("S { x: 1 }", "x").as_str(),
            "(a.b()).x"
        );
        assert_eq!(counter.assign("4"), "counter = 4; counter");
    }
}
