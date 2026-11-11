// rawr
// iced's pokedex in the fixture's widgets: `column!` boxes a `String`'s
// `as_ref()`, whose target only the crate's `impl Widget for &str` fixes,
// as rustc selects the one impl whose self type is a shared reference.

use events::widgets::{button, column, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
    description: String,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.value += 1;
                self.description = format!("pressed {}", self.value);
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            self.description.as_ref(),
            button("Increment").on_press(Message::Increment)
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
