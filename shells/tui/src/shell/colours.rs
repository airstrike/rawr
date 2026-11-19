//! The colours of the shown file's lines, as a syntax highlighter gives
//! them. The highlighter carries its state from line to line, so it runs
//! once over the lines up to the last one drawn and keeps what it found: a
//! draw highlights only the lines no draw reached before, and new text
//! starts it again.

use std::cell::RefCell;
use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::sync::LazyLock;

use ratatui::style::Color;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);

/// A dark theme: terminals draw on dark backgrounds far more often than
/// light ones.
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    ThemeSet::load_defaults()
        .themes
        .remove("base16-ocean.dark")
        .unwrap_or_default()
});

/// One line's pieces: each one's bytes in the line and its colour.
pub(super) type Pieces = Vec<(Range<usize>, Color)>;

/// The lines highlighted so far, from the first, and the highlighter that
/// goes on from the line after them. A draw takes the shell by reference,
/// so what it finds is kept in a cell.
#[derive(Default)]
pub(super) struct Colours(RefCell<Found>);

#[derive(Default)]
struct Found {
    highlighter: Option<HighlightLines<'static>>,
    lines: Vec<Pieces>,
}

impl Colours {
    /// Forget what was found: the file or its text changed.
    pub(super) fn forget(&mut self) {
        *self.0.get_mut() = Found::default();
    }

    /// Hand `draw` the pieces of `lines`, a file at `path`, highlighted up
    /// to line `last` (from 0, not included) at least.
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
            highlighter,
            lines: done,
        } = &mut *found;
        if done.len() < last {
            let highlighter = highlighter.get_or_insert_with(|| {
                let syntax = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .and_then(|extension| SYNTAXES.find_syntax_by_extension(extension))
                    .unwrap_or_else(|| SYNTAXES.find_syntax_plain_text());
                HighlightLines::new(syntax, &THEME)
            });
            for line in &lines[done.len()..last] {
                let mut at = 0;
                let pieces = highlighter
                    .highlight_line(line, &SYNTAXES)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(style, piece)| {
                        let bytes = at..at + piece.len();
                        at = bytes.end;
                        let colour = style.foreground;
                        (bytes, Color::Rgb(colour.r, colour.g, colour.b))
                    })
                    .collect();
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

    use super::Colours;

    #[test]
    fn lines_are_highlighted_once_up_to_the_last_drawn() {
        let lines: Vec<String> = ["fn main() {", "    let count = 1;", "}"]
            .map(str::to_owned)
            .to_vec();
        let mut colours = Colours::default();
        let path = Path::new("/p/src/main.rs");
        let first = colours.with(path, &lines, 1, <[_]>::len);
        assert_eq!(first, 1, "only the lines drawn are highlighted");
        let keyword = colours.with(path, &lines, 3, |pieces| pieces[0][0].clone());
        assert_eq!(&lines[0][keyword.0], "fn");
        // A draw of fewer lines finds them all done.
        colours.with(path, &lines, 2, |pieces| assert_eq!(pieces.len(), 3));
        colours.forget();
        colours.with(path, &lines, 1, |pieces| assert_eq!(pieces.len(), 1));
    }
}
