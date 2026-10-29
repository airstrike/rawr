// rawr
// iced's the_matrix: the program's own type implements the crate's
// `canvas::Program` over any message, and its state is a std type Rust
// code would hold as a script value, `RefCell<Vec<i64>>`, which the trait
// asks `Default` of. The canvas keeps the state between draws, and `draw`
// fills it the first time and changes it each time after.

use std::cell::RefCell;

use events::canvas::{self, Bounds, Geometry, Pen};
use events::widgets::Widget;
use events::Theme;

#[derive(Default)]
struct Ticker {
    tick: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
}

impl Ticker {
    fn update(&mut self, message: Message) {
        match message {
            Message::Tick => {
                self.tick += 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        canvas::canvas(self)
    }
}

impl<Message> canvas::Program<Message> for Ticker {
    type State = RefCell<Vec<i64>>;

    fn draw(
        &self,
        state: &Self::State,
        pen: &Pen,
        _theme: &Theme,
        bounds: Bounds,
    ) -> Vec<Geometry> {
        let mut cells = state.borrow_mut();
        if cells.is_empty() {
            for cell in 0..3 {
                cells.push(cell * 10);
            }
        }
        cells[0] += 1;
        vec![pen.stroke(&format!(
            "{} cells {:?} at {}x{} tick {}",
            cells.len(),
            cells,
            bounds.width,
            bounds.height,
            self.tick
        ))]
    }
}

fn main() {
    events::widgets::run(Ticker::update, Ticker::view);
}
