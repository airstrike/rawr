// rawr
// Opaque `impl Widget` outputs, as cargo checks them under edition 2024:
// one built from its parameter, and one from a `&'static str`, take no
// lifetime from an input. A canvas behind `impl Widget<Curve>` is mapped,
// and `column!` boxes the `Map` through the trait's own `boxed`, as `Map`'s
// impl has parameters its type does not. A button behind `impl
// Widget<Part>` is mapped too: its type is a `Widget` at any renderer, and
// only the `impl Widget<Part>` fixes the one `map` is called at. As
// iced's bezier_tool, the canvas is a module's: a method borrowing its
// state and the curves returns it behind `impl Framed<Curve>`, the trait
// `map` is a method of, and its program borrows both.

use events::widgets::{button, column, Framed, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
    bezier: bezier::State,
    curves: Vec<bezier::Curve>,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
    Curve(bezier::Curve),
    Part(Part),
}

#[derive(Debug, Clone)]
enum Part {
    Down,
}

fn increment(message: Message) -> impl Widget<Message> {
    button("Increment").on_press(message)
}

fn decrement() -> impl Widget<Message> {
    let label: &'static str = "Decrement";
    button(label).on_press(Message::Decrement)
}

fn part() -> impl Widget<Part> {
    button("Decrement").on_press(Part::Down)
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
            Message::Curve(curve) => {
                self.value += curve.length();
                self.curves.push(curve);
            }
            Message::Part(Part::Down) => self.value -= 2,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            increment(Message::Increment),
            part().map(Message::Part),
            decrement(),
            self.bezier.view(&self.curves).map(Message::Curve),
        ]
    }
}

mod bezier {
    use events::canvas::{self, Bounds, Event, Geometry, Pen};
    use events::widgets::Framed;
    use events::Theme;

    #[derive(Default)]
    pub struct State {
        strokes: i64,
    }

    impl State {
        pub fn view<'a>(&'a self, curves: &'a [Curve]) -> impl Framed<Curve> {
            canvas::canvas(Bezier {
                state: self,
                curves,
            })
        }
    }

    struct Bezier<'a> {
        state: &'a State,
        curves: &'a [Curve],
    }

    impl canvas::Program<Curve> for Bezier<'_> {
        type State = ();

        fn update(&self, _state: &mut (), event: &Event, _bounds: Bounds) -> Option<Curve> {
            match event {
                Event::Pressed(label) => Some(Curve::Drawn(label.clone())),
            }
        }

        fn draw(&self, _state: &(), pen: &Pen, _theme: &Theme, _bounds: Bounds) -> Vec<Geometry> {
            let label = format!("curves {} {}", self.curves.len(), self.state.strokes);
            vec![pen.stroke(&label)]
        }
    }

    #[derive(Debug, Clone)]
    pub enum Curve {
        Drawn(String),
    }

    impl Curve {
        pub fn length(&self) -> i64 {
            match self {
                Curve::Drawn(label) => label.len() as i64,
            }
        }
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
