//! Where a program can pause (#489): the first step of each statement the
//! program wrote, with its file and line, and the breakpoints a tool set
//! on a line or a function.
//!
//! A breakpoint on a line keeps the line's text. When the program is
//! compiled again, after a save, the breakpoint stays on that line while
//! the line still has that text. Otherwise it moves to the nearest line
//! with that text, and it is removed when no line has it.

use std::collections::HashMap;
use std::mem::Discriminant;

use serde_json::{json, Value};

use super::host::Files;
use crate::ast::{ASTNode, Expr, Stmt};
use crate::{Position, AST};

/// Which node a step is at: a statement's or an expression's kind, so a
/// node nested at the same position doesn't pause again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Stmt(Discriminant<Stmt>),
    Expr(Discriminant<Expr>),
}

impl Kind {
    fn of(node: &ASTNode) -> Self {
        match node {
            ASTNode::Stmt(stmt) => Self::Stmt(std::mem::discriminant(*stmt)),
            ASTNode::Expr(expr) => Self::Expr(std::mem::discriminant(*expr)),
        }
    }
}

/// A statement's first step: its file and line.
#[derive(Clone, Debug)]
pub(super) struct Stop {
    pub file: String,
    pub line: usize,
    kind: Kind,
}

/// The steps of one compiled program where it can pause.
#[derive(Default)]
pub(super) struct Stops {
    at: HashMap<Position, Stop>,
    /// Each function, as Rust names it, with its first statement's step.
    functions: Vec<(String, Position)>,
    /// The function each closure is written in, by the closure's name.
    closures: HashMap<String, String>,
}

impl Stops {
    /// The statements of `ast` that `locate` places in the program's
    /// files. A statement nested in another on the same line is not one:
    /// the outer one pauses for the line.
    pub fn of(ast: &AST, locate: &dyn Fn(Position) -> Option<(String, usize)>) -> Self {
        let mut stops = Self::default();
        stops.collect(ast.statements().iter(), locate);
        for function in ast.iter_fn_def() {
            if let Some(first) = stops.collect(function.body.iter(), locate) {
                let name =
                    crate::resolve::lower_nominal::function_display_name(&function.name).to_owned();
                stops.functions.push((name, first));
            }
            let mut path = Vec::new();
            for statement in function.body.iter() {
                statement.walk(&mut path, &mut |path: &[ASTNode]| {
                    if let Some(ASTNode::Expr(Expr::DynamicConstant(value, _))) = path.last()
                        && let Some(pointer) = value.read_lock::<crate::FnPtr>()
                        && pointer.is_anonymous()
                    {
                        // A root function the program also names by its
                        // own name has one body: its path names it.
                        let within = stops
                            .closures
                            .entry(pointer.fn_name().to_owned())
                            .or_insert_with(|| function.name.to_string());
                        if !within.contains("::") {
                            function.name.as_str().clone_into(within);
                        }
                    }
                    true
                });
                path.clear();
            }
        }
        stops
    }

    /// The function `name` as Rust names it: a closure as the function it
    /// is written in names it, `main::run::{{closure}}`.
    pub fn function_name(&self, name: &str) -> String {
        match self.closures.get(name) {
            Some(within) => format!("{}::{{{{closure}}}}", self.function_name(within)),
            None => crate::resolve::lower_nominal::function_display_name(name).to_owned(),
        }
    }

