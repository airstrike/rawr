// rawr
// Colors with palette. palette's dependency `half` has an implementation
// Rawr cannot model, which is unavailable by itself: palette, iced and
// everything else import as usual.
use palette::encoding::Srgb;
use palette::rgb::Rgb;
use palette::RgbHue;

fn main() {
    // A generic struct, at the types the annotation names.
    let pink: Rgb<Srgb, f32> = Rgb::new(1.0, 0.2, 0.6);
    let _ = pink;
    println!("pink is an Rgb<Srgb, f32>");

    // A hue of -30 degrees is 330 degrees, as an f32.
    let hue: RgbHue<f32> = RgbHue::new(-30.0);
    println!("hue: {} degrees", hue.into_positive_degrees());
}
