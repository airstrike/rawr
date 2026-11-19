// From iced (https://github.com/iced-rs/iced), examples/pick_list/src/main.rs.
// Copyright 2019 Héctor Ramón, Iced contributors. MIT licence:
// https://github.com/iced-rs/iced/blob/master/LICENSE
// Changed: the tall spaces and the scrollable around them are dropped, so
// the list shows at the window's top.

use iced::widget::{column, pick_list};
use iced::{Center, Fill, Widget};

pub fn main() -> iced::Result {
    iced::run(Example::update, Example::view)
}

#[derive(Default)]
struct Example {
    selected_language: Option<Language>,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    LanguageSelected(Language),
}

impl Example {
    fn update(&mut self, message: Message) {
        match message {
            Message::LanguageSelected(language) => {
                self.selected_language = Some(language);
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let languages = pick_list(self.selected_language, Language::ALL, Language::to_string)
            .on_select(Message::LanguageSelected)
            .placeholder("Choose a language...");

        column!["Which is your favorite language?", languages]
            .width(Fill)
            .align_x(Center)
            .spacing(10)
            .padding(20)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    Rust,
    Elm,
    Ruby,
    Haskell,
    C,
    Javascript,
    Other,
}

impl Language {
    const ALL: &[Language] = &[
        Language::C,
        Language::Elm,
        Language::Ruby,
        Language::Haskell,
        Language::Rust,
        Language::Javascript,
        Language::Other,
    ];
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Language::Rust => "Rust",
                Language::Elm => "Elm",
                Language::Ruby => "Ruby",
                Language::Haskell => "Haskell",
                Language::C => "C",
                Language::Javascript => "Javascript",
                Language::Other => "Some other language",
            }
        )
    }
}