    /// Add the stops in `statements`; the first one's position.
    fn collect<'a>(
        &mut self,
        statements: impl Iterator<Item = &'a Stmt>,
        locate: &dyn Fn(Position) -> Option<(String, usize)>,
    ) -> Option<Position> {
        let mut first = None;
        let mut path = Vec::new();
        for statement in statements {
            statement.walk(&mut path, &mut |path: &[ASTNode]| {
                let Some((node, outer)) = path.split_last() else {
                    return true;
                };
                let candidate = match node {
                    ASTNode::Stmt(Stmt::Expr(..) | Stmt::Block(..) | Stmt::Noop(..)) => false,
                    ASTNode::Stmt(..) => true,
                    // `()` runs nothing: a block's value when it has no tail.
                    ASTNode::Expr(Expr::Unit(..)) => false,
                    // An expression statement's step is its expression's.
                    ASTNode::Expr(..) => {
                        matches!(outer.last(), Some(ASTNode::Stmt(Stmt::Expr(..))))
                    }
                };
                if !candidate {
                    return true;
                }
                let position = node.position();
                let Some((file, line)) = locate(position) else {
                    return true;
                };
                let nested = outer.iter().any(|enclosing| {
                    self.at
                        .get(&enclosing.position())
                        .is_some_and(|stop| stop.file == file && stop.line == line)
                });
                if !nested && !self.at.contains_key(&position) {
                    self.at.insert(
                        position,
                        Stop {
                            file,
                            line,
                            kind: Kind::of(node),
                        },
                    );
                    first.get_or_insert(position);
                }
                true
            });
            path.clear();
        }
        first
    }

    /// The stop a step at `position` on `node` is at.
    pub fn get(&self, position: Position, node: &ASTNode) -> Option<&Stop> {
        self.at
            .get(&position)
            .filter(|stop| stop.kind == Kind::of(node))
    }

    /// The file and line of the stop at `position`.
    pub fn located(&self, position: Position) -> Option<(String, usize)> {
        self.at
            .get(&position)
            .map(|stop| (stop.file.clone(), stop.line))
    }

    /// The stops on `line` of `file`.
    fn on_line(&self, file: &str, line: usize) -> Vec<Position> {
        let mut found: Vec<Position> = self
            .at
            .iter()
            .filter(|(_, stop)| stop.file == file && stop.line == line)
            .map(|(position, _)| *position)
            .collect();
        found.sort_unstable();
        found
    }

    /// Whether any statement of `file` is in the program.
    fn has_file(&self, file: &str) -> bool {
        self.at.values().any(|stop| stop.file == file)
    }

    /// The first statements of the functions named `name`: a whole path as
    /// Rust names it (`counter::tick`), or its last segments (`tick`).
    fn function(&self, name: &str) -> Vec<Position> {
        self.functions
            .iter()
            .filter(|(full, _)| {
                full == name
                    || full
                        .strip_suffix(name)
                        .is_some_and(|before| before.ends_with("::"))
            })
            .map(|(_, position)| *position)
            .collect()
    }
}

/// What a breakpoint was set on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Spec {
    /// A line of a file, with the line's text when it was set.
    Line {
        file: String,
        line: usize,
        text: String,
    },
    /// A function's first statement.
    Function { name: String },
}

impl Spec {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Line { file, line, text } => json!({ "file": file, "line": line, "text": text }),
            Self::Function { name } => json!({ "function": name }),
        }
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        if let Some(name) = value.get("function").and_then(Value::as_str) {
            return Some(Self::Function {
                name: name.to_owned(),
            });
        }
        Some(Self::Line {
            file: value.get("file")?.as_str()?.to_owned(),
            line: usize::try_from(value.get("line")?.as_u64()?).ok()?,
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        })
    }

    /// Whether a `clear` naming `file`, `line` and `function` takes this
    /// breakpoint: each one given must match.
    pub fn cleared_by(
        &self,
        file: Option<&str>,
        line: Option<usize>,
        function: Option<&str>,
    ) -> bool {
        match self {
            Self::Line {
                file: at, line: on, ..
            } => {
                function.is_none()
                    && file.is_none_or(|file| file == at)
                    && line.is_none_or(|line| line == *on)
            }
            Self::Function { name } => {
                file.is_none() && line.is_none() && function.is_none_or(|function| function == name)
            }
        }
    }
}

/// A breakpoint and the steps it pauses at in the compiled program.
#[derive(Clone, Debug)]
pub(super) struct Point {
    /// What names this breakpoint while its line moves: the revision of
    /// the breakpoints that set it.
    pub id: u64,
    pub spec: Spec,
    pub at: Vec<Position>,
}

impl Point {
    /// The breakpoint as the breakpoints file and its events list it: its
    /// spec's fields and its `id`.
    pub fn to_json(&self) -> Value {
        let mut listed = self.spec.to_json();
        listed["id"] = json!(self.id);
        listed
    }

    /// A breakpoint the breakpoints file lists, its steps not yet placed.
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            id: value.get("id")?.as_u64()?,
            spec: Spec::from_json(value)?,
            at: Vec::new(),
        })
    }
}

/// A breakpoint `id` set on `spec` in the program `stops` holds, its line's
/// text read from `files`, or why it can't be. Without `stops`, the
/// program hasn't compiled yet and the breakpoint waits for it.
pub(super) fn set(
    id: u64,
    spec: Spec,
    stops: Option<&Stops>,
    files: &Files,
) -> Result<Point, String> {
    let spec = match spec {
        Spec::Line { file, line, .. } => {
            let text = files
                .read(&file)
                .map_err(|error| format!("cannot read {file}: {error}"))?;
            let count = text.lines().count();
            let Some(written) = line
                .checked_sub(1)
                .and_then(|index| text.lines().nth(index))
            else {
                return Err(format!("{file} has no line {line}: it has {count} lines"));
            };
            Spec::Line {
                text: written.trim().to_owned(),
                file,
                line,
            }
        }
        spec @ Spec::Function { .. } => spec,
    };
    let Some(stops) = stops else {
        return Ok(Point {
            id,
            spec,
            at: Vec::new(),
        });
    };
    let at = positions(&spec, stops);
    if at.is_empty() {
        return Err(match &spec {
            Spec::Line { file, .. } if !stops.has_file(file) => {
                format!("{file} isn't one of the program's source files, or has no statements")
            }
            Spec::Line { file, line, .. } => {
                format!("line {line} of {file} has no statement to stop at")
            }
            Spec::Function { name } => {
                format!("cannot find function `{name}` in this program, or it has no statements")
            }
        });
    }
    Ok(Point { id, spec, at })
}

