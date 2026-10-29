// rawr
// Generic helpers as methods, and a generic whose parameter reaches a crate
// type only in its body. rawr compiles each once per instantiation its
// callers use (D629), as it does a generic free function whose signature
// names a crate type.

use events::widgets::{button, column, text, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.value += 1;
            }
            Message::Decrement => {
                self.value -= 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            self.padded(button("Increment").on_press(Message::Increment), 5),
            self.padded(text(self.value).size(50), 7),
            self.padded(Self::labeled("Decrement", Message::Decrement), 1),
            text(painted(events::Color::rgb(1, 2, self.value as u8))),
            text(painted(Shade::Dark)),
        ]
    }

    fn padded<'a>(
        &self,
        content: impl Widget<Message> + 'a,
        padding: u16,
    ) -> impl Widget<Message> + 'a {
        column![content].padding(padding + self.value as u16)
    }

    fn labeled<'a, M: Clone + 'a>(label: &'a str, message: M) -> impl Widget<M> + 'a {
        button(label).on_press(message)
    }
}

#[derive(Debug, Clone, Copy)]
enum Shade {
    Dark,
}

impl From<Shade> for events::Color {
    fn from(shade: Shade) -> Self {
        match shade {
            Shade::Dark => events::Color::rgb(10, 20, 30),
        }
    }
}

/// A generic whose parameter reaches a crate function only in its body.
fn painted<C: Into<events::Color>>(color: C) -> String {
    format!("painted {}", events::paint(color))
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
