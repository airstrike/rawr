// rawr
// What `explain` gives on an `Element<'_, ..>` that borrows a local
// counter, kept past the block the counter lives in: rustc reports E0597
// at the borrow.

use events::widgets::{button, column, rule, text, Element, Widget};
use events::Framed;

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

    fn example(&self) -> Element<'_, Message> {
        rule(4, Message::Decrement).boxed()
    }

    fn view(&self) -> impl Widget<Message> {
        let example = {
            let local = Counter::default();
            local.example().explain(0x0000ff_u32)
        };
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            example.boxed(),
            button("Decrement").on_press(Message::Decrement),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
