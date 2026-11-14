// rawr
// iced's screenshot: `picture(handle).framed()` at an alias of a fallback
// renderer over backends a public path reaches, as iced's `Renderer` is
// over `wgpu::Renderer` and `tiny_skia::Renderer`. The renderer's
// `imaging::Renderer::Handle` is the generic impl's `A::Handle` at that
// instance, which fixes the picture's `Handle`, so `&Handle` converts into
// the crate's own handle, as rustc infers it (#1939).

use events::image::Handle;
use events::widgets::open::Renderer;
use events::widgets::{button, column, picture, Framed, Widget};
use events::Theme;

struct Shown {
    handle: Handle,
    count: u16,
}

impl Default for Shown {
    fn default() -> Self {
        Self {
            handle: Handle::from_name("open"),
            count: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Shown {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.count += 1,
        }
    }

    fn view(&self) -> impl Widget<Message, Theme, Renderer> {
        let handle = &self.handle;
        column![
            picture(handle).width(self.count).framed(),
            button("Increment").on_press(Message::Increment),
        ]
    }
}

fn main() {
    events::widgets::run(Shown::update, Shown::view);
}
