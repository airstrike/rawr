//! Code a reader writes, compiled into a program at one place and then run
//! on its own: the code a paused frame evaluates (#490), and each entry of
//! a REPL session (#1207).
//!
//! The program's own compiler reads the code where it is written, so its
//! types, borrows and errors are those of a statement written there. Only
//! the code's own statements then run, against the values the program
//! holds. The code's last expression, its tail, is framed so its value's
//! `{:?}` text is kept: `{:?}` borrows what it prints, as `println!` does,
//! so reading a binding never moves it.
//!
//! The text the compiler reads is made of pieces of the texts a reader
//! sees (a file, the code) and of rawr's frame. [`Spliced`] keeps where
//! each piece is shown, so an error in it is reported where the reader
//! wrote it.

use crate::ast::Stmt;
use crate::diagnostic::{Report, Source};
use crate::tokenizer::Token;
use crate::{Engine, Position, Span};

/// The binding a frame keeps its tail's `{:?}` text in.
pub(crate) const VALUE: &str = "__rawr_eval";

/// Code split as Rust reads a block: its statements, then the tail
/// expression whose value the block has, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Code<'a> {
    pub(crate) statements: &'a str,
    pub(crate) tail: Option<&'a str>,
    /// Where the tail starts in the code.
    tail_at: usize,
}

/// Split `code` after its last statement: what follows the last `;` outside
/// any bracket is the tail, unless it is itself a statement or an item.
pub(crate) fn split<'a>(engine: &Engine, code: &'a str) -> Code<'a> {
    let mut depth = 0usize;
    let mut end = 0;
    for (token, position) in crate::resolve::lower_nominal::lex_rust(engine, code) {
        match token {
            Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart => {
                depth += 1
            }
            Token::RightBrace | Token::RightParen | Token::RightBracket => {
                depth = depth.saturating_sub(1);
            }
            Token::SemiColon if depth == 0 => {
                end = offset(code, position) + 1;
            }
            Token::EOF => break,
            _ => {}
        }
    }
    let rest = &code[end..];
    let trimmed = rest.trim();
    if trimmed.is_empty() || is_statement(trimmed) {
        return Code {
            statements: code,
            tail: None,
            tail_at: code.len(),
        };
    }
    let tail_at = end + (rest.len() - rest.trim_start().len());
    Code {
        statements: &code[..end],
        tail: Some(trimmed),
        tail_at,
    }
}

/// Whether `text`, code after the last `;`, is a statement or an item
/// rather than an expression.
fn is_statement(text: &str) -> bool {
    if text.starts_with("#[") || text.starts_with("#![") {
        return true;
    }
    let word: String = text
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    matches!(
        word.as_str(),
        "let"
            | "fn"
            | "struct"
            | "enum"
            | "union"
            | "impl"
            | "trait"
            | "use"
            | "mod"
            | "const"
            | "static"
            | "type"
            | "pub"
            | "extern"
            | "macro_rules"
    )
}

/// The byte offset in `text` of `position`, a one-based line and character
/// column.
fn offset(text: &str, position: Position) -> usize {
    crate::diagnostic::offset(
        text,
        position.line().unwrap_or(1),
        position.position().unwrap_or(1),
    )
}

/// The position of byte `at` of `text`: its one-based line and character
/// column.
fn position_of(text: &str, at: usize) -> Position {
    let before = &text[..at.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |line| line.chars().count())
        + 1;
    Position::new(
        u16::try_from(line).unwrap_or(u16::MAX),
        u16::try_from(column).unwrap_or(u16::MAX),
    )
}

/// One piece of a spliced text.
#[derive(Clone, Copy, Debug)]
struct Piece {
    /// Where it starts in the spliced text.
    at: usize,
    /// The shown text it comes from, and where it starts there.
    shown: usize,
    from: usize,
    /// rawr wrote it: all of it is shown at `from`.
    written: bool,
}

/// A text the compiler reads, made of pieces of the texts a reader sees and
/// of rawr's frames.
#[derive(Clone, Debug, Default)]
pub(crate) struct Spliced {
    text: String,
    pieces: Vec<Piece>,
}

impl Spliced {
    /// The text the compiler reads.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Add `text`, which shown text `shown` has at byte `from`.
    pub(crate) fn copy(&mut self, shown: usize, from: usize, text: &str) {
        self.piece(shown, from, text, false);
    }

    /// Add rawr's own `text`, shown at byte `from` of `shown`.
    pub(crate) fn write(&mut self, shown: usize, from: usize, text: &str) {
        self.piece(shown, from, text, true);
    }

    fn piece(&mut self, shown: usize, from: usize, text: &str, written: bool) {
        self.pieces.push(Piece {
            at: self.text.len(),
            shown,
            from,
            written,
        });
        self.text.push_str(text);
    }

