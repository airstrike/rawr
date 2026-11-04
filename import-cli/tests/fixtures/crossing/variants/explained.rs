// rawr
// iced's ferris and layout: `explain`, a method of the blanket-implemented
// `Framed`, gives `impl Widget` from the trait's own default body. One
// result sits in `column!` as it is, as ferris's image does. One is boxed
// through its trait's own method. One is boxed through a method a blanket
// impl over that trait gives, as layout's `.explain(color).boxed()`.

use events::widgets::{button, column, rule, text, Framed, Widget};

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
        let explained = if self.value > 1 {
            rule(2, Message::Decrement).explain(0x0000ff_u32).boxed()
        } else {
            rule(2, Message::Decrement).boxed()
        };
        // `framed` is `Framed`'s, which the opaque type has through the
        // blanket impl over `Widget`, its bound, as iced's
        // `.explain(color).boxed()` is `iced_widget::Widget`'s.
        let framed = rule(3, Message::Increment).explain(0x00ff00_u32).framed();
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            rule(1, Message::Increment).explain(0xffffff_u32),
            explained,
            framed,
            button("Decrement").on_press(Message::Decrement),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
