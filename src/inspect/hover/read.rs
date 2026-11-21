//! A hover's read of one expression in a paused frame, which runs none of
//! the program's code beyond the `Debug` a read of a binding runs anyway.
//!
//! A place is read from its binding's `Debug` text and that text's parts
//! (see [`super::super::parts`]), with nothing compiled: a binding, a
//! field or tuple item, a deref, a reference, and an index by a literal or
//! by another place. Std's pointers and references print as what they
//! point to, so a deref reads the text it is applied to.
//!
//! Casts, arithmetic and comparisons on numbers, indexing a sequence or a
//! map, and `len` and `is_empty` on a string, a sequence or a map are
//! evaluated as `eval` evaluates code, once every operand is known to be
//! one of those std types by its text's shape. Every other call, every
//! macro, `?`, `.await`, and a `Deref` or an `Index` the program wrote is
//! refused, with a short reason that points to the console.

use super::super::parts;
use super::expression::{Binary, Kind, Length, Literal, Node, Runs, Unary};

/// What a read asks of the paused frame.
pub(in crate::inspect) enum Need<'a> {
    /// A binding's `Debug` text, or why it can't be read: `None` when the
    /// frame has no binding by that name.
    Binding(&'a str),
    /// Code evaluated in the frame, as `eval` evaluates it: its value's
    /// `Debug` text, or its error. `None` when the run can't.
    Evaluate(&'a str),
}

/// What a hover reads at an expression.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::inspect) enum Read {
    /// Its value, as `{:?}` prints it.
    Value(String),
    /// It is code a hover doesn't run, or its value can't be read, and why.
    Refused(String),
    /// There is nothing there to read, and why: a name no binding has, or a
    /// literal.
    Unread(String),
}

/// The frame a hover reads in: how it asks the frame, and the methods the
/// program wrote that a read must not run.
pub(in crate::inspect) struct Frame<'f> {
    ask: &'f mut dyn FnMut(Need<'_>) -> Option<Result<String, String>>,
    /// Each type the program wrote a `deref` or an `index` method for, by
    /// its name, with the method's name.
    written: Vec<(String, &'static str)>,
}

/// Why a place couldn't be read from its text: the read's answer, or that
/// it is computed, which evaluating it reads.
enum Missed {
    Read(Read),
    Computed,
}

/// What a value is, by the shape of its `Debug` text, as far as a hover
/// tells std's types apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Number,
    Bool,
    Char,
    Text,
    /// `[..]`: a `Vec`, an array, a slice or a `VecDeque`.
    Sequence,
    /// `{..}`: a map or a set.
    Map,
    Other,
}

/// What a hover tells of code it doesn't run.
const CONSOLE: &str = "evaluate it in the console";

impl Shape {
    fn of(text: &str) -> Self {
        match text.as_bytes().first() {
            Some(b'[') => Self::Sequence,
            Some(b'{') => Self::Map,
            Some(b'"') => Self::Text,
            Some(b'\'') => Self::Char,
            _ if text == "true" || text == "false" => Self::Bool,
            _ if text.parse::<f64>().is_ok() => Self::Number,
            _ => Self::Other,
        }
    }
}

impl<'f> Frame<'f> {
    /// A frame that `ask` reads, whose program wrote the methods in
    /// `written`: each `(type, method)`, the method `deref`, `deref_mut`,
    /// `index` or `index_mut`.
    pub(in crate::inspect) fn new(
        ask: &'f mut dyn FnMut(Need<'_>) -> Option<Result<String, String>>,
        written: Vec<(String, &'static str)>,
    ) -> Self {
        Self { ask, written }
    }

    /// What `node`, written in `text`, reads as.
    pub(in crate::inspect) fn read(&mut self, node: &Node, text: &str) -> Read {
        if let Kind::Literal(_) = node.kind {
            return Read::Unread("a literal is its own value".to_owned());
        }
        match self.place(node, text) {
            Ok(value) => Read::Value(value),
            Err(Missed::Read(read)) => read,
            Err(Missed::Computed) => self.computed(node, text),
        }
    }

