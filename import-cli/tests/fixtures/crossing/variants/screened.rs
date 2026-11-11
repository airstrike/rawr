// rawr
// iced's screenshot: `picture(handle)` of the `&Handle` a pattern binds,
// boxed by `.framed()` in one arm of an `if` whose other arm boxes text.
// `picture<Handle>(impl Into<Handle>)` leaves `Handle` to the widget's
// impl, whose renderer only the element's use in the view fixes, so the
// borrowed handle converts into the crate's own handle, as rustc infers
// it.

use events::image::Handle;
use events::widgets::{button, column, picture, text, Framed, Widget};

#[derive(Default)]
struct Screenshot {
    shot: Option<(u16, Handle)>,
    count: u16,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Screenshot {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.count += 1;
                self.shot = Some((self.count, Handle::from_name("shot")));
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let image = if let Some((_count, handle)) = &self.shot {
            picture(handle).width(self.count).framed()
        } else {
            text("Press the button").boxed()
        };
        column![image, button("Increment").on_press(Message::Increment)]
    }
}

fn main() {
    events::widgets::run(Screenshot::update, Screenshot::view);
}
