// rawr
// `outlined`, a default method of a trait the crate re-exports at its root,
// called on an `Element<'_, ..>` a function of the program gives: the type
// its body gives has no public path, so Rust code can't write the opaque
// type a value borrowing the element is held as.
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
            self.example().outlined().boxed()
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
