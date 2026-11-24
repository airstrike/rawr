// rawr
// `tinted.rs` edited: a new program type through the crate's generic
// `shade`, `Panel` and `draw`, and new native inputs to `shade`: a `Tint`
// itself and a `[u8; 3]` the crate converts.

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

    let panel = Panel::new("moods").push(Mood::Calm).push(Mood::Bright);
    let mut canvas = Canvas::new();
    tinted::draw(&panel, &mut canvas);
    let named = panel.map(|mood| format!("{mood:?}"));
    println!("{} {} {}", named.title, named.len(), named.is_empty());
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
    let warm = Panel::new("warm").push(Warmth(200)).push(Warmth(40));
    let mut cells = Canvas::new();
    tinted::draw(&warm, &mut cells);
    for cell in cells.cells() {
        println!("{cell}");
    }
    println!("shaded {}", tinted::shaded());
    println!("{:?}", calm.retinted(Mood::Bright).tint());
}
