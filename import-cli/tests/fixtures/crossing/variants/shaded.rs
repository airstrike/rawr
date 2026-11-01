// rawr
// A literal of a crate struct whose public field has a type rawr can't
// hold yet, a function pointer: valid Rust, which rawr reports at the
// literal, naming the field and its type as the crate writes them.

use events::advanced::graphics::mesh::{self, Shaded};

fn half(x: f32) -> f32 {
    x / 2.0
}

fn main() {
    let shaded = Shaded {
        position: [1.0, 2.0],
        shade: half,
    };
    println!("{}", mesh::shade(&shaded));
}
