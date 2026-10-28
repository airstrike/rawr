// rawr
// iced's canvas examples in the fixture's shape: the program's own type
// implements the crate's `canvas::Program`, and `canvas(self)` takes the
// view's `&self`. The crate calls `draw` back while it lays the view out,
// lending it a pen and a theme, and `update` while it handles a press,
// with the canvas's state, a script type, lent `&mut`. A script color
// converts into the crate's through the script's `impl From`, as lazy's
// does.

use events::canvas::{self, Bounds, Event, Geometry, Pen};
use events::widgets::Widget;
use events::Theme;

#[derive(Default)]
struct Sketch {
    presses: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Pressed(String),
}

#[derive(Default)]
struct Strokes {
    count: i64,
}

#[derive(Debug, Clone, Copy)]
enum Shade {
    Dark,
    Light,
}

impl From<Shade> for events::Color {
    fn from(shade: Shade) -> Self {
        match shade {
            Shade::Dark => events::Color::rgb(10, 20, 30),
            Shade::Light => events::Color::rgb(200, 210, 220),
        }
    }
}

impl Sketch {
    fn update(&mut self, message: Message) {
        match message {
            Message::Pressed(label) => {
                self.presses += label.len() as i64;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        canvas::canvas(self)
    }
}

impl canvas::Program<Message> for Sketch {
    type State = Strokes;

    fn update(&self, state: &mut Self::State, event: &Event, bounds: Bounds) -> Option<Message> {
        state.count += 1;
        match event {
            Event::Pressed(label) if bounds.area() > 0 => Some(Message::Pressed(label.clone())),
            Event::Pressed(_) => None,
        }
    }

    fn draw(&self, state: &Self::State, pen: &Pen, theme: &Theme, bounds: Bounds) -> Vec<Geometry> {
        let shade = if self.presses % 2 == 0 {
            Shade::Dark
        } else {
            Shade::Light
        };
        vec![
            pen.stroke(&format!("{} {}", theme.name(), self.presses)),
            pen.stroke(&format!(
                "{}x{} after {} {}",
                bounds.width,
                bounds.height,
                state.count,
                events::paint(shade)
            )),
        ]
    }
}

fn main() {
    events::widgets::run(Sketch::update, Sketch::view);
}
