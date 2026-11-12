// rawr
// elevato's game on iced's `ViewFn` shape: the crate's `run` takes
// `Program::view` as a higher-ranked `View`, whose blanket impl for every
// `Fn(&'a P, &'a Building) -> W` gives what the method gives, a crate
// function's opaque `impl Widget<Message>`, which the game then maps and
// boxes.

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
        viewed::floors(building, self.lit, Message::Light)
    }
}

fn main() {
    viewed::run(Program::update, Program::view);
}
