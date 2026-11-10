// rawr
// iced's game_of_life: `self.grid.view().map(Message::Grid.with(version))`.
// The grid's view is a program function that returns `impl Widget<Curve>`,
// and its `map` takes the `impl Fn(Curve) -> Message` that the crate's
// `Function::with` gives a two-field variant's constructor.

use events::canvas::{self, Bounds, Event, Geometry, Pen};
use events::widgets::{center, column, text, Framed, Widget};
use events::{Function, Theme};

#[derive(Default)]
struct Editor {
    curves: i64,
    version: usize,
}

#[derive(Debug, Clone)]
enum Curve {
    Drawn(String),
}

#[derive(Debug, Clone)]
enum Message {
    Curve(usize, Curve),
}

struct Bezier {
    curves: i64,
}

impl canvas::Program<Curve> for Bezier {
    type State = ();

    fn update(&self, _state: &mut (), event: &Event, _bounds: Bounds) -> Option<Curve> {
        match event {
            Event::Pressed(label) => Some(Curve::Drawn(label.clone())),
        }
    }

    fn draw(&self, _state: &(), pen: &Pen, _theme: &Theme, _bounds: Bounds) -> Vec<Geometry> {
        vec![pen.stroke(&format!("curves {}", self.curves))]
    }
}

fn grid(curves: i64) -> impl Widget<Curve> {
    canvas::canvas(Bezier { curves })
}

impl Editor {
    fn update(&mut self, message: Message) {
        match message {
            Message::Curve(version, Curve::Drawn(_)) => {
                self.curves += 1;
                self.version = version + 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let version = self.version;
        let controls = center(text(format!("{} curves", self.curves)));
        column![
            grid(self.curves).map(Message::Curve.with(version)),
            controls
        ]
    }
}

fn main() {
    events::widgets::run(Editor::update, Editor::view);
}
