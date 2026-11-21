// rawr
// game_of_life's shape: the program's `Grid` has an inherent
// `update(&mut self, message)` and implements the crate's
// `canvas::Program`, whose `update` takes four arguments. The trait is
// named only through `canvas` in `mod grid`, so it is not in scope where
// the app calls `self.grid.update(message)`, and rustc calls the inherent
// function there.

use events::widgets::Widget;
use grid::Grid;

#[derive(Debug, Clone)]
enum Message {
    Pressed(String),
}

#[derive(Default)]
struct App {
    grid: Grid,
}

impl App {
    fn update(&mut self, message: Message) {
        self.grid.update(message);
        println!("presses {}", self.grid.presses);
    }

    fn view(&self) -> impl Widget<Message> {
        self.grid.view()
    }
}

mod grid {
    use super::Message;
    use events::canvas::{self, Bounds, Event, Geometry, Pen};
    use events::widgets::Widget;
    use events::Theme;

    #[derive(Default)]
    pub struct Grid {
        pub presses: i64,
    }

    impl Grid {
        pub fn update(&mut self, message: Message) {
            match message {
                Message::Pressed(label) => self.presses += label.len() as i64,
            }
        }

        pub fn view(&self) -> impl Widget<Message> {
            canvas::canvas(self)
        }
    }

    impl canvas::Program<Message> for Grid {
        type State = ();

        fn update(&self, _state: &mut (), event: &Event, _bounds: Bounds) -> Option<Message> {
            match event {
                Event::Pressed(label) => Some(Message::Pressed(label.clone())),
            }
        }

        fn draw(&self, _state: &(), pen: &Pen, theme: &Theme, _bounds: Bounds) -> Vec<Geometry> {
            vec![pen.stroke(&format!("{} {}", theme.name(), self.presses))]
        }
    }
}

fn main() {
    events::widgets::run(App::update, App::view);
}
