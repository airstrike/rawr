// rawr
// The crate's `lobby<Message>(floors: u8) -> impl
// Widget<Message>`, called at the tail of a method of the program's that
// takes a building, with the crate's own `Message` imported by a `use`:
// the method's `impl Widget<Message>` fixes it (#1804).

use events::elevated::{self, Building, Message};
use events::widgets::{column, plaque, pressing, text, Widget};

#[derive(Default)]
struct Board {
    calls: i64,
    building: Building,
}

impl Board {
    fn update(&mut self, message: Message) {
        match message {
            Message::Call => self.calls += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            self.lobby(&self.building),
            pressing(plaque("Increment"), Message::Call),
            text(self.calls),
        ]
    }

    fn lobby(&self, building: &Building) -> impl Widget<Message> {
        elevated::lobby(building.floors)
    }
}

fn main() {
    events::widgets::run(Board::update, Board::view);
}
