// rawr
// iced's gallery (#623): `Viewer::view` gives `Option<impl
// Widget<Message>>`, and its body returns `Some(opaque(..))`. `opaque`
// leaves the theme and the renderer open, and the written `impl
// Widget<Message>` fixes them by the trait's defaults, as rustc does.
// The view boxes the viewer's widget inside the `Option`.

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
        column![
            button("Increment").on_press(Message::Increment),
            self.viewer.view(self.count).map(|viewer| viewer.boxed()),
        ]
    }
}

fn main() {
    events::widgets::run(Gallery::update, Gallery::view);
}
