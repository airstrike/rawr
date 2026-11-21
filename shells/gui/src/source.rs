//! The source around where the program is: the lines about one line of one
//! file, that line marked, and a gutter that sets and clears a breakpoint
//! on each.
//!
//! Each line is drawn in the colours a syntax highlighter gives it. The
//! highlighter carries its state from line to line, so it runs once over
//! the file's lines up to the last one shown, and keeps what it found: a
//! view reads those pieces and highlights nothing. New text for the file,
//! or a change between light and dark, starts it again.

use std::ops::{Range, RangeInclusive};
use std::path::Path;

use iced::highlighter::{self, Parser};
use iced::widget::text::Span;
use iced::widget::{button, column, container, rich_text, row, scrollable, span, text};
use iced::{Code, Color, Element, Fill, Font, Task, Widget};

use crate::action::Action;
use crate::theme;

/// Lines shown before a marked line.
const BEFORE: u64 = 6;
/// Lines shown after a marked line.
const AFTER: u64 = 24;

/// The file shown and its lines.
#[derive(Debug, Default)]
pub struct State {
    file: Option<String>,
    /// The file's text, and each line's bytes in it, without its newline.
    text: String,
    lines: Vec<Range<usize>>,
    /// Why the file couldn't be read.
    unread: Option<String>,
    /// The line marked: where the program stopped, or the call a caller's
    /// frame is at.
    marked: Option<u64>,
    /// The window draws light.
    light: bool,
    colours: Colours,
}

/// The highlighted lines of the file shown, from its first, and the
/// parser that goes on from the line after them.
#[derive(Debug, Default)]
struct Colours {
    parser: Option<Parser>,
    lines: Vec<Vec<Piece>>,
}

/// Part of a line drawn one way: its bytes in the line, and the colour and
/// font the theme gives the code it is, if any.
#[derive(Debug, Clone, PartialEq)]
struct Piece {
    bytes: Range<usize>,
    color: Option<Color>,
    font: Option<Font>,
}

#[derive(Clone, Debug)]
pub enum Message {
    Loaded {
        file: String,
        text: Result<String, String>,
    },
    Toggle(u64),
}

/// What the source asks of the debugger.
#[derive(Debug, PartialEq, Eq)]
pub enum Instruction {
    /// Set a breakpoint at the line, or clear the one there.
    Toggle { file: String, line: u64 },
}

impl State {
    /// Show `file` about `line`. The file is read again, as a save may
    /// have changed it; its lines stay shown until the new text comes.
    pub fn show(&mut self, file: &str, line: Option<u64>) -> Task<Message> {
        self.marked = line;
        if self.file.as_deref() != Some(file) {
            self.file = Some(file.to_owned());
            self.text.clear();
            self.lines.clear();
            self.unread = None;
            self.colours = Colours::default();
        }
        self.highlight();
        let file = file.to_owned();
        Task::perform(
            {
                let file = file.clone();
                async move { std::fs::read_to_string(&file).map_err(|error| error.to_string()) }
            },
            move |text| Message::Loaded {
                file: file.clone(),
                text,
            },
        )
    }

    /// The file shown.
    #[must_use]
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// Draw the lines light or dark, as the window is.
    pub fn follow(&mut self, light: bool) {
        if self.light != light {
            self.light = light;
            self.colours = Colours::default();
            self.highlight();
        }
    }

    pub fn update(&mut self, message: Message) -> Action<Instruction, Message> {
        match message {
            Message::Loaded { file, text } => {
                if self.file.as_deref() == Some(file.as_str()) {
                    match text {
                        Ok(text) => self.read(text),
                        Err(error) => self.unread = Some(error),
                    }
                }
                Action::none()
            }
            Message::Toggle(line) => match &self.file {
                Some(file) => Action::instruction(Instruction::Toggle {
                    file: file.clone(),
                    line,
                }),
                None => Action::none(),
            },
        }
    }

    /// Take the file's `text`: the same text keeps its colours.
    fn read(&mut self, text: String) {
        self.unread = None;
        if text == self.text {
            return;
        }
        self.lines = text
            .split_inclusive('\n')
            .scan(0, |start, line| {
                let at = *start;
                *start += line.len();
                let line = line.strip_suffix('\n').unwrap_or(line);
                Some(at..at + line.strip_suffix('\r').unwrap_or(line).len())
            })
            .collect();
        self.text = text;
        self.colours = Colours::default();
        self.highlight();
    }

    /// The lines shown, from 1: about the marked one, or the whole file.
    fn shown(&self) -> RangeInclusive<u64> {
        let total = self.lines.len() as u64;
        match self.marked {
            Some(around) => around.saturating_sub(BEFORE).max(1)..=(around + AFTER).min(total),
            None => 1..=total,
        }
    }

