// rawr
// `tinted_shaded.rs` edited: a new program type through the crate's generic
// `shade` and `draw`, and new native inputs to `shade`: a `Tint`
// itself and a `[u8; 3]` the crate converts.

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

/// A warmth from cold, 0, to hot, 255.
#[derive(Clone)]
struct Warmth(u8);

impl From<Warmth> for Tint {
    fn from(Warmth(heat): Warmth) -> Self {
        Tint {
            red: heat,
            green: heat / 2,
            blue: 255 - heat,
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

    let tint = tinted::shade(Tint {
        red: 8,
        green: 16,
        blue: 32,
    });
    let rgb = tinted::shade([10u8, 20, 30]);
    println!(
        "{:?} {} {:?} {}",
        tint.tint(),
        tint.order(),
        rgb.tint(),
        rgb.order()
    );
    let warm = [Warmth(200), Warmth(40)];
    let mut cells = Canvas::new();
    tinted::draw("warm", &warm, &mut cells);
    for cell in cells.cells() {
        println!("{cell}");
    }
    println!("shaded {}", tinted::shaded());
    println!("{:?}", calm.retinted(Mood::Bright).tint());
}