    /// `node`'s value, read from its binding's text.
    fn place(&mut self, node: &Node, text: &str) -> Result<String, Missed> {
        match &node.kind {
            Kind::Binding(name) => match (self.ask)(Need::Binding(name)) {
                Some(Ok(value)) => Ok(value),
                Some(Err(why)) => Err(Missed::Read(Read::Refused(why))),
                None => Err(Missed::Read(Read::Unread(format!(
                    "no binding `{name}` is in scope here"
                )))),
            },
            Kind::Literal(_) => Ok(text[node.span.clone()].to_owned()),
            Kind::Paren(inner) | Kind::Reference(inner) => self.place(inner, text),
            Kind::Deref(inner) => {
                let value = self.place(inner, text)?;
                if self.wrote(&value, "deref") {
                    return Err(refused(format!(
                        "`{}` would run the `Deref` the program wrote: {CONSOLE}",
                        &text[node.span.clone()]
                    )));
                }
                Ok(value)
            }
            Kind::Field(base, name) => {
                let value = self.place(base, text)?;
                if let Some(found) = part(&value, name) {
                    return Ok(found);
                }
                if self.wrote(&value, "deref") {
                    return Err(refused(format!(
                        "reaching `.{name}` would run the `Deref` the program wrote: {CONSOLE}"
                    )));
                }
                // A hand-written `Debug` shows no parts: evaluating the
                // field reads it.
                if parts::of(&value).is_none() {
                    return Err(Missed::Computed);
                }
                Err(refused(format!(
                    "`{}` shows no field `{name}`",
                    &text[base.span.clone()]
                )))
            }
            Kind::Index(base, index) => {
                let value = self.place(base, text)?;
                let key = self.place(index, text)?;
                self.indexed(&value, &key, &text[base.span.clone()])
            }
            _ => Err(Missed::Computed),
        }
    }

