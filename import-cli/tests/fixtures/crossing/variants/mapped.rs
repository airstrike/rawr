// rawr
// iced's bezier_tool and game_of_life: a canvas whose program sends its
// own messages, and `.map(Message::Curve)` turns them into the program's.
// The crate's `map` names its callable's type in its output, `Map<Self, F,
// Message>`.

use events::canvas::{self, Bounds, Event, Geometry, Pen};
use events::widgets::{center, Framed, Widget};
use events::Theme;

#[derive(Default)]
struct Editor {
    curves: i64,
}

#[derive(Debug, Clone)]
enum Curve {
    Drawn(String),
}

#[derive(Debug, Clone)]
enum Message {
    Curve(Curve),
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

impl Editor {
    fn update(&mut self, message: Message) {
        match message {
            Message::Curve(Curve::Drawn(_)) => self.curves += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let bezier = canvas::canvas(Bezier {
            curves: self.curves,
        });
        center(bezier.map(Message::Curve))
    }
}

fn main() {
    events::widgets::run(Editor::update, Editor::view);
}
