// rawr
// elevato's game on iced's `ViewFn` shape, as `viewed.rs`, where the
// crate function's opaque `impl Widget<Message>` borrows nothing of the
// building native code lends the view.

use events::viewed::{self, Building, Widget};

#[derive(Default)]
struct Program {
    lit: u8,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Light,
}

impl Program {
    fn update(&mut self, message: Message) {
        match message {
            Message::Light => self.lit += 1,
        }
    }

    fn view(&self, building: &Building) -> impl Widget<Message> {
        viewed::tower(building.floors, self.lit, Message::Light)
    }
}

fn main() {
    viewed::run(Program::update, Program::view);
}
