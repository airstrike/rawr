// rawr
// iced's modal and toast helpers: a function whose parameter is `impl
// Widget<Message>`, called with a button and with text, and a function
// generic over its message that builds a crate widget. rawr compiles each
// once per instantiation its callers use (D629). The open renderer of each
// argument is fixed by `Widget`'s defaults and the crate's impls (D604).

use events::widgets::{button, column, text, Button, Widget};

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
            padded(labeled("Increment", Message::Increment), 5),
            padded(text(self.value).size(50), 7),
            labeled("Decrement", Message::Decrement),
        ]
    }
}

fn padded<'a>(content: impl Widget<Message> + 'a, padding: u16) -> impl Widget<Message> + 'a {
    column![content].padding(padding)
}

fn labeled<'a, M: Clone + 'a>(label: &'a str, message: M) -> Button<'a, M, &'a str> {
    button(label).on_press(message)
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
