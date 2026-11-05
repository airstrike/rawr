// rawr
// iced's vectorial_text: a closure's parameter left to inference takes a
// slider's range, so its `RangeInclusive<T>` takes `T` from the closure's
// `fn(f32) -> _`, and a call's `2.0..=80.0` is a range of `f32`, as rustc
// infers it.

use events::controls::{caption, slider};
use events::widgets::{column, Widget};

#[derive(Debug, Clone)]
enum Message {
    SizeChanged(f32),
    AngleChanged(f32),
}

struct Text {
    size: f32,
    angle: f32,
}

impl Default for Text {
    fn default() -> Self {
        Text {
            size: 40.0,
            angle: 0.0,
        }
    }
}

impl Text {
    fn update(&mut self, message: Message) {
        match message {
            Message::SizeChanged(size) => {
                self.size = size;
            }
            Message::AngleChanged(angle) => {
                self.angle = angle;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let slider_with_label = |label, range, value, message: fn(f32) -> _| {
            column![caption(label), slider(range, value, message)]
        };
        column![
            slider_with_label("Size", 2.0..=80.0, self.size, Message::SizeChanged),
            slider_with_label("Angle", 0.0..=360.0, self.angle, Message::AngleChanged),
        ]
    }
}

fn main() {
    events::controls::application(Text::default, Text::update, Text::view).run()
}
