// rawr
// #916: a local holds what `opaque(caption(..))` gives, an opaque widget
// over `Caption<'a, Theme>`, a type that declares a lifetime, before
// `column!` boxes it. The opaque type has no Rust name, so the view's
// region holds the value as its runner names it.

use events::controls::{caption, slider};
use events::widgets::{column, opaque, Widget};

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
        let held = opaque(caption(self.value));
        column![slider(1..=100, self.value, Message::SliderChanged), held]
    }
}

fn main() {
    events::controls::application(Slider::default, Slider::update, Slider::view).run()
}
