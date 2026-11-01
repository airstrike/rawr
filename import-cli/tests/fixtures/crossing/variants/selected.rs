// rawr
// iced's solar_system and qr_code: `pick(..)` takes `Some(&self.backend)`,
// a reference to a crate enum the state holds, as its `V: Borrow<T>`, as
// it takes `self.chosen.as_ref()` and `Some(&self.language)`, a reference
// to a script enum.

use events::controls::pick;
use events::widgets::{column, Widget};
use events::Backend;

struct Example {
    backend: Backend,
    chosen: Option<Backend>,
    language: Language,
}

impl Default for Example {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            chosen: Some(Backend::Custom("Mine".to_owned())),
            language: Language::default(),
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    BackendSelected(Backend),
    Chosen(Backend),
    LanguageSelected(Language),
}

impl Example {
    fn update(&mut self, message: Message) {
        match message {
            Message::BackendSelected(backend) => {
                self.backend = backend;
            }
            Message::Chosen(backend) => {
                self.chosen = Some(backend);
            }
            Message::LanguageSelected(language) => {
                self.language = language;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let backend = pick(Some(&self.backend), Backend::ALL, Backend::to_string)
            .on_select(Message::BackendSelected);
        let chosen = pick(self.chosen.as_ref(), Backend::ALL, Backend::to_string)
            .on_select(Message::Chosen)
            .placeholder("None yet");
        let language = pick(Some(&self.language), Language::ALL, Language::to_string)
            .on_select(Message::LanguageSelected);

        column![backend, chosen, language]
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
enum Language {
    #[default]
    Rust,
    Elm,
}

impl Language {
    const ALL: &[Language] = &[Language::Rust, Language::Elm];
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
