// rawr
// iced's color_palette: `Hsl::from_color(rgb)` and `Rgb::from_color(hsl)`
// call an associated function of palette's blanket `impl<T, U>
// FromColor<T> for U where U: FromColorUnclamped<T> + Clamp`, through the
// type's path and through its module's.

use events::color::{self, convert::FromColor, Hsl, Rgb};

fn to_rgb(red: f32, green: f32, blue: f32) -> Rgb {
    Rgb {
        red,
        green,
        blue,
        ..Rgb::default()
    }
}

fn main() {
    let rgb = to_rgb(0.25, 0.5, 1.0);
    let hsl = color::Hsl::from_color(rgb);
    println!("hsl {} {} {}", hsl.hue, hsl.saturation, hsl.lightness);
    let shifted = Hsl {
        hue: hsl.hue + 400.0,
        ..hsl
    };
    let back = Rgb::from_color(shifted);
    println!("rgb {} {} {}", back.red, back.green, back.blue);
    let again = Hsl::from_color(to_rgb(1.0, 0.0, 0.0));
    println!("hsl {} {} {}", again.hue, again.saturation, again.lightness);
}
