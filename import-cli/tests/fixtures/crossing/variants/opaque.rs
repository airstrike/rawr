// rawr
// Opaque `impl Widget` outputs, as cargo checks them under edition 2024:
// one built from its parameter, and one from a `&'static str`, take no
// lifetime from an input. A canvas behind `impl Widget<Curve>` is mapped,
// and `column!` boxes the `Map` through the trait's own `boxed`, as `Map`'s
// impl has parameters its type does not.

use events::canvas::{self, Bounds, Event, Geometry, Pen};
use events::widgets::{button, column, Framed, Widget};
use events::Theme;

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
    Curve(Curve),
}

#[derive(Debug, Clone)]
enum Curve {
    Drawn(String),
}

struct Bezier {
    value: i64,
}

impl canvas::Program<Curve> for Bezier {
    type State = ();

    fn update(&self, _state: &mut (), event: &Event, _bounds: Bounds) -> Option<Curve> {
        match event {
            Event::Pressed(label) => Some(Curve::Drawn(label.clone())),
        }
    }

    fn draw(&self, _state: &(), pen: &Pen, _theme: &Theme, _bounds: Bounds) -> Vec<Geometry> {
        vec![pen.stroke(&format!("value {}", self.value))]
    }
}

fn increment(message: Message) -> impl Widget<Message> {
    button("Increment").on_press(message)
}

fn decrement() -> impl Widget<Message> {
    let label: &'static str = "Decrement";
    button(label).on_press(Message::Decrement)
}

fn bezier(value: i64) -> impl Widget<Curve> {
    canvas::canvas(Bezier { value })
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
            Message::Curve(Curve::Drawn(label)) => self.value += label.len() as i64,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            increment(Message::Increment),
            decrement(),
            bezier(self.value).map(Message::Curve),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