    /// The item `key` names in `value`, a sequence's or a map's text, the
    /// value of `base`.
    fn indexed(&self, value: &str, key: &str, base: &str) -> Result<String, Missed> {
        match Shape::of(value) {
            Shape::Sequence => {
                let digits =
                    key.trim_end_matches(|character: char| character.is_ascii_alphabetic());
                let Ok(index) = digits.replace('_', "").parse::<usize>() else {
                    return Err(refused(format!(
                        "`{base}` is indexed by a whole number, and `{key}` isn't one"
                    )));
                };
                let items = parts::of(value).unwrap_or_default();
                let name = format!("[{index}]");
                items
                    .iter()
                    .find(|item| item["name"] == name.as_str())
                    .and_then(|item| item["value"].as_str())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        refused(format!(
                            "index out of bounds: the len is {} but the index is {index}",
                            items.len()
                        ))
                    })
            }
            Shape::Map => {
                part(value, key).ok_or_else(|| refused(format!("`{base}` has no entry for {key}")))
            }
            _ if self.wrote(value, "index") => Err(refused(format!(
                "`{base}[..]` would run the `Index` the program wrote: {CONSOLE}"
            ))),
            _ => Err(refused(format!("`{base}` isn't a sequence or a map"))),
        }
    }

    /// `node`'s value, evaluated in the frame, once its operands are std's
    /// numbers, strings, sequences and maps.
    fn computed(&mut self, node: &Node, text: &str) -> Read {
        if let Err(read) = self.shape(node, text) {
            return read;
        }
        let code = &text[node.span.clone()];
        match (self.ask)(Need::Evaluate(code)) {
            Some(Ok(value)) => Read::Value(value),
            Some(Err(error)) => Read::Refused(error.lines().next().unwrap_or_default().to_owned()),
            None => Read::Unread("this run can't evaluate code".to_owned()),
        }
    }

    /// What `node` is, when a hover may evaluate it; `Err` says why not.
    fn shape(&mut self, node: &Node, text: &str) -> Result<Shape, Read> {
        if let Kind::Literal(literal) = node.kind {
            return Ok(match literal {
                Literal::Number => Shape::Number,
                Literal::Text => Shape::Text,
                Literal::Char => Shape::Char,
                Literal::Bool => Shape::Bool,
            });
        }
        match self.place(node, text) {
            Ok(value) => return Ok(Shape::of(&value)),
            Err(Missed::Read(read)) => return Err(read),
            Err(Missed::Computed) => {}
        }
        let written = &text[node.span.clone()];
        match &node.kind {
            Kind::Paren(inner) | Kind::Reference(inner) | Kind::Deref(inner) => {
                self.shape(inner, text)
            }
            Kind::Field(base, _) => {
                self.shape(base, text)?;
                if self
                    .written
                    .iter()
                    .any(|(_, method)| method.starts_with("deref"))
                {
                    return Err(Read::Refused(format!(
                        "`{written}` may run a `Deref` the program wrote: {CONSOLE}"
                    )));
                }
                Ok(Shape::Other)
            }
            Kind::Index(base, index) => {
                let base_shape = self.shape(base, text)?;
                let index_shape = self.shape(index, text)?;
                match (base_shape, index_shape) {
                    (Shape::Sequence, Shape::Number) => {}
                    (Shape::Map, shape) if shape != Shape::Other => {}
                    (Shape::Sequence | Shape::Map, _) => {
                        return Err(Read::Refused(format!(
                            "`{written}` indexes by a value a hover can't tell is std's: \
                             {CONSOLE}"
                        )));
                    }
                    _ => {
                        return Err(Read::Refused(format!(
                            "`{}` isn't a sequence or a map a hover indexes: {CONSOLE}",
                            &text[base.span.clone()]
                        )));
                    }
                }
                Ok(self.items(base, text))
            }
            Kind::Cast(inner, _) => match self.shape(inner, text)? {
                Shape::Number | Shape::Bool | Shape::Char => Ok(Shape::Number),
                _ => Err(Read::Refused(format!(
                    "`{written}` casts a value a hover can't tell is a number: {CONSOLE}"
                ))),
            },
            Kind::Unary(unary, inner) => match (unary, self.shape(inner, text)?) {
                (Unary::Negate, Shape::Number) => Ok(Shape::Number),
                (Unary::Not, shape @ (Shape::Number | Shape::Bool)) => Ok(shape),
                _ => Err(operator(written)),
            },
            Kind::Binary(left, binary, right) => {
                let left = self.shape(left, text)?;
                let right = self.shape(right, text)?;
                match binary {
                    Binary::Arithmetic if (left, right) == (Shape::Number, Shape::Number) => {
                        Ok(Shape::Number)
                    }
                    Binary::Comparison
                        if left == right
                            && matches!(
                                left,
                                Shape::Number | Shape::Bool | Shape::Char | Shape::Text
                            ) =>
                    {
                        Ok(Shape::Bool)
                    }
                    Binary::Logic if (left, right) == (Shape::Bool, Shape::Bool) => Ok(Shape::Bool),
                    _ => Err(operator(written)),
                }
            }
            Kind::Length(receiver, length) => match self.shape(receiver, text)? {
                Shape::Text | Shape::Sequence | Shape::Map => Ok(match length {
                    Length::Len => Shape::Number,
                    Length::IsEmpty => Shape::Bool,
                }),
                _ => Err(Read::Refused(format!(
                    "`{written}` would call a method the program or a crate wrote: {CONSOLE}"
                ))),
            },
            Kind::Runs(runs, _) => Err(Read::Refused(format!(
                "a hover doesn't run {}: {CONSOLE}",
                match runs {
                    Runs::Call => "calls",
                    Runs::Macro => "macros",
                    Runs::Try => "`?`",
                    Runs::Await => "`.await`",
                }
            ))),
            Kind::Other(_) => Err(Read::Refused(
                "a hover reads one place, or arithmetic on places".to_owned(),
            )),
            Kind::Binding(_) | Kind::Literal(_) => Ok(Shape::Other),
        }
    }

    /// What every item of `base`, a sequence or a map, is, when they are
    /// all one shape and `base` reads as a place.
    fn items(&mut self, base: &Node, text: &str) -> Shape {
        let Ok(value) = self.place(base, text) else {
            return Shape::Other;
        };
        let shapes: Vec<Shape> = parts::of(&value)
            .unwrap_or_default()
            .iter()
            .filter_map(|item| item["value"].as_str().map(Shape::of))
            .collect();
        match shapes.split_first() {
            Some((first, rest)) if rest.iter().all(|shape| shape == first) => *first,
            _ => Shape::Other,
        }
    }

    /// Whether the program wrote `method` (`deref` or `index`, or its
    /// `_mut` twin) for the type whose `Debug` text `value` is: a struct's
    /// or an enum's, by the name it starts with.
    fn wrote(&self, value: &str, method: &str) -> bool {
        let name: String = value
            .chars()
            .take_while(|character| character.is_alphanumeric() || *character == '_')
            .collect();
        !name.is_empty()
            && self
                .written
                .iter()
                .any(|(owner, written)| *owner == name && written.starts_with(method))
    }
}

/// The part of `value`'s text named `name`, a field's, a tuple item's or a
/// map entry's.
fn part(value: &str, name: &str) -> Option<String> {
    parts::of(value)?
        .iter()
        .find(|part| part["name"] == name)
        .and_then(|part| part["value"].as_str())
        .map(str::to_owned)
}

fn refused(why: String) -> Missed {
    Missed::Read(Read::Refused(why))
}

