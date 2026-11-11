// rawr
// iced's bezier_tool (#990): the canvas program's `update` sets the state
// the crate lends it, `*state = Some(Pending::One { .. })`, where the
// state is an `Option` of the program's own type. The canvas keeps what
// the update left, and `draw` shows it.

use events::canvas::{self, Bounds, Event, Geometry, Pen};
use events::widgets::Widget;
use events::Theme;

#[derive(Default)]
struct Editor {
    presses: i64,
}

#[derive(Debug, Clone, Copy)]
enum Pending {
    One { from: i64 },
    Two { from: i64, to: i64 },
}

#[derive(Debug, Clone)]
enum Message {
    Curve(i64, i64, i64),
}

impl Editor {
    fn update(&mut self, message: Message) {
        match message {
            Message::Curve(..) => self.presses += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        canvas::canvas(self)
    }
}

impl canvas::Program<Message> for Editor {
    type State = Option<Pending>;

    fn update(&self, state: &mut Self::State, event: &Event, _bounds: Bounds) -> Option<Message> {
        let at = match event {
            Event::Pressed(label) => label.len() as i64 + self.presses,
        };
        match *state {
            None => {
                *state = Some(Pending::One { from: at });
                None
            }
            Some(Pending::One { from }) => {
                *state = Some(Pending::Two { from, to: at });
                None
            }
            Some(Pending::Two { from, to }) => {
                *state = None;
                Some(Message::Curve(from, to, at))
            }
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        pen: &Pen,
        _theme: &Theme,
        _bounds: Bounds,
    ) -> Vec<Geometry> {
        vec![pen.stroke(&format!("pending {state:?}"))]
    }
}

fn main() {
    events::widgets::run(Editor::update, Editor::view);
}
