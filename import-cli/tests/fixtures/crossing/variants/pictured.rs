// rawr
// iced's gallery and ferris: `picture(&self.handle)` and
// `picture("ferris")` call `picture<Handle>(impl Into<Handle>)`, whose
// `Handle` the argument doesn't fix. The widget's impl is of the renderers
// whose `imaging::Renderer::Handle` is `Handle`, so `column!`'s boxing and
// `.explain(..)` at the crate's renderer fix it to the crate's own handle,
// as rustc infers it, and each argument converts into it (#1099, #1071).

use events::image::Handle;
use events::widgets::{button, column, picture, Framed, Widget};

struct Gallery {
    handle: Handle,
    count: u16,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            handle: Handle::from_name("sun"),
            count: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Gallery {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.count += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            picture(&self.handle).width(self.count),
            picture("ferris").width(2).explain(0xffffff_u32),
            button("Increment").on_press(Message::Increment),
        ]
    }
}

fn main() {
    events::widgets::run(Gallery::update, Gallery::view);
}
