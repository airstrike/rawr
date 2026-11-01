// rawr
// iced's gallery (#626): `stack![content, viewer]` takes `viewer:
// Option<impl Widget<Message>>` as it is, through the crate's `impl Widget
// for Option<T>`. The viewer is `None` until the second press, before
// anything has made a value of the opaque type, and the call that boxes
// the `Option` still runs at that type, as rustc compiles it.

use events::controls::slider;
use events::widgets::{button, center, column, opaque, Widget};

#[derive(Default)]
struct Gallery {
    count: u8,
    viewer: Viewer,
}

#[derive(Default)]
struct Viewer {
    open: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Slid(u8),
}

impl Viewer {
    fn view(&self, count: u8) -> Option<impl Widget<Message>> {
        if !self.open {
            return None;
        }

        Some(opaque(center(slider(0..=10, count, Message::Slid))))
    }
}

impl Gallery {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.count += 1;
                self.viewer.open = self.count >= 2;
            }
            Message::Slid(count) => self.count = count,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let viewer = self.viewer.view(self.count);

        column![button("Increment").on_press(Message::Increment), viewer]
    }
}

fn main() {
    events::widgets::run(Gallery::update, Gallery::view);
}
