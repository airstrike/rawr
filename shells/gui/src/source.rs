//! The source around where the program is: the lines about one line of one
//! file, that line marked, and a gutter that sets and clears a breakpoint
//! on each.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Fill, Font, Task, Theme};

use crate::action::Action;

/// Lines shown before a marked line.
const BEFORE: u64 = 6;
/// Lines shown after a marked line.
const AFTER: u64 = 24;

/// The file shown and its lines.
#[derive(Debug, Default)]
pub struct State {
    file: Option<String>,
    lines: Vec<String>,
    /// Why the file couldn't be read.
    unread: Option<String>,
    /// The line marked: where the program stopped, or the call a caller's
    /// frame is at.
    marked: Option<u64>,
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
    /// Show `file` about `line`, reading it when it isn't the file shown.
    pub fn show(&mut self, file: &str, line: Option<u64>) -> Task<Message> {
        self.marked = line;
        if self.file.as_deref() == Some(file) {
            return Task::none();
        }
        self.file = Some(file.to_owned());
        self.lines = Vec::new();
        self.unread = None;
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

    pub fn update(&mut self, message: Message) -> Action<Instruction, Message> {
        match message {
            Message::Loaded { file, text } => {
                if self.file.as_deref() == Some(file.as_str()) {
                    match text {
                        Ok(text) => self.lines = text.lines().map(str::to_owned).collect(),
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
        let lines = &self.lines;
        let total = lines.len() as u64;
        // With no line marked, the whole file.
        let (first, last) = match self.marked {
            Some(around) => (
                around.saturating_sub(BEFORE).max(1),
                (around + AFTER).min(total),
            ),
            None => (1, total),
        };
        let rows = (first..=last).map(|number| {
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
            let code = text(lines[usize::try_from(number - 1).unwrap_or_default()].as_str())
                .font(Font::MONOSPACE);
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