    /// Add `code`, which shown text `shown` has at byte `from`: its
    /// statements, then a frame that keeps its tail's `{:?}` text in
    /// [`VALUE`]. A `scoped` frame holds the statements too, so the
    /// bindings they make end with it. Returns where the frame starts, or
    /// `None` when there is none: unscoped code without a tail.
    pub(crate) fn frame(
        &mut self,
        shown: usize,
        from: usize,
        code: &Code<'_>,
        scoped: bool,
    ) -> Option<usize> {
        let mut start = None;
        if scoped {
            start = Some(self.text.len());
            self.write(shown, from, "{ ");
        }
        self.copy(shown, from, code.statements);
        if !scoped && code.tail.is_none() {
            return None;
        }
        let tail_at = from + code.tail_at;
        if !scoped {
            start = Some(self.text.len());
            self.write(shown, tail_at, "{ ");
        }
        // The tail is parenthesized: `format!` reads `name = value` as a
        // named argument, and in Rust an assignment is an expression of
        // type `()`. A parenthesized place is still a place, so the borrow
        // `format!` takes doesn't move it.
        self.write(
            shown,
            tail_at,
            &format!(" let {VALUE}: ::std::string::String = ::std::format!(\"{{:?}}\", ("),
        );
        match code.tail {
            Some(tail) => self.copy(shown, tail_at, tail),
            None => self.write(shown, tail_at, "()"),
        }
        let end = from + code.tail_at + code.tail.map_or(0, str::len);
        // A statement, never the tail of what holds it.
        self.write(shown, end, ")); }; ");
        start
    }

    /// Where byte `at` of the spliced text is shown: the shown text, and
    /// the byte there.
    fn shown(&self, at: usize) -> (usize, usize) {
        let index = self
            .pieces
            .partition_point(|piece| piece.at <= at)
            .saturating_sub(1);
        let Some(piece) = self.pieces.get(index) else {
            return (0, at);
        };
        if piece.written {
            (piece.shown, piece.from)
        } else {
            (piece.shown, piece.from + (at - piece.at))
        }
    }

    /// The position in the spliced text of byte `at`.
    pub(crate) fn position(&self, at: usize) -> Position {
        position_of(&self.text, at)
    }

    /// `report`, whose labels in `spliced` (a source of this text) are
    /// moved to where `shown` shows them.
    pub(crate) fn relabel(&self, report: &Report, spliced: &str, shown: &[Source]) -> Report {
        report.relabeled(|source, span| {
            // A source's text may go on with what its macros expand to.
            if source.identity() != spliced || !source.text().starts_with(self.text.as_str()) {
                return None;
            }
            let start = self.shown(offset(&self.text, span.start()));
            let end = self.shown(offset(&self.text, span.end()));
            let end = if end.0 == start.0 && end.1 >= start.1 {
                end.1
            } else {
                start.1
            };
            let target = shown.get(start.0)?;
            Some((
                target.clone(),
                Span::new(
                    position_of(target.text(), start.1),
                    position_of(target.text(), end),
                ),
            ))
        })
    }
}

/// The statements a frame that starts at `start` holds, in `statements` or
/// nested in them; `place` gives a statement's position in the spliced
/// text.
pub(crate) fn framed<'a>(
    statements: impl IntoIterator<Item = &'a Stmt>,
    start: Position,
    place: &dyn Fn(Position) -> Option<Position>,
) -> Option<Vec<Stmt>> {
    let mut found = None;
    let mut path = Vec::new();
    for statement in statements {
        statement.walk(&mut path, &mut |path: &[crate::ast::ASTNode]| {
            if found.is_some() {
                return false;
            }
            if let Some(crate::ast::ASTNode::Stmt(Stmt::Block(block))) = path.last()
                && place(block.position()) == Some(start)
            {
                found = Some(block.iter().cloned().collect());
                return false;
            }
            true
        });
        path.clear();
        if found.is_some() {
            break;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_splits_after_its_last_statement() {
        let engine = Engine::new();
        let split = |code| {
            let code = super::split(&engine, code);
            (code.statements, code.tail)
        };
        assert_eq!(split("x + 1"), ("", Some("x + 1")));
        assert_eq!(
            split("let y = { 1; 2 }; y * 2 "),
            ("let y = { 1; 2 };", Some("y * 2"))
        );
        assert_eq!(split("v.push(1);"), ("v.push(1);", None));
        assert_eq!(split("let z = 3"), ("let z = 3", None));
        assert_eq!(split("fn f() {}"), ("fn f() {}", None));
        assert_eq!(split("f(\";\")"), ("", Some("f(\";\")")));
    }

    #[test]
    fn a_frame_shows_its_code_where_it_was_written() {
        let engine = Engine::new();
        let code = super::split(&engine, "let a = 1; a + b");
        let mut spliced = Spliced::default();
        spliced.copy(0, 0, "fn main() {\n    ");
        let start = spliced.frame(1, 0, &code, true);
        spliced.copy(0, 16, "f();\n}\n");
        assert_eq!(start, Some(16));
        let text = spliced.text().to_owned();
        let b = text.find("a + b").unwrap() + 4;
        assert_eq!(spliced.shown(b), (1, 15));
        let after = text.find("f();").unwrap();
        assert_eq!(spliced.shown(after), (0, 16));
        // rawr's frame is shown where the tail starts.
        let frame = text.find("format").unwrap();
        assert_eq!(spliced.shown(frame), (1, 11));
    }
}
