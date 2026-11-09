//! Native `main`, as inboard's: the marked `theme` module defines the app's
//! `Theme` over its native child module's theme, and native code uses it
//! as the crate's widgets do: through its `From` impls in both directions,
//! the crate's traits it implements, a `From<&Theme>` for the crate's
//! settings and a pattern on its variant.
mod grid;
mod home;
mod inbox;
mod style;
mod theme;
mod widget;

pub use theme::Theme;
pub use widget::Element;

/// The workspace's messages, as inboard's, which map the home screen's.
#[derive(Debug, Clone, PartialEq)]
enum Message {
    Home(home::Message),
}

/// The workspace's pane, as inboard's: the marked home screen's element
/// over the theme, mapped into the workspace's messages.
fn pane<'a>(
    screen: &'a home::State,
    source: &'a home::wizard::Source,
    grid: Option<Element<'a, grid::Message>>,
    theme: &'a Theme,
) -> Element<'a, Message> {
    use ui::Widget;
    screen
        .view(source.view(), grid, theme)
        .map(Message::Home)
        .boxed()
}

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
    println!("{}", home::framed().draw(&theme));
    println!("{}", home::label().draw(&dark));
    println!("view {}", home::view(&theme));
    println!("view {}", home::view(&dark));
    println!("{}", home::empty().draw(&theme));
    let badge = home::badge(&dark);
    println!("{}", badge.draw());
    let inbox = home::inbox(&theme);
    println!("inbox {} {:?}", inbox.draw(&theme), inbox.message());
    println!("titled {}", home::titled("mail", &dark).draw(&theme));
    let page = home::page(grid::home(&theme), &theme);
    println!("page {} {:?}", page.draw(&theme), page.message());
    let page = home::page(None, &dark);
    println!("page {} {:?}", page.draw(&dark), page.message());
    println!(
        "lent in place {}",
        home::place(&theme) == std::ptr::from_ref(&theme).addr()
    );
    let source = home::wizard::Source::new(&["sales", "costs"]);
    let mut screen = home::State::new("welcome");
    for grid in [None, grid::home(&dark)] {
        let page = pane(&screen, &source, grid, &theme);
        println!("page {} {:?}", page.draw(&theme), page.message());
    }
    screen.open_wizard(3);
    let page = pane(&screen, &source, grid::home(&theme), &dark);
    println!("page {} {:?}", page.draw(&dark), page.message());
    println!("catalog read {}", source.reads());
    println!("clones {}", theme::clones());
}
