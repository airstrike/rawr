// rawr
// iced's color_palette (#628): `Rgb::from_color(lab)` goes through
// palette's impl `FromColorUnclamped<Lab<<S::Space as RgbSpace>::WhitePoint,
// T>> for Rgb<S, T>`, whose input projects the standard's white point.
// rustc resolves the projection at `Rgb`'s `Srgb` to `D65`, and so does
// Rawr.

use events::color::{convert::FromColor, Lab, Rgb};

fn to_rgb(lab: Lab) -> Rgb {
    Rgb::from_color(lab)
}

fn main() {
    let lab: Lab = Lab::new(50.0, 20.0, -40.0);
    let rgb: Rgb = Rgb::from_color(lab);
    println!("rgb {} {} {}", rgb.red, rgb.green, rgb.blue);
    let bright = to_rgb(Lab::new(90.0, -10.0, 30.0));
    println!("rgb {} {} {}", bright.red, bright.green, bright.blue);
}
