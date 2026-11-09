//! Native `main`, as inboard's: the marked `theme` module defines the app's
//! `Theme` over its native child module's theme, and native code uses it
//! as the crate's widgets do: through its `From` impls in both directions,
//! the crate's traits it implements, a `From<&Theme>` for the crate's
//! settings and a pattern on its variant.
mod theme;

use theme::Theme;

fn main() {
    let theme = Theme::default();
    println!("default {theme:?}");
    match &theme {
        Theme::Selected(palette) => println!("matched {}", palette.name),
    }
    println!("accent {}", theme.accent());
    println!("{}", ui::catalog(&theme));
    println!("{}", ui::chart(&theme));
    println!("{}", ui::view("hello", &theme));
    let settings = ui::Settings::from(&theme);
    println!("settings {settings:?}");
    let native: ui::Theme = theme.clone().into();
    println!("crate {native:?}");
    let back = Theme::from(native);
    println!("back {back:?} {}", back == theme);
    let dark = theme::dark();
    println!("dark {dark:?}");
    println!("{}", ui::chart(&dark));
    println!("hue {:?} {:?}", theme::Hue::Warm, theme::Hue::Cool);
}
