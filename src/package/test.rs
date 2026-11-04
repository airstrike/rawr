//! A crate's `#[test]` functions, as `rustc --test` collects them for its
//! harness: each with its path from the crate root, what it must do to pass
//! (`#[should_panic]`) and whether it is left out (`#[ignore]`).

use std::path::Path;

use crate::item;
use crate::ImmutableString;

/// One `#[test]` function of a compiled crate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Test {
    name: Box<str>,
    function: ImmutableString,
    location: Location,
    expected: Expected,
    ignored: Option<Option<Box<str>>>,
}

/// What a test must do to pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expected {
    /// Return: `#[test]` alone.
    Return,
    /// Panic, with a message that contains the text when there is one:
    /// `#[should_panic]` or `#[should_panic(expected = "text")]`.
    Panic(Option<Box<str>>),
}

/// Where a test's function is named: its file, and the line and column of
/// its name, where libtest places a test that did not panic as expected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Location {
    path: Box<Path>,
    line: usize,
    column: usize,
}

/// A `#[test]` function as the parse declares it, before the crate compiles.
pub(crate) struct Declared {
    modules: Box<[ImmutableString]>,
    name: ImmutableString,
    location: Location,
    expected: Expected,
    ignored: Option<Option<Box<str>>>,
}

impl Test {
    /// The test's path from the crate root, as libtest names it:
    /// `tests::adds`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The compiled function to call with no arguments.
    #[must_use]
    pub fn function(&self) -> &str {
        &self.function
    }

    /// Where the test's function is named.
    #[must_use]
    pub const fn location(&self) -> &Location {
        &self.location
    }

    /// What the test must do to pass.
    #[must_use]
    pub const fn expected(&self) -> &Expected {
        &self.expected
    }

    /// `Some` when `#[ignore]` leaves the test out, with its reason when it
    /// gives one (`#[ignore = "slow"]`).
    #[must_use]
    pub fn ignored(&self) -> Option<Option<&str>> {
        self.ignored.as_ref().map(Option::as_deref)
    }
}

impl Location {
    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The line, from 1.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// The column, from 1.
    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }
}

impl Declared {
    /// The `#[test]` functions of `graph`'s entry package, in no order.
    pub(crate) fn of(graph: &super::Graph) -> Vec<Self> {
        let package = graph.entry_package().name();
        graph
            .parsed_sources()
            .filter(|(source, _)| source.package() == package)
            .flat_map(|(source, items)| {
                items.iter().filter_map(|parsed| match parsed {
                    item::Parsed::Function(function) => Self::from_function(source, function),
                    _ => None,
                })
            })
            .collect()
    }

    fn from_function(source: &super::source::Source, function: &item::Function) -> Option<Self> {
        let attributes = &function.header.attributes;
        let named = |name: &str| {
            attributes.iter().find(|attribute| {
                attribute.path.segments.len() == 1 && attribute.path.segments[0].name.text == name
            })
        };
        named("test")?;
        let expected = named("should_panic").map_or(Expected::Return, |attribute| {
            Expected::Panic(argument(&attribute.text, "should_panic", Some("expected")))
        });
        let ignored = named("ignore").map(|attribute| argument(&attribute.text, "ignore", None));
        let start = function.name.span.start();
        Some(Self {
            modules: source.modules().into(),
            name: function.name.text.clone(),
            location: Location {
                path: source.path().into(),
                line: start.line().unwrap_or(1),
                column: start.position().unwrap_or(1),
            },
            expected,
            ignored,
        })
    }

    /// Whether `key` names this test's function.
    pub(crate) fn is(&self, key: &item::Key) -> bool {
        key.kind() == item::Kind::Function
            && key.owner().is_none()
            && key.modules() == &*self.modules
            && key.name() == self.name.as_str()
    }

    /// This test, at the compiled function `function`.
    pub(crate) fn compiled(self, function: ImmutableString) -> Test {
        let name = self
            .modules
            .iter()
            .map(ImmutableString::as_str)
            .chain([self.name.as_str()])
            .collect::<Vec<_>>()
            .join("::")
            .into_boxed_str();
        Test {
            name,
            function,
            location: self.location,
            expected: self.expected,
            ignored: self.ignored,
        }
    }
}

/// The string an attribute's text gives after its `name`: `name = "text"`,
/// or with `key`, `name(key = "text")`. `None` when it gives none.
fn argument(text: &str, name: &str, key: Option<&str>) -> Option<Box<str>> {
    let rest = text.strip_prefix(name)?.trim_start();
    let rest = match key {
        Some(key) if rest.starts_with('(') => rest[1..].trim_start().strip_prefix(key)?,
        _ => rest,
    };
    literal(rest.trim_start().strip_prefix('=')?.trim_start())
}

/// The value of the Rust string literal `text` starts with.
fn literal(text: &str) -> Option<Box<str>> {
    let mut characters = text.strip_prefix('"')?.chars();
    let mut value = String::new();
    loop {
        match characters.next()? {
            '"' => return Some(value.into_boxed_str()),
            '\\' => match characters.next()? {
                'n' => value.push('\n'),
                't' => value.push('\t'),
                'r' => value.push('\r'),
                '0' => value.push('\0'),
                // A line continuation: the newline and the next line's
                // leading whitespace are not in the string.
                '\n' => characters = characters.as_str().trim_start().chars(),
                escaped => value.push(escaped),
            },
            character => value.push(character),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::argument;

    #[test]
    fn should_panic_gives_its_expected_text() {
        let expected = |text| argument(text, "should_panic", Some("expected"));
        assert_eq!(expected("should_panic"), None);
        assert_eq!(
            expected(r#"should_panic(expected = "Divide by \"zero\"")"#).as_deref(),
            Some(r#"Divide by "zero""#)
        );
        assert_eq!(expected(r#"should_panic = "x""#).as_deref(), Some("x"));
    }

    #[test]
    fn ignore_gives_its_reason() {
        assert_eq!(argument("ignore", "ignore", None), None);
        assert_eq!(
            argument(r#"ignore = "slow""#, "ignore", None).as_deref(),
            Some("slow")
        );
    }
}
