// rawr
// iced's layout: `self.example.view().explain(color).boxed()` calls
// `explain`, a default method of a trait the crate re-exports at its root,
// on an `Element<'_, ..>` a function of the program gives. The opaque type
// it gives borrows what the element borrows.

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
        let example = if self.value > 1 {
            self.example().explain(0x0000ff_u32).boxed()
        } else {
            self.example()
        };
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            example,
            button("Decrement").on_press(Message::Decrement),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
