// rawr
// elevato's program, beside the crate whose trait the game's view gives:
// `game::view<'a, Message: 'a>(floors: u8) -> impl Framed<Message> + 'a`
// is a crate's function whose output is another crate's extension trait,
// and the method `view(&self, building: &Building) -> impl
// Framed<Message>`, written with `events`'s trait and `game`'s `Message`,
// fixes it (#1804).

use events::widgets::Framed;
use game::{Building, Message};

#[derive(Default)]
struct Program {
    calls: i64,
    building: Building,
}

impl Program {
    fn update(&mut self, message: Message) {
        match message {
            Message::Call => self.calls += 1,
        }
    }

    fn screen(&self) -> impl Framed<Message> {
        self.view(&self.building)
    }

    pub fn view(&self, building: &Building) -> impl Framed<Message> {
        game::view(building.floors)
    }
}

fn main() {
    events::widgets::run(Program::update, Program::screen);
}
