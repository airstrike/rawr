// rawr
// iced's clock, game_of_life and the_matrix in the fixture: a number's
// `.into()` where a crate struct's field takes a `Pixels` goes through the
// `From` impl of `Pixels` that takes it, as rustc infers it. A float
// literal's `14.0.into()` is the `f32` of `From<f32>`, an integer literal's
// the `u32` of `From<u32>`, and an `f32` value or constant takes
// `From<f32>`.

use events::conversions::{frame, Caption};
use events::widgets::{column, Widget};

#[derive(Debug, Clone, Copy)]
enum Message {
    Grow,
}

struct Sizes {
    radius: f32,
}

impl Default for Sizes {
    fn default() -> Self {
        Sizes { radius: 150.0 }
    }
}

impl Sizes {
    fn update(&mut self, message: Message) {
        match message {
            Message::Grow => self.radius += 15.0,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        const CELL_SIZE: f32 = 10.0;
        let radius = self.radius;
        let computed = Caption {
            content: "computed".to_string(),
            size: (radius / 15.0).into(),
            ..Caption::default()
        };
        let literal = Caption {
            content: "literal".to_string(),
            size: 14.0.into(),
        };
        let integer = Caption {
            content: "integer".to_string(),
            size: 20.into(),
        };
        let constant = Caption {
            content: "constant".to_string(),
            size: CELL_SIZE.into(),
        };
        column![
            frame("grow").on_press(Message::Grow),
            frame(format!("{computed:?}")),
            frame(format!("{literal:?}")),
            frame(format!("{integer:?}")),
            frame(format!("{constant:?}")),
        ]
    }
}

fn main() {
    events::controls::application(Sizes::default, Sizes::update, Sizes::view).run()
}
