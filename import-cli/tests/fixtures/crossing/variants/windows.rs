// rawr
// iced's multi_window: the program's view passes a window's view, a
// method whose `impl Widget` output is declared after its caller, to a
// generic crate function. The caller's body waits for that output's
// hidden type, as rustc reads the whole crate before it checks a body.

use events::widgets::{button, center, column, text, Widget};
use std::collections::BTreeMap;

struct Example {
    windows: BTreeMap<u64, Window>,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Default for Example {
    fn default() -> Self {
        let mut windows = BTreeMap::new();
        windows.insert(1, Window::new("One"));
        Example { windows }
    }
}

impl Example {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                if let Some(window) = self.windows.get_mut(&1) {
                    window.count += 1;
                }
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        if let Some(window) = self.windows.get(&1) {
            center(window.view(1)).padding(3).boxed()
        } else {
            text("none").boxed()
        }
    }
}

struct Window {
    title: String,
    count: i64,
}

impl Window {
    fn new(title: &str) -> Self {
        Window {
            title: title.to_string(),
            count: 0,
        }
    }

    fn view(&self, id: u64) -> impl Widget<Message> {
        column![
            text(format!("{} {id} {}", self.title, self.count)),
            button("Increment").on_press(Message::Increment),
        ]
    }
}

fn main() {
    events::widgets::run(Example::update, Example::view);
}
