// rawr
// iced's exit, toast, game_of_life and modal in the fixture: `padding([10,
// 20])` takes the pair as the `[u16; 2]` of `Padding`'s `From`, `padding(4)`
// the `u16` and `padding([1.5, 2.5])` the `[f32; 2]`; `slider(1.0..=30.0,
// ..)` takes a range of floats as the `RangeInclusive<f64>` its value's
// `f64` makes it, and `slider(0.0..=1.0, ..)` one of `f32`; and a style's
// `background: Some(Color { .. }.into())` converts the crate's `Color` into
// its `Background` through `From`.

use events::controls::slider;
use events::conversions::{frame, Style};
use events::widgets::{column, Widget};
use events::Color;

#[derive(Debug, Clone, Copy)]
enum Message {
    Timeout(f64),
    Level(f32),
    Confirm,
}

struct Conversions {
    timeout: f64,
    level: f32,
    confirmed: bool,
}

impl Default for Conversions {
    fn default() -> Self {
        Conversions {
            timeout: 5.0,
            level: 0.5,
            confirmed: false,
        }
    }
}

impl Conversions {
    fn update(&mut self, message: Message) {
        match message {
            Message::Timeout(timeout) => self.timeout = timeout,
            Message::Level(level) => self.level = level,
            Message::Confirm => self.confirmed = true,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let label = if self.confirmed { "done" } else { "pair" };
        column![
            frame(label).padding([10, 20]).on_press(Message::Confirm),
            frame("one").padding(4),
            frame("floats").padding([1.5, 2.5]),
            slider(1.0..=30.0, self.timeout, Message::Timeout),
            slider(0.0..=1.0, self.level, move |level| Message::Level(level)),
            frame("dim").style(|_theme| Style {
                background: Some(
                    Color {
                        r: 9,
                        ..Color::BLACK
                    }
                    .into(),
                ),
                ..Style::default()
            }),
        ]
    }
}

fn main() {
    events::controls::application(Conversions::default, Conversions::update, Conversions::view)
        .run()
}
