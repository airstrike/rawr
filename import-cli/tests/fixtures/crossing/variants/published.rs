// rawr
// iced's bezier_tool: `canvas::Action::publish(Curve { from, to })`,
// printed with `{:?}`. The crate's derived `Debug` prints the program's
// curve, whose own `Debug` prints the crate's points it holds.

use events::plane::{Action, Point};

#[derive(Debug, Clone, Copy)]
struct Curve {
    from: Point,
    to: Point,
}

fn main() {
    let curve = Curve {
        from: Point::new(5.0, 5.0),
        to: Point::new(45.0, 75.0),
    };
    let action = Action::publish(curve).and_capture();
    println!("{action:?}");
    println!("{curve:?}");
}
