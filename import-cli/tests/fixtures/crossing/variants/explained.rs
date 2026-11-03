// rawr
// iced's ferris and layout: `explain`, a method of the blanket-implemented
// `Framed`, gives `impl Widget` from the trait's own default body. One
// result sits in `column!` as it is, and another is boxed, as layout's
// `.explain(color).boxed()`.

use events::widgets::{button, column, text, Framed, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let decrement = if self.value > 1 {
            button("Decrement")
                .on_press(Message::Decrement)
                .explain("blue")
                .boxed()
        } else {
            button("Decrement").on_press(Message::Decrement).boxed()
        };
        column![
            button("Increment")
                .on_press(Message::Increment)
                .explain("white"),
            text(self.value).size(50),
            decrement,
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
