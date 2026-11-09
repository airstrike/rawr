//! Native `main`, as inboard's: the marked `theme` module defines the app's
//! `Theme` over its native child module's theme, and native code uses it
//! as the crate's widgets do: through its `From` impls in both directions,
//! the crate's traits it implements, a `From<&Theme>` for the crate's
//! settings and a pattern on its variant.
mod home;
mod inbox;
mod style;
mod theme;
mod widget;

pub use theme::Theme;
pub use widget::Element;

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
    // Every style function reads the theme's colors where they are.
    let mut total = 0;
    for _ in 0..100 {
        total += style::button(&theme) + style::text(&dark);
    }
    let colors = theme.colors();
    println!(
        "styled {total} with {colors:?} at {}",
        theme.colors().accent
    );
    let Theme::Selected(palette) = &theme;
    println!("in place {}", std::ptr::eq(colors, &palette.colors));
    println!("{}", ui::base(&theme));
    let mut renamed = theme::dark();
    renamed.rename("dusk");
    println!("renamed {renamed:?} {}", ui::base(&renamed));
    let typeface = theme::typeface();
    println!(
        "typeface {} {} {typeface:?}",
        typeface.size(),
        typeface.family()
    );
    println!("{}", home::heading());
    println!("{}", home::container().draw(&theme));
    println!("{}", home::label().draw(&dark));
    println!("view {}", home::view(&theme));
    println!("view {}", home::view(&dark));
    println!("{}", home::empty().draw(&theme));
    let badge = home::badge(&dark);
    println!("{}", badge.draw());
    let inbox = home::inbox(&theme);
    println!("inbox {} {:?}", inbox.draw(&theme), inbox.message());
    println!("titled {}", home::titled("mail", &dark).draw(&theme));
    println!(
        "lent in place {}",
        home::place(&theme) == std::ptr::from_ref(&theme).addr()
    );
    println!("clones {}", theme::clones());
}