    /// Highlight the lines up to the last one shown, past those already
    /// highlighted: the parser names what code each piece is, and the
    /// window's theme colours it.
    fn highlight(&mut self) {
        let last = usize::try_from(*self.shown().end()).unwrap_or(usize::MAX);
        if self.colours.lines.len() >= last {
            return;
        }
        let token = self
            .file
            .as_deref()
            .and_then(|file| Path::new(file).extension())
            .and_then(|extension| extension.to_str())
            .unwrap_or("txt")
            .to_owned();
        let theme = theme::window(self.light);
        let parser = self
            .colours
            .parser
            .get_or_insert_with(|| Parser::new(&highlighter::Settings { token }));
        for bytes in &self.lines[self.colours.lines.len()..last] {
            let pieces = parser
                .parse_line(&self.text[bytes.clone()])
                .map(|(bytes, code): (Range<usize>, Code)| {
                    let style = theme::code(code, &theme);
                    Piece {
                        bytes,
                        color: style.color,
                        font: style.style.map(|style| Font {
                            style,
                            ..Font::MONOSPACE
                        }),
                    }
                })
                .collect();
            self.colours.lines.push(pieces);
        }
    }

    /// Line `index`, from 0, in its colours.
    fn line(&self, index: usize) -> Element<'_, Message> {
        let written = &self.text[self.lines[index].clone()];
        match self.colours.lines.get(index) {
            Some(pieces) if !pieces.is_empty() => {
                let spans: Vec<Span<'_, ()>> = pieces
                    .iter()
                    .map(|piece| {
                        span(&written[piece.bytes.clone()])
                            .color_maybe(piece.color)
                            .font_maybe(piece.font)
                    })
                    .collect();
                rich_text(spans).font(Font::MONOSPACE).size(12).boxed()
            }
            _ => text(written).font(Font::MONOSPACE).size(12).boxed(),
        }
    }

    /// The lines about the marked one, with a dot on each line in
    /// `breakpoints`.
    pub fn view(&self, breakpoints: &[u64]) -> Element<'_, Message> {
        let Some(file) = &self.file else {
            return container(
                text("No source yet: the program shows its file once it pauses.")
                    .size(theme::SIZE)
                    .style(text::secondary),
            )
            .padding(8)
            .boxed();
        };
        if let Some(error) = &self.unread {
            return text(format!("{file}: {error}"))
                .size(theme::SIZE)
                .style(text::danger)
                .boxed();
        }
        let rows = self.shown().map(|number| {
            let set = breakpoints.contains(&number);
            let gutter = button(
                row![
                    text(if set { "●" } else { " " })
                        .font(Font::MONOSPACE)
                        .size(theme::SIZE)
                        .style(theme::breakpoint),
                    text(format!("{number:>4}"))
                        .font(Font::MONOSPACE)
                        .size(theme::SIZE)
                        .style(theme::faint),
                ]
                .spacing(2),
            )
            .padding([0, 4])
            .style(button::text)
            .on_press(Message::Toggle(number));
            let code = self.line(usize::try_from(number - 1).unwrap_or_default());
            let line = container(row![gutter, code].spacing(6)).width(Fill);
            if self.marked == Some(number) {
                line.style(theme::marked).boxed()
            } else {
                line.boxed()
            }
        });
        scrollable(column(rows)).height(Fill).width(Fill).boxed()
    }
}

/// The last two parts of `file`'s path.
#[must_use]
pub fn short_path(file: &str) -> &str {
    let mut cuts = file.rmatch_indices('/').map(|(at, _)| at);
    cuts.nth(1).map_or(file, |at| &file[at + 1..])
}

#[cfg(test)]
mod tests {
    use super::{Message, State};

    const SOURCE: &str = "fn main() {\n    let count = 1;\n}\n";

    fn loaded(state: &mut State, text: &str) {
        let _ = state.update(Message::Loaded {
            file: "/p/src/main.rs".to_owned(),
            text: Ok(text.to_owned()),
        });
    }

    #[test]
    fn lines_are_highlighted_once_and_again_only_when_the_text_changes() {
        let mut state = State::default();
        let _ = state.show("/p/src/main.rs", Some(2));
        loaded(&mut state, SOURCE);
        assert_eq!(state.colours.lines.len(), 3);
        let keyword = &state.colours.lines[0][0];
        assert_eq!(&SOURCE[keyword.bytes.clone()], "fn");
        assert!(keyword.color.is_some(), "a keyword has a colour");

        // The same text, read again at a new stop, keeps what was found.
        let parser = format!("{:?}", state.colours.parser);
        let _ = state.show("/p/src/main.rs", Some(1));
        loaded(&mut state, SOURCE);
        assert_eq!(format!("{:?}", state.colours.parser), parser);

        let edited = "fn main() {\n    let count = 2;\n    let more = count;\n}\n";
        loaded(&mut state, edited);
        assert_eq!(state.colours.lines.len(), 4, "new text is highlighted anew");

        // Light and dark draw the line in different colours: its
        // punctuation takes the theme's text shades.
        let dark = state.colours.lines[0].clone();
        state.follow(true);
        assert_ne!(state.colours.lines[0], dark);
    }
}
