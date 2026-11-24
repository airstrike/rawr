// rawr
// A crate's generic code at a native type and at the program's own: `shade`
// takes `impl Into<Tint>`, and `draw` takes the program's `M`.
// Their bodies call the crate's private helper, build its private-field
// `Style` and bump its private static, which the crate's concrete `plain`
// bumps too.

use events::tinted::{self, Canvas, Tint};

#[derive(Debug, Clone)]
enum Mood {
    Calm,
    Bright,
}

impl From<Mood> for Tint {
    fn from(mood: Mood) -> Self {
        match mood {
            Mood::Calm => Tint {
                red: 40,
                green: 80,
                blue: 160,
            },
            Mood::Bright => Tint::from(240),
        }
    }
}

fn main() {
    let plain = tinted::plain();
    let gray = tinted::shade(200u8);
    let calm = tinted::shade(Mood::Calm);
    println!("{plain:?}");
    println!("{gray:?} {}", gray.order());
    println!("{:?} {}", calm.tint(), calm.order());

    let moods = [Mood::Calm, Mood::Bright];
    let mut canvas = Canvas::new();
    tinted::draw("moods", &moods, &mut canvas);
    for cell in canvas.cells() {
        println!("{cell}");
    }
    println!("shaded {}", tinted::shaded());
    println!("{:?}", calm.retinted(Mood::Bright).tint());
}
