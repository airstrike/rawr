// rawr
// iced's game_of_life, arc and delineate: `.theme(Theme::Dark)` takes the
// theme itself where `Application::theme` takes `impl ThemeFn<State,
// Theme>`, which the crate implements for its `Theme` beside every
// `Fn(&State)`. The value fills the parameter as rustc fills it, by the
// crate's impl for its type.

use events::controls::{toggle, Theme};
use events::widgets::{column, Widget};

#[derive(Debug, Default)]
struct Themed {
    enabled: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Toggled(bool),
}

impl Themed {
    fn update(&mut self, message: Message) {
        match message {
            Message::Toggled(enabled) => self.enabled = enabled,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![toggle(self.enabled)
            .label("Dark")
            .on_toggle(Message::Toggled)]
    }
}

fn main() {
    events::controls::application(Themed::default, Themed::update, Themed::view)
        .theme(Theme::Dark)
        .run()
}
