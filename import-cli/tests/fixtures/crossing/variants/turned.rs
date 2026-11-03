// rawr
// iced's gradient: `slider(Radians::RANGE, self.angle, ..)` reads an
// associated constant whose type is a std range of the crate's own type,
// and the slider takes it at that type.

use events::controls::{slider, Turn};
use events::widgets::{column, Widget};

#[derive(Debug, Clone)]
enum Message {
    Turned(Turn),
}

struct Dial {
    angle: Turn,
}

impl Default for Dial {
    fn default() -> Self {
        Dial { angle: Turn(0.25) }
    }
}

impl Dial {
    fn update(&mut self, message: Message) {
        match message {
            Message::Turned(angle) => {
                self.angle = angle;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![slider(Turn::RANGE, self.angle, Message::Turned)]
    }
}

fn main() {
    events::controls::application(Dial::default, Dial::update, Dial::view).run()
}
