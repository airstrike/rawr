// rawr
// iced's slider and pick_list: `opaque(slider(..))` gives an opaque widget
// whose theme and renderer, and `opaque(caption(..))` one whose message
// too, nothing at the call fixes. `column!` boxes them, and the view's
// `impl Widget<Message>` fixes them by the trait's defaults, as rustc
// carries a later use's types back to the call that made the value.

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
        column![
            opaque(slider(1..=100, self.value, Message::SliderChanged)),
            held,
        ]
    }
}

fn main() {
    events::controls::application(Slider::default, Slider::update, Slider::view).run()
}
