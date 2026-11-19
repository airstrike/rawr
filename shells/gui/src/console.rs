//! The eval console: Rust typed here is evaluated where the program is
//! paused, and each answer is shown under its code.

use iced::widget::{column, scrollable, text, text_input};
use iced::{Element, Fill, Font};

use crate::action::Action;

/// The text input's id.
pub const INPUT: &str = "eval";

/// The code typed so far and what was evaluated.
#[derive(Debug, Default)]
pub struct State {
    input: String,
    entries: Vec<Entry>,
}

/// One evaluation: its code, and its answer once it came.
#[derive(Debug)]
struct Entry {
    code: String,
    answer: Option<Result<String, String>>,
}

#[derive(Clone, Debug)]
pub enum Message {
    Input(String),
    Submit,
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
                self.entries.push(Entry {
                    code: code.clone(),
                    answer: None,
                });
                Action::instruction(Instruction::Eval {
                    entry: self.entries.len() - 1,
                    code,
                })
            }
        }
    }

    /// The answer to the entry numbered `entry`.
    pub fn answer(&mut self, entry: usize, answer: Result<String, String>) {
        if let Some(entry) = self.entries.get_mut(entry) {
            entry.answer = Some(answer);
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let entries = self.entries.iter().map(|entry| {
            let answer = match &entry.answer {
                None => text("…"),
                Some(Ok(value)) => text(value.as_str()).font(Font::MONOSPACE),
                Some(Err(error)) => text(error.as_str()).style(text::danger),
            };
            column![
                text(format!("> {}", entry.code)).font(Font::MONOSPACE),
                answer
            ]
            .into()
        });
        column![
            scrollable(column(entries).spacing(4))
                .anchor_bottom()
                .height(Fill)
                .width(Fill),
            text_input("Evaluate Rust where the program is paused", &self.input)
                .id(INPUT)
                .font(Font::MONOSPACE)
                .on_input(Message::Input)
                .on_submit(Message::Submit),
        ]
        .spacing(4)
        .into()
    }
}
