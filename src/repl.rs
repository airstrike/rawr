//! A REPL session (#1207): its entries are the statements of one body, in
//! the order they were entered, as if each were written after the last.
//!
//! Each entry is compiled with every entry before it, so its types, its
//! borrows and the items it names are those of the whole body, and its
//! errors are rustc's for that body. Only the entry's own statements then
//! run, against the session's scope. A local lives until the session ends,
//! when the body's end drops what is left of them, newest first, as Rust
//! drops a body's locals; a temporary, a `let _` or a moved value drops
//! where Rust drops it. An entry that fails is not kept.

use crate::diagnostic::{self, Report, Source};
use crate::splice::{self, Spliced};
use crate::{Dynamic, Engine, ImmutableString, Position, Scope, AST};

/// The name a session's errors give its entries.
const IDENTITY: &str = "<repl>";

/// The entries a REPL ran, and what running the next one takes.
#[derive(Debug, Default)]
pub struct Session {
    entries: Vec<String>,
}

/// The session's entries as the compiler reads them, with `next` after
/// them, and as a reader sees them.
struct Body {
    spliced: Spliced,
    shown: Source,
    /// Where `next` starts in the spliced text, and its frame, if any.
    next: usize,
    frame: Option<usize>,
}

impl Session {
    /// A session with no entries yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `entry` against `scope`, which holds the values of the entries
    /// before it: the `{:?}` text of its tail, when it has one.
    ///
    /// # Errors
    ///
    /// The entry's compile error as rustc reports it, or its panic,
    /// rendered. The entry is not kept, and `scope` is as it was.
    pub fn run(
        &mut self,
        engine: &Engine,
        scope: &mut Scope,
        entry: &str,
    ) -> Result<Option<String>, String> {
        let body = self.body(engine, Some(entry))?;
        let ast = body.compile(engine)?;
        let start = body.spliced.position(body.next);
        let frame = body.frame.map(|at| body.spliced.position(at));
        let mut statements: Vec<_> = ast
            .statements()
            .iter()
            .skip_while(|statement| !at_or_after(statement.position(), start))
            .cloned()
            .collect();
        // What the body's end writes after it: no position in the text.
        while statements
            .last()
            .is_some_and(|statement| statement.position().is_none())
        {
            statements.pop();
        }
        let framed = match frame {
            Some(frame) => {
                let block = statements.pop();
                Some(
                    block
                        .and_then(|block| {
                            splice::framed([&block], frame, &|position| Some(position))
                        })
                        .unwrap_or_default(),
                )
            }
            None => None,
        };
        let before = scope.len();
        let ran = body
            .run(engine, scope, &ast, statements)
            .and_then(|()| match framed {
                Some(framed) => {
                    let kept = scope.len();
                    let value = body.run(engine, scope, &ast, framed).map(|()| {
                        scope
                            .get_value::<ImmutableString>(splice::VALUE)
                            .map(|text| text.to_string())
                    });
                    scope.rewind(kept);
                    value
                }
                None => Ok(None),
            });
        if ran.is_err() {
            scope.rewind(before);
        } else {
            self.entries.push(entry.to_owned());
        }
        ran.map(|value| value.filter(|text| text != "()"))
    }

    /// End the session: the locals left in `scope` drop, newest first.
    ///
    /// # Errors
    ///
    /// A panic of a `drop`, rendered.
    pub fn end(self, engine: &Engine, scope: &mut Scope) -> Result<(), String> {
        if self.entries.is_empty() {
            return Ok(());
        }
        let body = self.body(engine, None)?;
        let ast = body.compile(engine)?;
        let statements = ast.statements();
        let written = statements
            .iter()
            .rposition(|statement| !statement.position().is_none())
            .map_or(0, |last| last + 1);
        let drops = statements[written..].to_vec();
        body.run(engine, scope, &ast, drops)
    }

    /// The body of the entries, then `next`.
    fn body(&self, engine: &Engine, next: Option<&str>) -> Result<Body, String> {
        let mut spliced = Spliced::default();
        let mut shown = String::new();
        let mut frame = None;
        let mut start = 0;
        for entry in self.entries.iter().map(String::as_str).chain(next) {
            start = spliced.text().len();
            let code = splice::split(engine, entry);
            frame = spliced.frame(0, shown.len(), &code, false);
            shown.push_str(entry);
            spliced.copy(0, shown.len(), "\n");
            shown.push('\n');
        }
        let shown = Source::new(IDENTITY, shown).map_err(|error| error.to_string())?;
        Ok(Body {
            spliced,
            shown,
            next: start,
            frame: next.and(frame),
        })
    }
}

