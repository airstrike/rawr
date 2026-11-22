// rawr
// A program generic's `impl Into<f32>` takes a crate type whose
// `From<Turn> for f32` the crate implements, and a float, as rustc's
// monomorphised body does (#1719).

use events::controls::Turn;

fn degrees(turn: impl Into<f32>) -> f32 {
    turn.into()
}

fn halved<T: Into<f64>>(value: T) -> f64 {
    value.into() / 2.0
}

fn main() {
    println!("{} {}", degrees(Turn(0.25)), degrees(45.0));
    println!("{} {}", halved(Turn(0.5)), halved(3_u8));
}
