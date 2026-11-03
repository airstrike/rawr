// rawr
// iced's combo_box: `choose(&self.languages, .., self.selected.as_ref(),
// ..)` borrows the state for the view's lifetime and its selection, an
// `Option<&T>` at a late-bound lifetime, for the call alone.

use events::controls::{choose, choose::State};
use events::widgets::{column, Widget};

struct Example {
    languages: State<Language>,
    selected: Option<Language>,
}

impl Default for Example {
    fn default() -> Self {
        Self {
            languages: State::new(Language::ALL.to_vec()),
            selected: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Selected(Language),
}

impl Example {
    fn update(&mut self, message: Message) {
        match message {
            Message::Selected(language) => self.selected = Some(language),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let chosen = choose(
            &self.languages,
            "Type a language...",
            self.selected.as_ref(),
            Message::Selected,
        );

        column![chosen]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Language {
    Rust,
    Elm,
}

impl Language {
    const ALL: [Language; 2] = [Language::Rust, Language::Elm];
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Language::Rust => "Rust",
            Language::Elm => "Elm",
        })
    }
}

fn main() {
    events::controls::application(Example::default, Example::update, Example::view).run()
}