impl Body {
    /// Compile the body, or its error as rustc reports it, rendered.
    fn compile(&self, engine: &Engine) -> Result<AST, String> {
        engine
            .compile_scripts_with_scope_raw(None, [self.spliced.text()], engine.optimization_level)
            .map_err(|error| {
                let report = Source::new(IDENTITY, self.spliced.text())
                    .ok()
                    .and_then(|source| Report::from_parse(source, &error).ok());
                self.rendered(report, || error.to_string())
            })
    }

    /// Run `statements` of `ast` against `scope`, or the panic, rendered.
    fn run(
        &self,
        engine: &Engine,
        scope: &mut Scope,
        ast: &AST,
        statements: Vec<crate::ast::Stmt>,
    ) -> Result<(), String> {
        let mut ast = ast.clone();
        *ast.statements_mut() = statements.into_iter().collect();
        engine
            .eval_ast_with_scope::<Dynamic>(scope, &ast)
            .map(drop)
            .map_err(|error| {
                let report = Source::new(IDENTITY, self.spliced.text())
                    .ok()
                    .and_then(|source| Report::from_evaluation(Some(source), &error).ok());
                self.rendered(report, || error.to_string())
            })
    }

    /// `report`, an error in the spliced text, rendered where the reader
    /// wrote it; `otherwise` without one.
    fn rendered(&self, report: Option<Report>, otherwise: impl FnOnce() -> String) -> String {
        report.map_or_else(otherwise, |report| {
            let shown = self
                .spliced
                .relabel(&report, IDENTITY, std::slice::from_ref(&self.shown));
            diagnostic::render(&shown).trim_end().to_owned()
        })
    }
}

/// Whether `position` is at `start` or after it in the text.
fn at_or_after(position: Position, start: Position) -> bool {
    !position.is_none() && position >= start
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a session prints and answers for `entries`, then at its end,
    /// line by line.
    fn session(entries: &[&str]) -> Vec<String> {
        let printed = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let mut engine = Engine::new();
        let printing = std::sync::Arc::clone(&printed);
        engine.on_output(move |_, text| printing.lock().unwrap().push_str(text));
        let mut session = Session::new();
        let mut scope = Scope::new();
        for entry in entries {
            let answer = match session.run(&engine, &mut scope, entry) {
                Ok(Some(value)) => format!("=> {value}\n"),
                Ok(None) => String::new(),
                Err(error) => format!("error: {error}\n"),
            };
            printed.lock().unwrap().push_str(&answer);
        }
        session.end(&engine, &mut scope).unwrap();
        let printed = printed.lock().unwrap().clone();
        printed.lines().map(str::to_owned).collect()
    }

    const NOISY: &str = "#[derive(Debug)] struct Noisy(&'static str); \
        impl Drop for Noisy { fn drop(&mut self) { println!(\"drop {}\", self.0); } }";

    #[test]
    fn a_session_drops_its_locals_where_a_body_drops_them() {
        // Each entry is a statement of one body: `a` and `b` live to its
        // end and drop newest first, the shadowed `a` too; a temporary and
        // `let _` drop at their statement, a moved value where it moves.
        assert_eq!(
            session(&[
                NOISY,
                "let a = Noisy(\"a\");",
                "println!(\"after a\");",
                "let b = Noisy(\"b\"); let a = Noisy(\"a2\");",
                "let _ = Noisy(\"t\");",
                "Noisy(\"e\");",
                "let c = Noisy(\"c\");",
                "drop(c);",
                "a.0",
                "println!(\"last\");",
            ]),
            [
                "after a",
                "drop t",
                "drop e",
                "drop c",
                "=> \"a2\"",
                "last",
                "drop a2",
                "drop b",
                "drop a",
            ]
        );
    }

    #[test]
    fn an_entry_reads_what_earlier_entries_declared_and_fails_as_rustc_does() {
        let printed = session(&[
            "fn twice(n: i32) -> i32 { n * 2 }",
            "let x = 4;",
            "twice(x) + 1",
            "x + \"a\"",
            "x",
        ]);
        // The error shows the entry as written, at its line of the
        // session, not rawr's frame.
        assert_eq!(
            printed,
            [
                "=> 9",
                "error: error[E0277]: cannot add `&str` to `i32`",
                " --> <repl>:4:3",
                "  |",
                "4 | x + \"a\"",
                "  |   ^ no implementation for `i32 + &str`",
                "  |",
                "  = help: the trait `Add<&str>` is not implemented for `i32`",
                "=> 4",
            ]
        );
    }
}
