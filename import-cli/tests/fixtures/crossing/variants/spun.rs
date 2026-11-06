// rawr
// iced's ferris: `f32::from(self.rotation.degrees())` converts a method's
// output through the crate's `impl From<Turn> for f32`, as the field form
// does, and `.into()` of a method's output reaches the same impl.

use events::controls::{caption, slider, Spin, Turn};
use events::widgets::{column, Widget};

#[derive(Debug, Clone)]
enum Message {
    Turned(Turn),
}

struct Picture {
    spin: Spin,
}

impl Default for Picture {
    fn default() -> Self {
        Picture {
            spin: Spin::Floating(Turn(0.25)),
        }
    }
}

impl Picture {
    fn update(&mut self, message: Message) {
        match message {
            Message::Turned(turn) => {
                self.spin = Spin::Solid(turn);
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let half: f32 = Spin::Solid(Turn(self.spin.turn().0 / 2.0)).turn().into();
        column![
            slider(Turn::RANGE, self.spin.turn(), Message::Turned),
            caption(format!("Rotation: {:.0}", f32::from(self.spin.turn()))),
            caption(half),
        ]
    }
}

fn main() {
    events::controls::application(Picture::default, Picture::update, Picture::view).run()
}
