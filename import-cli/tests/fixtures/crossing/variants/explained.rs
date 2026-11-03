// rawr
// iced's ferris and layout: `explain`, a method of the blanket-implemented
// `Framed`, gives `impl Widget` from the trait's own default body. One
// result sits in `column!` as it is, as ferris's image does, and another
// is boxed, as layout's `.explain(color).boxed()`.

use events::controls::slider;
use events::widgets::{button, column, text, Framed, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
    Slid(u8),
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
            Message::Slid(_) => {}
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let explained = if self.value > 1 {
            slider(1..=100, 50, Message::Slid).explain(0x0000ff_u32).boxed()
        } else {
            slider(1..=100, 50, Message::Slid).boxed()
        };
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            slider(1..=10, 5, Message::Slid).explain(0xffffff_u32),
            explained,
            button("Decrement").on_press(Message::Decrement),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
