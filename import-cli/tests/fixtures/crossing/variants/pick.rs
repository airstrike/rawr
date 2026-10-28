// rawr
// iced's pick_list in the fixture's controls: `column!` boxes a string
// literal through the crate's `impl Widget for &str`, and `pick(..)` takes
// an `Option` of a script enum, a script `&[Language]` constant as its
// `L: Borrow<[T]>`, and the enum's `Display` as `to_string`; the enum's
// derived `PartialEq` marks the selected option.

use events::controls::pick;
use events::widgets::{column, Widget};

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
        let languages = pick(self.selected_language, Language::ALL, Language::to_string)
            .on_select(Message::LanguageSelected)
            .placeholder("Choose a language...");

        column!["Which is your favorite language?", languages]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    Rust,
    Elm,
    Ruby,
    Other,
}

impl Language {
    const ALL: &[Language] = &[
        Language::Elm,
        Language::Ruby,
        Language::Rust,
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
                Language::Other => "Some other language",
            }
        )
    }
}

fn main() {
    events::controls::application(Example::default, Example::update, Example::view).run()
}
