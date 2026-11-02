// rawr
// iced's color_palette: `hsl.shift_hue(-135.0).lighten(0.075)`. palette's
// `ShiftHue::shift_hue(self, amount: Self::Scalar)` takes the trait's own
// associated type, which each impl sets to its component (`type Scalar =
// T`), so the amount's type is the projection on the receiver's type, as
// rustc reads it (#638).

use events::color::{convert::FromColor, Hsl, Lighten, Rgb, ShiftHue};

fn main() {
    let rgb: Rgb = Rgb {
        red: 0.25,
        green: 0.5,
        blue: 1.0,
        ..Rgb::default()
    };
    let hsl = Hsl::from_color(rgb);
    let shifted = hsl.shift_hue(-135.0);
    println!("hue {} {}", hsl.hue, shifted.hue);
    let lighter = hsl.shift_hue(120.0).lighten(0.5);
    println!(
        "hsl {} {} {}",
        lighter.hue, lighter.saturation, lighter.lightness
    );
}
