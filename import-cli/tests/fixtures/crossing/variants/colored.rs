// rawr
// iced's gradient and custom_quad in the fixture: `color!` makes a crate
// color through the crate's own path, as iced's `color!` names
// `$crate::Color`, and the program reads its fields, destructures it and
// builds one from it at the public path (#953).

use events::Color;

fn red(color: Color) -> u8 {
    color.r
}

fn main() {
    let shade = events::color!(1, 2, 3);
    let Color { r, g, b } = shade;
    println!("red {} of {r} {g} {b}", red(shade));
    let lighter = Color { r: 9, ..shade };
    println!("lighter {}", events::paint(lighter));
}
