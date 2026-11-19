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

use iced::highlighter::{self, Highlighter};
use iced::widget::text::{Highlighter as _, Span};
use iced::widget::{button, column, container, rich_text, row, scrollable, span, text};
use iced::{Color, Element, Fill, Font, Task, Theme};

use crate::action::Action;

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
/// highlighter that goes on from the line after them.
#[derive(Debug, Default)]
struct Colours {
    highlighter: Option<Highlighter>,
    lines: Vec<Vec<Piece>>,
}

/// Part of a line drawn one way: its bytes in the line, and the colour and
/// font the highlighter gave it, if any.
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
    /// highlighted.
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
        let theme = if self.light {
            highlighter::Theme::InspiredGitHub
        } else {
            highlighter::Theme::Base16Ocean
        };
        let highlighter = self
            .colours
            .highlighter
            .get_or_insert_with(|| Highlighter::new(&highlighter::Settings { theme, token }));
        for bytes in &self.lines[self.colours.lines.len()..last] {
            let pieces = highlighter
                .highlight_line(&self.text[bytes.clone()])
                .map(|(bytes, highlight)| Piece {
                    bytes,
                    color: highlight.color(),
                    font: highlight.font(),
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
                let spans: Vec<Span<'_, (), Font>> = pieces
                    .iter()
                    .map(|piece| {
                        span(&written[piece.bytes.clone()])
                            .color_maybe(piece.color)
                            .font_maybe(piece.font)
                    })
                    .collect();
                rich_text(spans).font(Font::MONOSPACE).into()
            }
            _ => text(written).font(Font::MONOSPACE).into(),
        }
    }

    /// The lines about the marked one, with a dot on each line in
    /// `breakpoints`.
    pub fn view(&self, breakpoints: &[u64]) -> Element<'_, Message> {
        let Some(file) = &self.file else {
            return container(text(
                "No source yet: the program shows its file once it pauses.",
            ))
            .padding(8)
            .into();
        };
        if let Some(error) = &self.unread {
            return text(format!("{file}: {error}")).into();
        }
        let rows = self.shown().map(|number| {
            let marked = self.marked == Some(number);
            let set = breakpoints.contains(&number);
            let gutter = button(
                row![
                    text(if set { "●" } else { " " })
                        .font(Font::MONOSPACE)
                        .style(text::danger),
                    text(format!("{number:>4}")).font(Font::MONOSPACE),
                ]
                .spacing(4),
            )
            .padding([0, 4])
            .style(button::text)
            .on_press(Message::Toggle(number));
            let code = self.line(usize::try_from(number - 1).unwrap_or_default());
            let line = container(row![gutter, code].spacing(8)).width(Fill);
            if marked {
                line.style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style::default().background(palette.primary.weak.color)
                })
                .into()
            } else {
                line.into()
            }
        });
        column![
            text(file.as_str()).size(13),
            scrollable(column(rows)).height(Fill)
        ]
        .spacing(4)
        .into()
    }
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
        let highlighter = format!("{:?}", state.colours.highlighter);
        let _ = state.show("/p/src/main.rs", Some(1));
        loaded(&mut state, SOURCE);
        assert_eq!(format!("{:?}", state.colours.highlighter), highlighter);

        let edited = "fn main() {\n    let count = 2;\n    let more = count;\n}\n";
        loaded(&mut state, edited);
        assert_eq!(state.colours.lines.len(), 4, "new text is highlighted anew");

        // Light and dark draw a keyword in different colours.
        let dark = state.colours.lines[0][0].color;
        state.follow(true);
        assert_ne!(state.colours.lines[0][0].color, dark);
    }
}
