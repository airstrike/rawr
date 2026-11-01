// rawr
// iced's counter with tiles, whose `Widget` impl holds when their content's
// does, as iced's `Button<'a, Message, W, Theme>`: a string literal's is the
// impl for `&str`, which no facade header writes.

use events::widgets::{column, text, tile, Widget};

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
            tile("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            tile("Decrement").on_press(Message::Decrement)
        ]
        .padding(20)
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
