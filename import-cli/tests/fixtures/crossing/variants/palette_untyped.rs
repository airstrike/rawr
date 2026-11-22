// rawr
// `Rgb { .., ..Rgb::default() }` with nothing that fixes the color's
// standard: palette's derived `Default` bounds it, so rustc's E0283 at
// the `let` (#611).

use events::color::Rgb;

fn main() {
    let color = Rgb {
        red: 1.0,
        ..Rgb::default()
    };
    println!("{}", color.red);
}
