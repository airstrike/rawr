// rawr
// `turn.into()` into a local that only `y * 2.0` uses: a float, but not
// which, so neither the crate's `From<Turn>` for `f32` nor for `f64` is
// selected. rustc's E0283.

use events::controls::Turn;

fn main() {
    let turn = Turn(0.25);
    let y = turn.into();
    println!("{}", y * 2.0);
}
