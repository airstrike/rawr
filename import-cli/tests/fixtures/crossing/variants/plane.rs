// rawr
// iced's game_of_life: `translation + (cursor_position - start) * (1.0 /
// scaling)`. `Point - Point` is a `Vector` and `Point - Vector` a `Point`,
// two impls of `Sub` for one generic type, each with its own `Output`.

use events::plane::{Point, Vector};

fn main() {
    let translation = Vector::new(1.0_f32, 2.0);
    let start = Point::new(3.0_f32, 4.0);
    let cursor = Point::new(10.0_f32, 20.0);
    let scaling = 2.0_f32;
    let moved = translation + (cursor - start) * (1.0 / scaling);
    let back: Point<f32> = cursor - moved;
    let ahead = back + moved;
    println!("{moved:?}");
    println!("{back:?} {ahead:?}");
}
