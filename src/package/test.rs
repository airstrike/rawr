//! A crate's `#[test]` functions, as `rustc --test` collects them for its
//! harness: each with its path from the crate root, what it must do to pass
//! (`#[should_panic]`) and whether it is left out (`#[ignore]`), and the
//! harness that runs them ([`harness`]).

pub mod harness;

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

    /// Whether `key` names this test's function: its path from the crate
    /// root is the test's name.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    pub(crate) fn names(&self, key: &item::Key) -> bool {
        let mut path = self.name.split("::");
        key.kind() == item::Kind::Function
            && key.owner().is_none()
            && key
                .modules()
                .iter()
                .all(|module| path.next() == Some(module.as_str()))
            && path.next() == Some(key.name())
            && path.next().is_none()
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

    /// The tests `declared` names, each at its function in `executable`,
    /// sorted by name, as libtest runs them.
    pub(crate) fn compiled_in(
        declared: Vec<Self>,
        executable: &crate::program::Executable,
    ) -> Box<[Test]> {
        let mut tests = declared
            .into_iter()
            .filter_map(|test| {
                let item = executable.items().find(|item| {
                    item.key().package() == executable.package() && test.is(item.key())
                })?;
                let function = crate::resolve::lower_nominal::private_function_name(item.key());
                Some(test.compiled(function))
            })
            .collect::<Vec<_>>();
        tests.sort_by(|left, right| left.name().cmp(right.name()));
        tests.into_boxed_slice()
    }

    /// Whether `key` names this test's function.
    fn is(&self, key: &item::Key) -> bool {
        key.kind() == item::Kind::Function
            && key.owner().is_none()
            && key.modules() == &*self.modules
            && key.name() == self.name.as_str()
    }

    /// This test, at the compiled function `function`.
    fn compiled(self, function: ImmutableString) -> Test {
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
    use super::{argument, Expected, Test};
    use crate::package::{snapshot::Snapshot, Graph};
    use crate::{CallFnOptions, Dynamic, Engine, Scope, AST};

    const CRATE: &str = r#"
fn main() {
    println!("{}", add(1, 2));
}

fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[test]
fn top_level() {
    assert!(cfg!(test));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        // A module sees the private items of the modules it is in.
        assert_eq!(add(2, 2), Total::new(4).0);
    }

    #[test]
    #[should_panic(expected = "zero")]
    fn divides() {
        let zero = add(0, 0);
        if zero == 0 {
            panic!("divide by zero");
        }
    }

    #[test]
    #[ignore = "slow"]
    fn later() {
        #[cfg(test)]
        let built = true;
        #[cfg(not(test))]
        let built = false;
        assert!(built);
    }

    #[test]
    fn fails() {
        assert_eq!(add(1, 1), 3);
    }
}

struct Total(i64);

impl Total {
    fn new(total: i64) -> Self {
        Self(total)
    }
}
"#;

    /// `text` as a crate's root, built for its tests when `test`.
    fn compiled(text: &str, test: bool) -> (Engine, AST, Box<[Test]>) {
        let mut engine = Engine::new();
        engine.set_test(test);
        let path = std::path::PathBuf::from("/tested/src/main.rs");
        let snapshot = Snapshot::new(path.clone(), vec![(path, text.into())]).unwrap();
        let graph = Graph::load_snapshot(&engine, &snapshot).unwrap();
        let (ast, tests, _) = engine.compile_package_tests(graph).unwrap();
        (engine, ast, tests)
    }

    fn call(engine: &Engine, ast: &AST, test: &Test) -> Result<Dynamic, String> {
        engine
            .call_fn_with_options::<Dynamic>(
                CallFnOptions::new().eval_ast(false),
                &mut Scope::new(),
                ast,
                test.function(),
                (),
            )
            .map_err(|error| error.to_string())
    }

    #[test]
    fn a_crate_built_for_its_tests_has_them_sorted_by_name() {
        let (_, _, tests) = compiled(CRATE, true);
        let names: Vec<_> = tests.iter().map(Test::name).collect();
        assert_eq!(
            names,
            [
                "tests::adds",
                "tests::divides",
                "tests::fails",
                "tests::later",
                "top_level"
            ]
        );
        assert_eq!(tests[0].expected(), &Expected::Return);
        assert_eq!(tests[0].ignored(), None);
        assert_eq!(tests[0].location().line(), 20);
        assert_eq!(tests[0].location().column(), 8);
        assert_eq!(tests[1].expected(), &Expected::Panic(Some("zero".into())));
        assert_eq!(tests[3].ignored(), Some(Some("slow")));
    }

    #[test]
    fn cfg_test_holds_only_when_the_engine_builds_for_tests() {
        let (engine, ast, tests) = compiled(CRATE, false);
        let names: Vec<_> = tests.iter().map(Test::name).collect();
        assert_eq!(names, ["top_level"]);
        // `assert!(cfg!(test))` fails in a build that is not for tests.
        assert!(call(&engine, &ast, &tests[0]).is_err());
    }

    #[test]
    fn each_test_runs_as_a_function_and_its_failures_panic() {
        let (engine, ast, tests) = compiled(CRATE, true);
        let outcome = |name: &str| {
            let test = tests.iter().find(|test| test.name() == name).unwrap();
            call(&engine, &ast, test)
        };
        assert!(outcome("tests::adds").is_ok());
        assert!(outcome("top_level").is_ok());
        assert!(outcome("tests::later").is_ok());
        assert!(outcome("tests::divides")
            .unwrap_err()
            .contains("divide by zero"));
        assert!(outcome("tests::fails")
            .unwrap_err()
            .contains("assertion `left == right` failed"));
    }

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
