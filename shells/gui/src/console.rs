//! The eval console: Rust typed here is evaluated where the program is
//! paused, and each answer is shown under its code, as a compact row that
//! opens to its parts when it has any.

use std::collections::HashSet;

use iced::widget::{column, container, row, scrollable, text, text_input};
use iced::{Element, Fill, Font, Widget};

use crate::action::Action;
use crate::theme;
use crate::tree::{self, Part};

/// The text input's id.
pub const INPUT: &str = "eval";

/// The code typed so far, what was evaluated, and the answers open.
#[derive(Debug, Default)]
pub struct State {
    input: String,
    entries: Vec<Entry>,
    open: HashSet<String>,
}

/// One evaluation: its code, its place among the entries, and its answer
/// once it came.
#[derive(Debug)]
struct Entry {
    code: String,
    place: String,
    answer: Option<Result<Answer, String>>,
}

/// What an evaluation gave: the value's text and its parts.
#[derive(Debug)]
pub struct Answer {
    pub value: String,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug)]
pub enum Message {
    Input(String),
    Submit,
    /// Open or fold the answer's row at this place.
    Toggle(String),
}

/// What the console asks of the debugger.
#[derive(Debug, PartialEq, Eq)]
pub enum Instruction {
    /// Evaluate the code; its answer goes to the entry numbered `entry`.
    Eval { entry: usize, code: String },
}

impl State {
    pub fn update(&mut self, message: Message) -> Action<Instruction, Message> {
        match message {
            Message::Input(input) => {
                self.input = input;
                Action::none()
            }
            Message::Submit => {
                let code = std::mem::take(&mut self.input);
                if code.trim().is_empty() {
                    return Action::none();
                }
                let entry = self.entries.len();
                self.entries.push(Entry {
                    code: code.clone(),
                    place: entry.to_string(),
                    answer: None,
                });
                Action::instruction(Instruction::Eval { entry, code })
            }
            Message::Toggle(place) => {
                if !self.open.remove(&place) {
                    self.open.insert(place);
                }
                Action::none()
            }
        }
    }

    /// The answer to the entry numbered `entry`.
    pub fn answer(&mut self, entry: usize, answer: Result<Answer, String>) {
        if let Some(entry) = self.entries.get_mut(entry) {
            entry.answer = Some(answer);
        }
    }

    /// The entries as DevTools' console draws them, a hairline under each:
    /// the code after a `>`, its answer after a `<`, quiet, or why it has
    /// none in the colour of an error. The prompt is a bare line after the
    /// last: the accent's `>` and the code typed.
    pub fn view(&self) -> Element<'_, Message> {
        let entries = self.entries.iter().map(|entry| {
            let answer = match &entry.answer {
                None => quiet("…"),
                Some(Ok(answer)) if !answer.parts.is_empty() => tree::view(
                    [tree::Root {
                        place: &entry.place,
                        name: "",
                        value: Ok(&answer.value),
                        parts: &answer.parts,
                    }],
                    &self.open,
                    Message::Toggle,
                ),
                Some(Ok(answer)) => quiet(&answer.value),
                Some(Err(error)) => text(error.as_str())
                    .font(Font::MONOSPACE)
                    .size(theme::SIZE)
                    .style(theme::error)
                    .boxed(),
            };
            let code = text(entry.code.as_str())
                .font(Font::MONOSPACE)
                .size(theme::SIZE);
            column![
                container(column![marked(">", code.boxed()), marked("<", answer)]).padding([1, 8]),
                theme::hairline(),
            ]
            .boxed()
        });
        let prompt = row![
            text(">")
                .font(Font::MONOSPACE)
                .size(theme::SIZE)
                .style(theme::mark),
            text_input("", &self.input)
                .id(INPUT)
                .font(Font::MONOSPACE)
                .size(theme::SIZE)
                .padding(0)
                .style(theme::bare)
                .on_input(Message::Input)
                .on_submit(Message::Submit),
        ]
        .spacing(MARK)
        .padding([2, 8])
        .align_y(iced::Center);
        scrollable(column(entries).push(prompt.boxed()))
            .direction(theme::scrollbar())
            .anchor_bottom()
            .height(Fill)
            .width(Fill)
            .boxed()
    }
}

/// The room between a mark and what it marks.
const MARK: f32 = 8.0;

/// `content` after a quiet `mark`.
fn marked<'a>(mark: &'a str, content: Element<'a, Message>) -> Element<'a, Message> {
    row![quiet(mark), content].spacing(MARK).boxed()
}

/// Monospace text in the quiet colour.
fn quiet(content: &str) -> Element<'_, Message> {
    text(content)
        .font(Font::MONOSPACE)
        .size(theme::SIZE)
        .style(theme::faint)
        .boxed()
}