/// Why an operator written as `written` isn't evaluated.
fn operator(written: &str) -> Read {
    Read::Refused(format!(
        "`{written}` reads only std's operators on numbers and bools in a hover: on another \
         type it may run an impl the program or a crate wrote; {CONSOLE}"
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{Frame, Need, Read};
    use crate::inspect::hover::expression::Tokens;

    /// What `code`, given whole, reads as in a frame whose bindings print
    /// as `bindings`, which evaluates nothing but records what it was
    /// asked to, and whose program wrote `written`.
    fn read(
        code: &str,
        bindings: &[(&str, &str)],
        written: &[(&str, &'static str)],
    ) -> (Read, Vec<String>) {
        let bindings: HashMap<&str, &str> = bindings.iter().copied().collect();
        let mut evaluated = Vec::new();
        let mut ask = |need: Need<'_>| match need {
            Need::Binding(name) => bindings.get(name).map(|value| Ok((*value).to_owned())),
            Need::Evaluate(code) => {
                evaluated.push(code.to_owned());
                Some(Ok("7".to_owned()))
            }
        };
        let written = written
            .iter()
            .map(|(owner, method)| ((*owner).to_owned(), *method))
            .collect();
        let engine = crate::Engine::new();
        let tokens = Tokens::of(&engine, code);
        let node = tokens.whole().expect("the code is one expression");
        let answer = Frame::new(&mut ask, written).read(&node, code);
        (answer, evaluated)
    }

    const PLAYER: &str = r#"Player { name: "Ada", score: 3, at: (1, 2), items: [10, 20, 30] }"#;

    fn value(text: &str) -> Read {
        Read::Value(text.to_owned())
    }

    #[test]
    fn a_place_reads_from_its_bindings_parts_with_nothing_evaluated() {
        let bindings = [
            ("player", PLAYER),
            ("turn", "2"),
            ("scores", r#"{"ada": 4, "bo": 5}"#),
            ("name", r#""bo""#),
            ("boxed", "Player { score: 9 }"),
        ];
        let reads = [
            ("player.score", value("3")),
            ("player.at.1", value("2")),
            ("player.items[1]", value("20")),
            ("player.items[turn]", value("30")),
            ("scores[\"ada\"]", value("4")),
            ("scores[&name]", value("5")),
            ("(*boxed).score", value("9")),
            ("*boxed", value("Player { score: 9 }")),
            (
                "player.items[5]",
                Read::Refused("index out of bounds: the len is 3 but the index is 5".to_owned()),
            ),
            (
                "missing.score",
                Read::Unread("no binding `missing` is in scope here".to_owned()),
            ),
        ];
        for (code, expected) in reads {
            let (answer, evaluated) = read(code, &bindings, &[]);
            assert_eq!(answer, expected, "{code}");
            assert!(evaluated.is_empty(), "{code} evaluated {evaluated:?}");
        }
    }

    #[test]
    fn std_arithmetic_and_lengths_on_places_evaluate_once_their_shapes_are_std() {
        let bindings = [
            ("names", r#"["a", "b"]"#),
            ("turn", "3"),
            ("player", PLAYER),
        ];
        let (answer, evaluated) = read("names[turn as usize % names.len()]", &bindings, &[]);
        assert_eq!(answer, value("7"));
        assert_eq!(evaluated, ["names[turn as usize % names.len()]"]);
        let (answer, evaluated) = read("player.score + 1", &bindings, &[]);
        assert_eq!(answer, value("7"));
        assert_eq!(evaluated, ["player.score + 1"]);
        let (answer, evaluated) = read("player + 1", &bindings, &[]);
        assert!(
            matches!(answer, Read::Refused(_)),
            "a struct's `+` may be the program's"
        );
        assert!(evaluated.is_empty());
    }

    #[test]
    fn a_call_a_macro_and_a_programs_own_deref_or_index_are_refused() {
        let bindings = [
            ("player", PLAYER),
            ("grid", "Grid { cells: [1, 2] }"),
            ("wrapped", "Wrapper { inner: 1 }"),
        ];
        let written = [("Grid", "index"), ("Wrapper", "deref")];
        let refusals = [
            (
                "player.reset()",
                "a hover doesn't run calls: evaluate it in the console",
            ),
            (
                "tick()",
                "a hover doesn't run calls: evaluate it in the console",
            ),
            (
                "format!(\"{}\", 1)",
                "a hover doesn't run macros: evaluate it in the console",
            ),
            (
                "grid[0]",
                "`grid[..]` would run the `Index` the program wrote: evaluate it in the console",
            ),
            (
                "*wrapped",
                "`*wrapped` would run the `Deref` the program wrote: evaluate it in the console",
            ),
        ];
        for (code, why) in refusals {
            let (answer, evaluated) = read(code, &bindings, &written);
            assert_eq!(answer, Read::Refused(why.to_owned()), "{code}");
            assert!(evaluated.is_empty(), "{code} evaluated {evaluated:?}");
        }
    }
}
