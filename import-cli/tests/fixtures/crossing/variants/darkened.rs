// rawr
// iced's color_palette: `hsl.shift_hue(-105.0).darken(0.075)`. palette's
// `impl<T: Lighten> Darken for T { type Scalar = T::Scalar; }` applies to
// `Hsl` through its own `Lighten` impl, so the factor's type is
// `<Hsl as Lighten>::Scalar`, as rustc normalizes it (#727).

use events::color::{convert::FromColor, Darken, Hsl, Rgb, ShiftHue};

fn main() {
    let rgb: Rgb = Rgb {
        red: 0.25,
        green: 0.5,
        blue: 1.0,
        ..Rgb::default()
    };
    let hsl = Hsl::from_color(rgb);
    let darker = hsl.shift_hue(-105.0).darken(0.075);
    println!(
        "hsl {} {} {}",
        darker.hue, darker.saturation, darker.lightness
    );
}
