//! A compile's syntax tree, spliced from what an earlier compile parsed of
//! each part it kept (#1877).
//!
//! The lowering writes the program's text one source's part after
//! another, then the dispatchers, then the root's statements. Each part's
//! text is a run of functions, and the last's the statements: none reads
//! another's names, as a function body never reads a script's variables.
//! So each piece of the text parses alone to what the whole text's parse
//! holds of it, its positions the whole text's when its tokens are counted
//! from where it starts there. A part a runner's compile kept keeps what it
//! parsed of its text, before its names were rewritten; a later compile
//! whose part's text is the same, byte for byte, takes that parse again,
//! its lines moved to where the piece starts now, and parses only the
//! pieces whose text changed. A closure keeps the name its first parse
//! gave it; its value carries its own function, moved with it.
//!
//! A text optimized whole by a `Full` level folds a call by the functions
//! the whole text defines, so it is parsed whole, as a text whose scope
//! holds constants is.

use super::kept::{Part, Session};
use super::location::Generated;
use super::relocate::{relocate_block, relocate_statements};
use crate::ast::{ScriptFuncDef, StmtBlockContainer};
use crate::parser::ParseResult;
use crate::program::source;
use crate::{Engine, OptimizationLevel, Position, Scope, Shared, AST};

/// What a compile parsed of one kept part's text, before its names were
/// rewritten, and where in the whole text that text started.
pub(super) struct Parsed {
    text: Box<str>,
    at: Position,
    /// How many lines the text ends after the one it starts on.
    lines: usize,
    level: OptimizationLevel,
    statements: StmtBlockContainer,
    functions: Box<[Shared<ScriptFuncDef>]>,
}

impl Parsed {
    /// This parse of `text`, when it parsed that text at `level` from a
    /// place `at` whose column is the same, moved to start there.
    fn moved(&self, text: &str, at: Position, level: OptimizationLevel) -> Option<Parse> {
        let (Some(from), Some(to)) = (self.at.line(), at.line()) else {
            return None;
        };
        // A line past the last a position holds is the last: a parse that
        // reached it can't say where its lines were.
        if *self.text != *text
            || self.level != level
            || self.at.position() != at.position()
            || from + self.lines >= usize::from(u16::MAX)
        {
            return None;
        }
        let mut statements = self.statements.clone();
        let mut functions = self.functions.to_vec();
        if from != to {
            // Each position is on the piece's lines, from `from` on.
            let mut locate = |position: Position| match position.line() {
                Some(line) => Position::new(
                    u16::try_from((line + to).saturating_sub(from).max(1)).unwrap_or(u16::MAX),
                    u16::try_from(position.position().unwrap_or(0)).unwrap_or(u16::MAX),
                ),
                None => position,
            };
            relocate_statements(&mut statements, &mut locate);
            for function in &mut functions {
                let function = crate::func::shared_make_mut(function);
                relocate_block(&mut function.body, &mut locate);
            }
        }
        Some(Parse {
            statements,
            functions,
        })
    }
}

/// One piece's parse, as the whole text's holds it.
struct Parse {
    statements: StmtBlockContainer,
    functions: Vec<Shared<ScriptFuncDef>>,
}

/// The pieces of a compile's generated text, in order.
#[derive(Default)]
pub(super) struct Pieces(Vec<Piece>);

/// One piece of a compile's generated text: where it ends, and the kept
/// part it is, when it is one.
struct Piece {
    end: usize,
    part: Option<(source::Id, Part)>,
}

impl Pieces {
    /// The text written since the last piece is the next, ending at `end`:
    /// `part` of a source, or text no part keeps.
    pub(super) fn written(&mut self, end: usize, part: Option<(source::Id, Part)>) {
        self.0.push(Piece { end, part });
    }

