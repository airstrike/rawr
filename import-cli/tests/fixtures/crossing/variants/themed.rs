// rawr
// iced's styling and todos: `.theme(Themed::theme)` takes a function of
// the state where `Application::theme` takes `impl ThemeFn<State, Theme>`,
// through the crate's impl for every `Fn(&State)` whose output turns into
// an `Option<Theme>`. The function gives the theme itself, which turns
// into `Some` of it.

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

    fn theme(&self) -> Theme {
        if self.enabled {
            Theme::Dark
        } else {
            Theme::Light
        }
    }
}

fn main() {
    events::controls::application(Themed::default, Themed::update, Themed::view)
        .theme(Themed::theme)
        .run()
}
