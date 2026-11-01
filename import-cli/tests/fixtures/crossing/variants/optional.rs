// rawr
// iced's qr_code, solar_system and screenshot: `column!` boxes an
// `Option` of a widget through the crate's impl of its trait for
// `Option<T>`, as it boxes the crate's own widgets. The optional slider
// and caption appear once the value passes 60.

use events::controls::{caption, slider};
use events::widgets::{column, Widget};

#[derive(Debug, Clone)]
enum Message {
    SliderChanged(u8),
}

struct Optional {
    value: u8,
    shown: Option<u8>,
}

impl Default for Optional {
    fn default() -> Self {
        Optional {
            value: 50,
            shown: None,
        }
    }
}

impl Optional {
    fn update(&mut self, message: Message) {
        match message {
            Message::SliderChanged(value) => {
                self.value = value;
                self.shown = (value > 60).then_some(value);
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let h_slider = slider(1..=100, self.value, Message::SliderChanged);
        let maybe_caption = self.shown.map(|value| caption(value));
        column![
            h_slider,
            maybe_caption,
            self.shown
                .map(|value| slider(1..=100, value, Message::SliderChanged)),
        ]
    }
}

fn main() {
    events::controls::application(Optional::default, Optional::update, Optional::view).run()
}
