//! The kinds of code in the shown file's lines, as a syntax parser finds
//! them. The parser carries its state from line to line, so it runs once
//! over the lines up to the last one drawn and keeps what it found: a draw
//! parses only the lines no draw reached before, and new text starts it
//! again. Each piece's kind is the one `rawr gui`'s highlighter gives it
//! (iced's `iced_highlighter`, whose scope families `FAMILIES` mirrors), so
//! both shells colour code alike.

use std::cell::RefCell;
use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::sync::LazyLock;

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);

/// What a piece of code is, as `rawr gui` tells kinds apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Code {
    Keyword,
    Support,
    Function,
    Type,
    Path,
    String,
    Constant,
    Punctuation,
    Comment,
    Invalid,
    Variable,
    Other,
}

/// The scope families and their kinds, most specific first, so that
/// `entity.name.function` wins over `entity.name`. These are
/// `iced_highlighter`'s.
static FAMILIES: LazyLock<Vec<(Scope, Code)>> = LazyLock::new(|| {
    [
        ("meta.path", Code::Path),
        ("invalid", Code::Invalid),
        ("constant", Code::Constant),
        ("string", Code::String),
        ("comment", Code::Comment),
        ("keyword", Code::Keyword),
        ("storage.type.", Code::Keyword),
        ("storage.type", Code::Type),
        ("storage", Code::Keyword),
        ("entity.name.function", Code::Function),
        ("entity.name", Code::Type),
        ("entity.other.inherited-class", Code::Type),
        ("support", Code::Support),
        ("variable.function", Code::Function),
        ("variable", Code::Variable),
        ("punctuation", Code::Punctuation),
    ]
    .into_iter()
    .filter_map(|(name, code)| Some((Scope::new(name).ok()?, code)))
    .collect()
});

impl Code {
    /// The kind of a piece whose scopes are `stack`: the most specific
    /// scope with a family names it.
    fn of(stack: &[Scope]) -> Self {
        stack
            .iter()
            .rev()
            .find_map(|scope| {
                FAMILIES
                    .iter()
                    .find(|(family, _)| family.is_prefix_of(*scope))
                    .map(|(_, code)| *code)
            })
            .unwrap_or(Self::Other)
    }
}

/// One line's pieces: each one's bytes in the line and its kind.
pub(super) type Pieces = Vec<(Range<usize>, Code)>;

/// The lines highlighted so far, from the first, and the highlighter that
/// goes on from the line after them. A draw takes the shell by reference,
/// so what it finds is kept in a cell.
#[derive(Default)]
pub(super) struct Colours(RefCell<Found>);

#[derive(Default)]
struct Found {
    parser: Option<(ParseState, ScopeStack)>,
    lines: Vec<Pieces>,
}

impl Colours {
    /// Forget what was found: the file or its text changed.
    pub(super) fn forget(&mut self) {
        *self.0.get_mut() = Found::default();
    }

    /// Hand `draw` the pieces of `lines`, a file at `path`, parsed up to
    /// line `last` (from 0, not included) at least.
    pub(super) fn with<R>(
        &self,
        path: &Path,
        lines: &[String],
        last: usize,
        draw: impl FnOnce(&[Pieces]) -> R,
    ) -> R {
        let mut found = self.0.borrow_mut();
        let last = last.min(lines.len());
        let Found {
            parser,
            lines: done,
        } = &mut *found;
        if done.len() < last {
            let (state, stack) = parser.get_or_insert_with(|| {
                let syntax = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .and_then(|extension| SYNTAXES.find_syntax_by_extension(extension))
                    .unwrap_or_else(|| SYNTAXES.find_syntax_plain_text());
                (ParseState::new(syntax), ScopeStack::new())
            });
            for line in &lines[done.len()..last] {
                let operations = state.parse_line(line, &SYNTAXES).unwrap_or_default();
                let mut pieces = Pieces::new();
                let mut at = 0;
                // Each operation changes the scopes from its byte on: the
                // bytes before it take the scopes before it.
                let ends = operations
                    .into_iter()
                    .map(|(byte, operation)| (byte, Some(operation)))
                    .chain([(line.len(), None)]);
                for (byte, operation) in ends {
                    if byte > at {
                        pieces.push((at..byte, Code::of(&stack.scopes)));
                        at = byte;
                    }
                    if let Some(operation) = operation {
                        let _ = stack.apply(&operation);
                    }
                }
                done.push(pieces);
            }
        }
        draw(done)
    }
}

impl fmt::Debug for Colours {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines = self.0.try_borrow().map_or(0, |found| found.lines.len());
        formatter
            .debug_struct("Colours")
            .field("lines", &lines)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Code, Colours};

    #[test]
    fn lines_are_highlighted_once_up_to_the_last_drawn() {
        let lines: Vec<String> = ["fn main() {", "    let count = 1;", "}"]
            .map(str::to_owned)
            .to_vec();
        let mut colours = Colours::default();
        let path = Path::new("/p/src/main.rs");
        let first = colours.with(path, &lines, 1, <[_]>::len);
        assert_eq!(first, 1, "only the lines drawn are highlighted");
        let (bytes, code) = colours.with(path, &lines, 3, |pieces| pieces[0][0].clone());
        assert_eq!(&lines[0][bytes], "fn");
        assert_eq!(code, Code::Keyword);
        // A draw of fewer lines finds them all done.
        colours.with(path, &lines, 2, |pieces| assert_eq!(pieces.len(), 3));
        colours.forget();
        colours.with(path, &lines, 1, |pieces| assert_eq!(pieces.len(), 1));
    }
}
