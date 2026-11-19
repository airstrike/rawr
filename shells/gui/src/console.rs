//! The eval console: Rust typed here is evaluated where the program is
//! paused, and each answer is shown under its code, as a compact row that
//! opens to its parts when it has any.

use std::collections::HashSet;

use iced::widget::{column, row, scrollable, text, text_input};
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

    pub fn view(&self) -> Element<'_, Message> {
        let entries = self.entries.iter().map(|entry| {
            let answer = match &entry.answer {
                None => text("…").size(theme::SIZE).boxed(),
                Some(Ok(answer)) => tree::view(
                    [tree::Root {
                        place: &entry.place,
                        name: "",
                        value: Ok(&answer.value),
                        parts: &answer.parts,
                    }],
                    &self.open,
                    Message::Toggle,
                ),
                Some(Err(error)) => text(error.as_str())
                    .size(theme::SIZE)
                    .style(theme::error)
                    .boxed(),
            };
            column![
                text(format!("> {}", entry.code))
                    .font(Font::MONOSPACE)
                    .size(theme::SIZE)
                    .style(text::secondary),
                answer
            ]
            .boxed()
        });
        column![
            scrollable(column(entries).spacing(2).padding([2, 6]))
                .anchor_bottom()
                .height(Fill)
                .width(Fill),
            row![
                text(">").font(Font::MONOSPACE).size(theme::SIZE),
                text_input("Evaluate Rust where the program is paused", &self.input)
                    .id(INPUT)
                    .font(Font::MONOSPACE)
                    .size(theme::SIZE)
                    .padding([3, 4])
                    .on_input(Message::Input)
                    .on_submit(Message::Submit),
            ]
            .spacing(4)
            .padding([0, 6])
            .align_y(iced::Center),
        ]
        .spacing(2)
        .boxed()
    }
}
