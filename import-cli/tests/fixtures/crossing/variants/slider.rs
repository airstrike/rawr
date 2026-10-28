// rawr
// iced's slider in the fixture's controls: `logo(..)` gives an opaque
// `impl Widget` that `column!` boxes through the trait's own method, a
// slider takes a `RangeInclusive<u8>` and `default(impl Into<u8>)` takes a
// number literal as the `u8` it is.

use events::controls::{logo, slider};
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
        column![h_slider, logo(self.value as f32)]
    }
}

fn main() {
    events::controls::application(Slider::default, Slider::update, Slider::view).run()
}
