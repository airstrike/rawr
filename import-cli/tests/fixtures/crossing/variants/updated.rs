// rawr
// iced's color_palette (#617): struct updates of a crate's generic struct.
// `Rgb { red, ..Rgb::default() }` takes green and blue from a default
// whose component type only the return type and `red` fix, and the
// program reads them later. `Hsl { lightness, ..hsl }` in a loop copies
// the other fields out of `hsl` each time, as they are `Copy`.

use events::color::{convert::FromColor, Hsl, Rgb};

fn tinted(red: f32) -> Rgb {
    Rgb {
        red,
        ..Rgb::default()
    }
}

fn main() {
    let rgb = tinted(0.5);
    println!("rgb {} {} {}", rgb.red, rgb.green, rgb.blue);
    let hsl = Hsl::from_color(tinted(1.0));
    for step in 1..4 {
        let graded = Hsl {
            lightness: step as f32 / 4.0,
            ..hsl
        };
        let back = Rgb::from_color(graded);
        println!("step {step} {} {} {}", back.red, back.green, back.blue);
    }
    println!("hsl {} {} {}", hsl.hue, hsl.saturation, hsl.lightness);
}
