// rawr
// The crate's `floors<'a, Message: 'a>(floors: u8) -> impl
// Widget<Message> + 'a`, called from a free function that takes a
// building: the enclosing `impl Widget<Message>` fixes `Message` (#1804).

use events::elevated::{self, Building};
use events::widgets::{column, plaque, pressing, text, Widget};

#[derive(Default)]
struct Board {
    value: i64,
    building: Building,
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
            floors_of(&self.building),
            pressing(plaque("Increment"), Message::Increment),
            text(self.value),
        ]
    }
}

fn floors_of(building: &Building) -> impl Widget<Message> {
    elevated::floors(building.floors)
}

fn main() {
    events::widgets::run(Board::update, Board::view);
}
