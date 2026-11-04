// rawr
// iced's widgets give `impl Widget<Message, Theme, Renderer>`: nothing at
// `badge(..)` or `opaque(slider(..))` fixes its theme or renderer, nor at
// `badge(..)` its message. `.map(..)` fixes the message by the callable's
// input, and `column!`'s boxing of the `Map` fixes the theme and renderer
// through `Map`'s impl, whose where-clause asks the opaque type for
// `Widget`, a supertrait of the `Framed` it is written as. The view's
// `impl Widget<Message>` gives them the trait's defaults, and rustc carries
// them back to the call that made the value. The column holds opaque
// widgets alone, each boxed into the view's element.

use events::controls::{badge, slider};
use events::widgets::{column, opaque, Framed, Widget};

#[derive(Debug, Clone)]
enum Message {
    SliderChanged(u8),
    Pressed(u8),
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
            Message::SliderChanged(value) | Message::Pressed(value) => {
                self.value = value;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let held = badge(self.value);
        column![
            opaque(slider(1..=100, self.value, Message::SliderChanged)),
            badge(self.value).map(Message::Pressed),
            held.map(Message::Pressed),
            badge(self.value),
        ]
    }
}

fn main() {
    events::controls::application(Slider::default, Slider::update, Slider::view).run()
}
