// rawr
// iced's slider in the fixture's controls: `logo(..)` gives an opaque
// `impl Widget` that `column!` boxes through the trait's own method, a
// slider takes a `RangeInclusive<u8>` and `default(impl Into<u8>)` takes a
// number literal as the `u8` it is. `caption(..)` names the function where
// a macro has the same name, as iced's `text(self.value)` does, and gives a
// type whose only name is an alias that passes its parameters on.

use events::controls::{caption, logo, slider};
use events::widgets::{column, Widget};

#[derive(Debug, Clone)]
enum Message {
    SliderChanged(u8),
}

struct Slider {
    value: u8,
}

impl Default for Slider {
    fn default() -> Self {
        Slider { value: 50 }
    }
}

impl Slider {
    fn update(&mut self, message: Message) {
        match message {
            Message::SliderChanged(value) => {
                self.value = value;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let h_slider = slider(1..=100, self.value, Message::SliderChanged).default(50);
        let caption = caption(self.value);
        column![h_slider, caption, logo(self.value as f32)]
    }
}

fn main() {
    events::controls::application(Slider::default, Slider::update, Slider::view).run()
}
