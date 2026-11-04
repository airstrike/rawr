// rawr
// iced's layout in the fixture: `fn square(size: impl Into<Length> +
// Copy) -> impl Widget<Message>` takes an anonymous generic parameter of
// two bounds and passes it twice to a crate's methods that take `impl
// Into<Length>`. A number fills it through the crate's `From` impls, and
// a literal takes the kind of the one impl of its kind. A crate function
// takes such a parameter too.

use events::controls::application;
use events::conversions::{frame, sized, square, Pixels};
use events::widgets::{column, Widget};

#[derive(Debug, Clone, Copy)]
enum Message {
    Grow,
}

struct Squares {
    side: f32,
}

impl Default for Squares {
    fn default() -> Self {
        Squares { side: 10.0 }
    }
}

/// A square of side `size`, as iced's layout draws one.
fn boxed(label: &str, size: impl Into<Pixels> + Copy) -> impl Widget<Message> {
    sized(label).width(size).height(size)
}

impl Squares {
    fn update(&mut self, message: Message) {
        match message {
            Message::Grow => self.side += 5.0,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            frame("grow").on_press(Message::Grow),
            boxed("program", self.side),
            boxed("pixels", Pixels(3.0)),
            boxed("integer", 7u32),
            boxed("literal", 50),
            square("crate", self.side * 2.0),
            square("crate-integer", 4u32),
            square("crate-literal", 4),
        ]
    }
}

fn main() {
    application(Squares::default, Squares::update, Squares::view).run()
}
