// rawr
// iced's gallery: `float(image(allocation.handle()))` wraps the image in a
// widget whose type names no renderer, in a function written `-> impl
// Widget<Message> + 'a`, and the viewer's `Option<impl Widget<Message>>`
// holds one too. Each output's type names the image, and only the output's
// defaulted renderer fixes the image's `Handle`, through the renderer's
// `imaging::Renderer::Handle`, so `&Handle` converts into the crate's own
// handle, as rustc infers it (#1213, #1099).

use events::image::Handle;
use events::widgets::{button, column, picture, tile, Widget};

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
            card(&self.handle, self.count),
            self.viewer(),
            button("Increment").on_press(Message::Increment),
        ]
    }

    fn viewer(&self) -> Option<impl Widget<Message> + '_> {
        if self.count == 0 {
            return None;
        }
        Some(tile(picture(&self.handle)))
    }
}

fn card<'a>(handle: &'a Handle, count: u16) -> impl Widget<Message> + 'a {
    tile(picture(handle).width(count))
}

fn main() {
    events::widgets::run(Gallery::update, Gallery::view);
}
