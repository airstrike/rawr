// rawr
// iced's websocket and screenshot: `caption`, a generic function whose
// name a macro and a module share, is passed as a value to
// `Iterator::map` and to `Option::map`. The closure's input fixes its
// `impl ToString`, and the view's widget fixes its theme.

use events::controls::{caption, slider};
use events::widgets::{column, Column, Widget};

#[derive(Debug, Clone)]
enum Message {
    SliderChanged(u8),
}

struct Valued {
    names: Vec<String>,
    shown: Option<u8>,
}

impl Default for Valued {
    fn default() -> Self {
        Valued {
            names: vec!["first".to_string(), "second".to_string()],
            shown: None,
        }
    }
}

impl Valued {
    fn update(&mut self, message: Message) {
        match message {
            Message::SliderChanged(value) => {
                self.names.push(format!("at {value}"));
                self.shown = Some(value);
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            slider(1..=100, 50, Message::SliderChanged),
            Column::with_children(self.names.iter().map(caption)),
            self.shown.map(caption),
        ]
    }
}

fn main() {
    events::controls::application(Valued::default, Valued::update, Valued::view).run()
}
