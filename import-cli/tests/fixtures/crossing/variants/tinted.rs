// rawr
// A crate's generic code at a native type and at the program's own: `shade`
// takes `impl Into<Tint>`, and `draw` and `Panel` take the program's `M`.
// Their bodies call the crate's private helper, build its private-field
// `Style` and bump its private static, which the crate's concrete `plain`
// bumps too.

use events::tinted::{self, Canvas, Panel, Tint};

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

    let panel = Panel::new("moods").push(Mood::Calm).push(Mood::Bright);
    let mut canvas = Canvas::new();
    tinted::draw(&panel, &mut canvas);
    let named = panel.map(|mood| format!("{mood:?}"));
    println!("{} {} {}", named.title, named.len(), named.is_empty());
    for cell in canvas.cells() {
        println!("{cell}");
    }
    println!("shaded {}", tinted::shaded());
    println!("{:?}", calm.retinted(Mood::Bright).tint());
}
