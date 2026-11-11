// rawr
// iced and iced_test together (#1660): `tester` re-exports `parts`, as
// iced_test re-exports iced_core, so `explain` on `parts`'s `Map` gives an
// opaque type both `tester` and `events`'s `Framed` reach, under names of
// their own. It is one type.

use events::widgets::{button, column, rule, text, Framed, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone)]
enum Inner {
    Pressed,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Inner(Inner),
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Inner(Inner::Pressed) => self.value -= 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            rule(4, Inner::Pressed)
                .map(Message::Inner)
                .explain(0x0000ff_u32),
        ]
    }
}

fn main() {
    let mapped = rule(4, Inner::Pressed).map(Message::Inner);
    println!("tested {}", tester::described(&mapped));
    events::widgets::run(Counter::update, Counter::view);
}
