// rawr
// `turn.into()` into a local no use gives a type: rustc's E0283, as the
// crate has `From<Turn>` for both `f32` and `f64`.

use events::controls::Turn;

fn main() {
    let turn = Turn(0.25);
    let _unused = turn.into();
}
