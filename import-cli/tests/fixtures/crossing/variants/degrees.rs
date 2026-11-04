// rawr
// iced's ferris: `f32::from(self.rotation.degrees())` calls the crate's
// `impl From<Turn> for f32`, an impl of a std trait for a std type, and
// `.into()` reaches the same impl when an `f32` is expected.

use events::controls::{caption, slider, Turn};
use events::widgets::{column, Widget};

#[derive(Debug, Clone)]
enum Message {
    Turned(Turn),
}

struct Dial {
    angle: Turn,
    degrees: f32,
}

impl Default for Dial {
    fn default() -> Self {
        Dial {
            angle: Turn(0.25),
            degrees: 0.0,
        }
    }
}

impl Dial {
    fn update(&mut self, message: Message) {
        match message {
            Message::Turned(angle) => {
                self.angle = angle;
                self.degrees = angle.into();
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let degrees = f32::from(self.angle);
        let half: f32 = Turn(self.angle.0 / 2.0).into();
        column![
            slider(Turn::RANGE, self.angle, Message::Turned),
            caption(degrees),
            caption(half),
            caption(self.degrees),
        ]
    }
}

fn main() {
    events::controls::application(Dial::default, Dial::update, Dial::view).run()
}
