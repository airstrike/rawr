// rawr
// The crate's `shaft<Message>(floors: u8) -> impl
// Framed<Message>`, an extension trait over `Widget` with defaulted
// parameters and a blanket impl, as iced's `Widget`: an enclosing `impl
// Framed<Message>` fixes `Message`, and so does an enclosing `impl
// Widget<Message>`, which only `Framed`'s supertrait names (#1804).

use events::elevated::{self, Building};
use events::widgets::{column, plaque, pressing, text, Framed, Widget};

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
            framed(&self.building),
            plain(&self.building),
            pressing(plaque("Increment"), Message::Increment),
            text(self.value),
        ]
    }
}

fn framed(building: &Building) -> impl Framed<Message> {
    elevated::shaft(building.floors)
}

fn plain(building: &Building) -> impl Widget<Message> {
    elevated::shaft(building.floors)
}

fn main() {
    events::widgets::run(Board::update, Board::view);
}
