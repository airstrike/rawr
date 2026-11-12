// rawr
// elevato's `fn view(&self, ..) -> impl Widget<Message> { view(..) }`:
// the crate's `plaque<'a, Message: 'a>(..) -> impl Widget<Message> + 'a`
// names `Message` only in its output, which the enclosing function's
// `impl Widget<Message>`, or the `impl Widget<Message>` input of the crate
// function it is passed to, fixes, as rustc infers it from the expected
// type.

use events::widgets::{column, plaque, pressing, text, Widget};

#[derive(Default)]
struct Board {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Board {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            self.heading(),
            footer(),
            pressing(plaque("Increment"), Message::Increment),
            text(self.value),
        ]
    }

    fn heading(&self) -> impl Widget<Message> {
        events::widgets::plaque("Board")
    }
}

fn footer() -> impl Widget<Message> {
    plaque("Footer")
}

fn main() {
    events::widgets::run(Board::update, Board::view);
}