    /// The syntax tree of `generated` as its whole text parses, spliced
    /// from `session`'s kept parses of the pieces whose text didn't change,
    /// each piece parsed again keeping its parse there. `None` when the
    /// whole text is parsed whole (the module's documentation).
    pub(super) fn parse(
        &self,
        engine: &Engine,
        scope: Option<&Scope>,
        generated: &Generated,
        session: &mut Session<'_>,
        level: OptimizationLevel,
    ) -> ParseResult<Option<AST>> {
        let text = generated.as_str();
        if level == OptimizationLevel::Full
            || scope.is_some_and(|scope| !scope.is_empty())
            || self.0.last().map(|piece| piece.end) != Some(text.len())
        {
            return Ok(None);
        }
        let mut statements = StmtBlockContainer::new();
        let mut functions = Vec::new();
        let mut start = 0;
        // Where `start` is: its line, and the byte its line starts at.
        let mut line = 1;
        let mut line_start = 0;
        for (index, &Piece { end, part }) in self.0.iter().enumerate() {
            let Some(piece) = text.get(start..end) else {
                return Ok(None);
            };
            if piece.is_empty() {
                continue;
            }
            let at = Position::new(
                u16::try_from(line).unwrap_or(u16::MAX),
                u16::try_from(text[line_start..start].chars().count()).unwrap_or(u16::MAX),
            );
            let lines = piece.bytes().filter(|byte| *byte == b'\n').count();
            let kept = part.and_then(|(source, part)| session.parsed(source, part));
            let parse = match kept.and_then(|kept| kept.moved(piece, at, level)) {
                Some(parse) => parse,
                None => {
                    let (statements, module) =
                        engine.compile_legacy_piece(scope, piece, at, level)?;
                    let parse = Parse {
                        statements,
                        functions: module
                            .iter_script_fn()
                            .map(|(.., function)| Shared::clone(function))
                            .collect(),
                    };
                    if let Some((source, part)) = part {
                        session.keep_parsed(
                            source,
                            part,
                            Parsed {
                                text: piece.into(),
                                at,
                                lines,
                                level,
                                statements: parse.statements.clone(),
                                functions: parse.functions.clone().into_boxed_slice(),
                            },
                        );
                    }
                    parse
                }
            };
            // Only the last piece, the root's statements, has statements:
            // one before it would bind a variable the whole text's later
            // statements read.
            if !parse.statements.is_empty() && index + 1 < self.0.len() {
                return Ok(None);
            }
            statements.extend(parse.statements);
            functions.extend(parse.functions);
            if let Some(last) = piece.rfind('\n') {
                line_start = start + last + 1;
            }
            line += lines;
            start = end;
        }
        Ok(Some(AST::new(statements, crate::Module::from(functions))))
    }
}

#[cfg(test)]
mod tests {
    use super::Parsed;
    use crate::{Engine, OptimizationLevel, Position};

    const FIRST: &str = "fn first(a) {\n    let b = a + 1;\n    for (item, index) in a {\n        b += item * index;\n    }\n    b * 2\n}\n";
    const SECOND: &str = "fn second(x) {\n    let total = 0;\n    for item in x {\n        total += item;\n    }\n    let add = |y| y + total;\n    add.call(3)\n}\n";

    /// Each function's body as the parse holds it, its positions written.
    fn bodies(module: &crate::SharedModule) -> Vec<String> {
        let mut bodies = module
            .iter_script_fn()
            .map(|(.., function)| format!("{} {:?}", function.name, function.body))
            .collect::<Vec<_>>();
        bodies.sort();
        bodies
    }

    #[test]
    fn a_piece_parsed_where_it_starts_is_what_the_whole_text_holds_of_it() {
        let engine = Engine::new();
        let level = OptimizationLevel::Simple;
        let whole = format!("{FIRST}{SECOND}");
        let ast = engine
            .compile_legacy_script_with_scope_raw(None, &whole, level)
            .unwrap();
        let (_, first) = engine
            .compile_legacy_piece(None, FIRST, Position::new(1, 0), level)
            .unwrap();
        let (_, second) = engine
            .compile_legacy_piece(None, SECOND, Position::new(8, 0), level)
            .unwrap();
        let mut pieces = bodies(&first);
        pieces.extend(bodies(&second));
        pieces.sort();
        assert_eq!(pieces, bodies(ast.shared_lib()));
    }

    #[test]
    fn a_kept_piece_moved_to_where_its_text_starts_now_is_its_parse_there() {
        let engine = Engine::new();
        let level = OptimizationLevel::Simple;
        let (statements, module) = engine
            .compile_legacy_piece(None, FIRST, Position::new(1, 0), level)
            .unwrap();
        let parsed = Parsed {
            text: FIRST.into(),
            at: Position::new(1, 0),
            lines: FIRST.matches('\n').count(),
            level,
            statements,
            functions: module
                .iter_script_fn()
                .map(|(.., function)| function.clone())
                .collect(),
        };
        let moved = parsed
            .moved(FIRST, Position::new(9, 0), level)
            .expect("the same text at the same column moves");
        let (_, there) = engine
            .compile_legacy_piece(None, FIRST, Position::new(9, 0), level)
            .unwrap();
        assert_eq!(
            bodies(&crate::Module::from(moved.functions).into()),
            bodies(&there)
        );
        // Another text, or another column, is parsed again.
        assert!(parsed.moved(SECOND, Position::new(9, 0), level).is_none());
        assert!(parsed.moved(FIRST, Position::new(9, 4), level).is_none());
    }
}
