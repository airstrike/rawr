// rawr
// iced's gallery card: `float(image(allocation.handle()))` wraps the image
// in a widget whose type names no renderer, and boxes it in a function
// written `-> impl Widget<Message> + 'a`. Only that output's defaulted
// renderer fixes the image's `Handle`, through the renderer's
// `imaging::Renderer::Handle`, so `&Handle` converts into the crate's own
// handle, as rustc infers it (#1213, #1099).

use events::image::Handle;
use events::widgets::{button, column, picture, tile, text, Framed, Widget};

struct Gallery {
    handle: Option<Handle>,
    count: u16,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            handle: Some(Handle::from_name("sun")),
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
            card(self.handle.as_ref(), self.count),
            button("Increment").on_press(Message::Increment),
        ]
    }
}

fn card<'a>(handle: Option<&'a Handle>, count: u16) -> impl Widget<Message> + 'a {
    if let Some(handle) = handle {
        tile(picture(handle).width(count)).boxed()
    } else {
        text("none").boxed()
    }
}

fn main() {
    events::widgets::run(Gallery::update, Gallery::view);
}