/// `point` in a program compiled again: its line moved to where its text
/// is now in `files`, and its steps there, under the same id. `None` when
/// its line's text is gone.
pub(super) fn moved(point: &Point, stops: &Stops, files: &Files) -> Option<Point> {
    let spec = match &point.spec {
        Spec::Line { file, line, text } => {
            let source = files.read(file).ok()?;
            let lines: Vec<&str> = source.lines().map(str::trim).collect();
            let line = nearest(&lines, *line, text)?;
            Spec::Line {
                file: file.clone(),
                line,
                text: text.clone(),
            }
        }
        spec @ Spec::Function { .. } => spec.clone(),
    };
    Some(Point {
        id: point.id,
        at: positions(&spec, stops),
        spec,
    })
}

/// The line (from 1) nearest `line` whose text is `text`; on a tie, the
/// later one.
fn nearest(lines: &[&str], line: usize, text: &str) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, written)| **written == text)
        .map(|(index, _)| index + 1)
        .min_by_key(|found| (found.abs_diff(line), std::cmp::Reverse(*found)))
}

fn positions(spec: &Spec, stops: &Stops) -> Vec<Position> {
    match spec {
        Spec::Line { file, line, .. } => stops.on_line(file, *line),
        Spec::Function { name } => stops.function(name),
    }
}

#[cfg(test)]
mod tests {
    use super::{nearest, Spec};

    #[test]
    fn a_closure_is_named_by_the_function_it_is_written_in() {
        let mut engine = crate::Engine::new();
        #[allow(deprecated)]
        engine.register_debugger(
            |_, debugger| debugger,
            |_, _, _, _, _| Ok(crate::debugger::DebuggerCommand::Continue),
        );
        let ast = engine
            .compile("pub fn run() -> i64 { let f = |x: i64| x + 1; f(2) }\nrun()")
            .unwrap_or_else(|error| panic!("{error}"));
        let stops = super::Stops::of(&ast, &|_| None);
        let closure = ast
            .iter_fn_def()
            .find(|function| crate::parser::is_anonymous_fn(&function.name))
            .expect("the closure is a function of the program");
        // The function by its path, when the program also names it so.
        let run = ast
            .iter_fn_def()
            .filter(|function| !crate::parser::is_anonymous_fn(&function.name))
            .map(|function| crate::resolve::lower_nominal::function_display_name(&function.name))
            .max_by_key(|name| name.len())
            .expect("`run` is a function of the program");
        assert!(run.ends_with("run"), "{run}");
        assert_eq!(
            stops.function_name(&closure.name),
            format!("{run}::{{{{closure}}}}")
        );
        assert_eq!(
            stops.function_name("\0rawr.function.main::run"),
            "main::run"
        );
        assert_eq!(
            stops.function_name("\0rawr.method.main::Counter::bump"),
            "main::Counter::bump"
        );
        assert_eq!(
            stops.function_name("\0rawr.closure.main::run{closure#0}"),
            "main::run{closure#0}"
        );
    }

    #[test]
    fn a_moved_line_is_found_by_its_text_nearest_where_it_was() {
        let lines = ["fn main() {", "tick();", "", "tick();", "}"];
        assert_eq!(nearest(&lines, 2, "tick();"), Some(2));
        assert_eq!(nearest(&lines, 3, "tick();"), Some(4));
        assert_eq!(nearest(&lines, 5, "tick();"), Some(4));
        assert_eq!(nearest(&lines, 2, "gone();"), None);
    }

    #[test]
    fn clear_takes_the_breakpoints_each_given_field_matches() {
        let line = Spec::Line {
            file: "/a.rs".into(),
            line: 3,
            text: String::new(),
        };
        let function = Spec::Function {
            name: "tick".into(),
        };
        assert!(line.cleared_by(None, None, None));
        assert!(line.cleared_by(Some("/a.rs"), Some(3), None));
        assert!(!line.cleared_by(Some("/a.rs"), Some(4), None));
        assert!(!line.cleared_by(None, None, Some("tick")));
        assert!(function.cleared_by(None, None, Some("tick")));
        assert!(!function.cleared_by(Some("/a.rs"), None, None));
        assert_eq!(Spec::from_json(&line.to_json()), Some(line));
        assert_eq!(Spec::from_json(&function.to_json()), Some(function));
    }
}
