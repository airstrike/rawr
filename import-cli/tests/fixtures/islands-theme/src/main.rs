//! Native `main`, as inboard's: the marked `theme` module defines the app's
//! `Theme` over its native child module's theme, and native code uses it
//! as the crate's widgets do: through its `From` impls in both directions,
//! the crate's traits it implements, a `From<&Theme>` for the crate's
//! settings and a pattern on its variant.
mod grid;
mod home;
mod inbox;
// Marked builders as inboard's, over boxed iterators of elements: native
// code builds one through their chain, and `rawr check` checks the rest.
#[allow(dead_code)]
mod sidebar;
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

/// A native helper generic over the message, as inboard's
/// `component::sidebar::atoms::small_input`, which calls the marked
/// `row_group` at whatever message it is called with.
fn small_row<'a, M: Clone + 'a>(label: &'a str, message: M) -> Element<'a, M> {
    widget::sidebar::row_group(label, message)
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
    let sidebar = home::sidebar(&dark);
    println!("sidebar {} {:?}", sidebar.draw(&dark), sidebar.message());
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
    let label = String::from("caption");
    let caption = home::caption(&label);
    let crate_theme = ui::Theme::custom("crate".to_owned(), 7);
    println!(
        "{} on native text {}",
        caption.draw(&crate_theme),
        std::ptr::eq(caption.content(), label.as_str())
    );
    let heading = String::from("panel");
    let mut panel = home::Panel::new(&heading)
        .item(home::inbox(&theme))
        .item(home::titled("mail", &dark));
    panel.push(home::page(None, &dark));
    println!("{}", panel.draw(&theme));
    println!(
        "panel on native text {}",
        std::ptr::eq(panel.title(), heading.as_str())
    );
    println!("sections {}", home::sections());
    // A marked function generic over the message, at two of native code's
    // message types: directly, through `.into()` and a crate's column, and
    // from a generic native helper.
    let row = widget::sidebar::row_group("tiles", grid::Message::Tile(4));
    println!("sidebar {} {:?}", row.draw(&theme), row.message());
    let column = ui::Column::<Element<'_, Message>>::with_children([widget::sidebar::row_group(
        "home",
        Message::Home(home::Message::Grid(grid::Message::Tile(5))),
    )
    .into()]);
    {
        use ui::Widget;
        println!(
            "sidebar {}",
            Widget::<Message, Theme>::draw(&column, &theme)
        );
    }
    let row = small_row(
        "helper",
        Message::Home(home::Message::Grid(grid::Message::Tile(6))),
    );
    println!("sidebar {} {:?}", row.draw(&theme), row.message());
    let row = small_row("grid", grid::Message::Tile(7));
    println!("sidebar {} {:?}", row.draw(&theme), row.message());
    // A marked sidebar generic over the message, at two of native code's
    // message types: a marked generic fn with an `impl IntoIterator`
    // argument makes it, its generic methods build it by value and by `&mut
    // self`, native code keeps it, reads its title by `&self` and draws the
    // element it turns into, and a marked `From` of a generic marked type
    // gives an element by `.into()`.
    let title = String::from("tiles");
    let mut tiles = widget::sidebar::header(&title, [small_row("add", grid::Message::Tile(8))])
        .content([widget::sidebar::row_group("first", grid::Message::Tile(9))])
        .spacing(3.0);
    println!(
        "sidebar {} on native text {}",
        tiles.title(),
        std::ptr::eq(tiles.title(), title.as_str())
    );
    tiles.push(small_row("pushed", grid::Message::Tile(12)));
    let element = tiles.into_element();
    println!("sidebar {} {:?}", element.draw(&theme), element.message());
    let screen = widget::sidebar::header("home", std::iter::empty()).content(vec![small_row(
        "inbox",
        Message::Home(home::Message::Grid(grid::Message::Tile(10))),
    )]);
    let element = screen.into_element();
    println!("sidebar {} {:?}", element.draw(&theme), element.message());
    // The marked `Content` given whole, by its own `IntoContent` impl, as
    // inboard's `.content(sidebar::content(..))`.
    let given =
        widget::sidebar::header::<grid::Message>("given", []).content(widget::sidebar::Content {
            items: vec![small_row("whole", grid::Message::Tile(14))],
            spacing: ui::Pixels(2.0),
        });
    let element = given.into_element();
    println!("sidebar {} {:?}", element.draw(&theme), element.message());
    let group: Element<'_, grid::Message> = widget::sidebar::Group {
        label: "rows",
        rows: vec![small_row("row", grid::Message::Tile(11))],
    }
    .into();
    println!("sidebar {} {:?}", group.draw(&theme), group.message());
    let strip = grid::strip("strip");
    println!("sidebar {} {:?}", strip.draw(&theme), strip.message());
    // inboard's own chain: marked builders keep the elements native code
    // gives them in a `Box<dyn Iterator>` they make, which native code
    // holds and pulls from one at a time, as marked code does between
    // (#1600).
    let mut tools = sidebar::header::<grid::Message>(
        "tools",
        [
            small_row("add", grid::Message::Tile(15)),
            small_row("more", grid::Message::Tile(16)),
            small_row("last", grid::Message::Tile(17)),
        ],
    )
    .content([small_row("first", grid::Message::Tile(18))])
    .spacing(4);
    if let Some(header) = &mut tools.header {
        let pulled = header.actions.next();
        println!("pulled {:?}", pulled.map(|action| action.draw(&theme)));
    }
    let marked = tools.next_action();
    println!("pulled {:?}", marked.map(|action| action.message()));
    if let Some(header) = &mut tools.header {
        println!("pulled {} more", header.actions.by_ref().count());
    }
    let first = tools.content.content.next();
    println!(
        "pulled {:?} of {}",
        first.map(|item| item.draw(&theme)),
        tools.header.map_or("none", |header| header.title)
    );
    println!("clones {}", theme::clones());
    // Run on, as a running program does, till told to stop: each tick
    // calls the marked generic through the native helper.
    if let Some(stop) = std::env::var_os("ISLANDS_STOP") {
        let mut tick = 0;
        while !std::path::Path::new(&stop).exists() {
            tick += 1;
            let row = small_row("tick", grid::Message::Tile(tick));
            println!("tick {tick} {}", row.draw(&theme));
            let bar = widget::sidebar::header::<grid::Message>("bar", []).spacing(1.0);
            println!("tock {tick} {}", bar.into_element().draw(&theme));
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
