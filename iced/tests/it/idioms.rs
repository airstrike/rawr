//! Idiomatic iced builder shapes compile and lower as in Rust.

use rawr_iced::program;

fn boot(view: &str) -> program::Adapter {
    boot_with(view, "")
}

fn boot_with(view: &str, items: &str) -> program::Adapter {
    let source = format!(
        r#"
use iced::{{Alignment, Border, Color, Element, Fill, FillPortion, Font, Length, Padding, Shrink, Theme, Widget}};
use iced::{{border, padding}};
use iced::widget::{{button, checkbox, column, container, row, scrollable, slider, text, text_input, toggler}};
struct State {{}}
#[derive(Debug, Clone)] enum Message {{ Pressed, Edited(String), Toggled(bool), Moved(i64) }}
impl State {{
    fn new() -> State {{ State {{}} }}
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> impl Widget<Message> {{ {view} }}
}}
{items}
pub fn main() -> iced::Result {{
    iced::application(State::new, State::update, State::view).run()
}}
"#
    );
    rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(&source)
        .unwrap_or_else(|error| panic!("{error}"))
        .prepare()
        .unwrap()
        .into_adapter()
}

fn find(adapter: &program::Adapter, label: &str) {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.find(label).unwrap();
    assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
}

#[test]
fn length_arguments_take_pixels_fill_and_portions() {
    let adapter = boot(
        r#"
        column![
            container(text("fixed")).width(96).height(40.5),
            row![text("a").width(FillPortion(2)), text("b").width(Length::FillPortion(1))]
                .width(Fill),
            container(text("shrink")).width(Shrink).height(Length::Shrink),
            container(text("float")).width(Length::Fixed(9.5)),
        ]
        .height(Fill)
        "#,
    );
    find(&adapter, "fixed");
}

#[test]
fn padding_arguments_take_pixels_and_padding_values() {
    let adapter = boot(
        r#"
        column![
            container(text("uniform")).padding(8),
            container(text("float")).padding(8.5),
            row![text("sides")].padding(Padding::new(8).left(16).right(16)),
            button("pad").padding(padding::horizontal(12).vertical(4)),
            text_input("name", "").padding(Padding::ZERO),
        ]
        .padding(padding::all(4))
        "#,
    );
    find(&adapter, "sides");
}

#[test]
fn builders_mirror_the_pinned_iced_widgets() {
    let adapter = boot(
        r#"
        column![
            scrollable(column![text("tall")].height(Fill)).width(Fill).height(200),
            text("styled")
                .color(Color::from_rgb(0.5, 0.2, 0.1))
                .font(Font::MONOSPACE)
                .line_height(1.5)
                .size(14.5)
                .height(Shrink),
            text_input("name", "").width(160).on_input(Message::Edited),
            toggler(true).size(18).on_toggle(Message::Toggled),
            button(row![text("go")].align_y(Alignment::End)).width(Fill),
            button(text("press")).on_press(Message::Pressed),
            button("subtle").style(button::subtle),
            button("background").style(button::background),
            text("base").style(text::base),
        ]
        .align_x(Alignment::Start)
        "#,
    );
    find(&adapter, "styled");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("press").unwrap();
    assert!(simulator.into_messages().next().is_some());
}

#[test]
fn container_style_functions_express_square_corners_and_colors() {
    let adapter = boot_with(
        r#"container(text("panel")).style(panel)"#,
        r#"
use iced::widget::container::Style;
fn panel(theme: &Theme) -> Style {
    Style::default()
        .background(Color::from_rgb(0.1, 0.1, 0.12))
        .color(Color::WHITE)
        .border(border::rounded(0).width(1).color(Color::BLACK))
}
"#,
    );
    find(&adapter, "panel");

    let failing = boot_with(
        r#"container(text("panel")).style(broken)"#,
        r#"
use iced::widget::container::Style;
fn broken(theme: &Theme) -> Style {
    Style::default().border(Border::default().width(0 - 1))
}
"#,
    );
    let mut simulator = iced_test::simulator(failing.view());
    assert!(simulator.find("panel").is_err());
}

/// `label` renders and no style failed while lowering: a failing style
/// function replaces its widget with the failure.
fn styled(adapter: &program::Adapter, label: &str) {
    find(adapter, label);
    let mut simulator = iced_test::simulator(adapter.view());
    let failed = simulator.find(
        |candidate: iced_test::selector::Candidate<'_>| match candidate {
            iced_test::selector::Candidate::Text { content, .. }
                if content.contains("style failed") =>
            {
                Some(content.to_owned())
            }
            _ => None,
        },
    );
    assert!(failed.is_err(), "{failed:?}");
}

fn bounds(adapter: &program::Adapter, label: &str) -> iced::Rectangle {
    let mut simulator = iced_test::simulator(adapter.view());
    let bounds = simulator.find(label).unwrap().bounds();
    assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
    bounds
}

#[test]
fn columns_and_rows_push_children_in_loops() {
    let adapter = boot_with(
        "ticks()",
        r#"
fn ticks() -> Element<'static, Message> {
    let mut ruler = row![];
    for tick in 0..3 {
        ruler = ruler.push(text(tick));
    }
    column![text("ruler")].push(ruler)
}
"#,
    );
    find(&adapter, "2");
}

#[test]
fn negative_spacing_overlaps_children() {
    let adapter = boot(r#"column![text("a"), text("b")].spacing(0 - 4)"#);
    let (a, b) = (bounds(&adapter, "a"), bounds(&adapter, "b"));
    assert_eq!(b.y, a.y + a.height - 4.0);
}

#[test]
fn a_column_grows_for_a_fill_child() {
    let adapter = boot(
        r#"
        container(column![
            text("top"),
            row![container(text("bottom")).height(Fill).align_y(Alignment::End)].height(Fill),
        ])
        .height(300)
        "#,
    );
    let bottom = bounds(&adapter, "bottom");
    assert_eq!(bottom.y + bottom.height, 300.0);
}

#[test]
fn colors_take_bytes_and_packed_hex() {
    let adapter = boot(
        r#"
        column![
            text("bytes").color(Color::from_rgb8(255, 128, 0)),
            text("alpha").color(Color::from_rgba8(255, 128, 0, 0.5)),
            text("hex").color(Color::from_packed_rgb8(0xff8000)),
            text("hex alpha").color(Color::from_packed_rgba8(0xff8000, 0.5)),
        ]
        "#,
    );
    find(&adapter, "hex");
}

#[test]
fn text_inputs_and_texts_take_borrowed_strings() {
    let adapter = boot_with(
        "draft(String::from(\"typed\"))",
        r#"
fn draft(value: String) -> Element<'static, Message> {
    column![text(&value), text_input("name", &value).on_input(Message::Edited)]
}
"#,
    );
    find(&adapter, "typed");
}

#[test]
fn typed_f32_and_u16_sizes_pass_as_in_rust() {
    let adapter = boot_with(
        "sized()",
        r#"
const GAP: f32 = 8.0;
const EDGE: u16 = 4;
fn sized() -> Element<'static, Message> {
    let framed = container(text("framed")).padding(padding::horizontal(GAP).vertical(GAP));
    column![text("sized").size(GAP), framed, container(text("edge")).padding(EDGE)]
        .spacing(GAP)
        .width(GAP)
}
"#,
    );
    find(&adapter, "sized");
}

#[test]
fn text_helpers_take_anything_into_a_string() {
    let adapter = boot_with(
        "labels()",
        r#"
use iced::widget::Text;
fn caption<T: Into<String>>(content: T) -> Text<'static> {
    let content: String = content.into();
    text(content).size(11)
}
fn named(name: &'static str) -> Element<'static, Message> {
    caption(name)
}
fn labels() -> Element<'static, Message> {
    column![caption("literal"), caption(format!("formatted {}", 2)), named("borrowed")]
}
"#,
    );
    find(&adapter, "literal");
    find(&adapter, "formatted 2");
    find(&adapter, "borrowed");
}

#[test]
fn custom_themes_are_built_from_a_seed_as_in_rust() {
    let adapter = boot_with(
        "sheet()",
        r#"
use iced::theme::palette::Seed;
fn custom(name: &str, background: Color, text: Color) -> Theme {
    let accent = Color::from_rgb8(0xff, 0x2e, 0x93);
    Theme::custom(name, Seed { background, text, primary: accent, success: accent, warning: text, danger: accent })
}
fn shade(theme: Theme) -> &'static str {
    if theme.palette().is_dark { "dark" } else { "light" }
}
fn sheet() -> Element<'static, Message> {
    let ink = custom("Ink", Color::from_rgb8(0x14, 0x14, 0x16), Color::from_rgb8(0xe4, 0xe4, 0xe8));
    let paper = custom("Paper", Color::from_rgb8(0xe6, 0xe6, 0xea), Color::from_rgb8(0x14, 0x14, 0x16));
    let seed = ink.seed();
    let read = Seed { primary: seed.text, ..seed };
    column![text(format!("ink {}", shade(ink))), text(format!("paper {}", shade(paper))), text(format!("reads {}", shade(Theme::custom("Read", read))))]
}
"#,
    );
    find(&adapter, "ink dark");
    find(&adapter, "paper light");
    find(&adapter, "reads dark");
}

#[test]
fn style_functions_read_the_theme_palette() {
    let adapter = boot_with(
        r#"container(text("panel")).style(panel)"#,
        r#"
use iced::widget::container::Style;
fn panel(theme: &Theme) -> Style {
    let palette = theme.palette();
    let rule = if palette.is_dark { palette.background.strong.color } else { theme.seed().text };
    Style::default()
        .background(palette.background.weak.color)
        .color(palette.primary.strong.text)
        .border(border::width(1).color(rule))
}
"#,
    );
    find(&adapter, "panel");
}

#[test]
fn style_functions_pass_by_module_path() {
    let root = std::env::temp_dir().join(format!("rawr-idioms-style-path-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("rawr.toml"), "[package]\nname = \"styled\"\n").unwrap();
    std::fs::write(
        root.join("src/theme.rawr"),
        r#"
use iced::Theme;
use iced::widget::container::Style;
pub fn rule(theme: &Theme) -> Style { Style::default().background(theme.palette().primary.base.color) }
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rawr"),
        r#"
mod theme;
use iced::Element;
use iced::widget::{container, text};
struct State {}
#[derive(Debug, Clone)] enum Message {}
impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { container(text("ruled")).style(theme::rule) }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#,
    )
    .unwrap();
    let target = rawr::project::Target::discover(&root).unwrap();
    let checked = rawr_iced::application::Compiler::new().unwrap().check(&target);
    std::fs::remove_dir_all(&root).ok();
    let adapter = checked
        .unwrap_or_else(|error| panic!("{error}"))
        .prepare()
        .unwrap()
        .into_adapter();
    find(&adapter, "ruled");
}

#[test]
fn scripted_widgets_sit_inside_column() {
    let adapter = boot_with(
        r#"column![Dial {}, text("beside")]"#,
        r#"
use rawr_iced::widget::event::{publish, Input, Outcome};
#[derive(Default)]
struct Clicks { count: i64 }
struct Dial;
#[widget]
impl Dial {
    fn state(&self) -> Clicks { Clicks { count: 0 } }
    fn update(&mut self, state: &mut Clicks, input: Input) -> Outcome<Message> {
        publish(Message::Pressed)
    }
}
"#,
    );
    find(&adapter, "beside");
}

#[test]
fn button_style_functions_match_status_and_extend_a_builtin() {
    let adapter = boot_with(
        r#"button("square").on_press(Message::Pressed).style(square)"#,
        r#"
use iced::widget::button::{Status, Style};
fn square(theme: &Theme, status: Status) -> Style {
    let palette = theme.palette();
    let background = match status {
        Status::Hovered | Status::Pressed => palette.primary.strong.color,
        Status::Active => palette.primary.base.color,
        Status::Disabled => palette.background.weak.color,
    };
    Style {
        background,
        border: border::rounded(0).width(1).color(palette.background.strong.color),
        ..button::primary(theme, status)
    }
}
"#,
    );
    styled(&adapter, "square");

    // Every status is evaluated while lowering: a failing arm fails the button.
    let failing = boot_with(
        r#"button("square").style(broken)"#,
        r#"
use iced::widget::button::{Status, Style};
fn broken(theme: &Theme, status: Status) -> Style {
    match status {
        Status::Disabled => Style { border: Border::default().width(0 - 1), ..Style::default() },
        _ => button::primary(theme, status),
    }
}
"#,
    );
    let mut simulator = iced_test::simulator(failing.view());
    assert!(simulator.find("square").is_err());
}

#[test]
fn checkbox_style_functions_match_variant_fields() {
    let adapter = boot_with(
        r#"column![text("tick"), checkbox(true).on_toggle(Message::Toggled).style(tick)]"#,
        r#"
use iced::widget::checkbox::{Status, Style};
fn tick(theme: &Theme, status: Status) -> Style {
    let palette = theme.palette();
    let checked = match status {
        Status::Active { is_checked } | Status::Disabled { is_checked } => is_checked,
        Status::Hovered { is_checked: true } => true,
        Status::Hovered { .. } => false,
    };
    Style {
        background: if checked { palette.primary.base.color } else { palette.background.base.color },
        border: border::rounded(0).width(1).color(palette.primary.strong.color),
        ..checkbox::primary(theme, status)
    }
}
"#,
    );
    styled(&adapter, "tick");
}

#[test]
fn toggler_style_functions_read_variant_fields() {
    let adapter = boot_with(
        r#"column![text("switch"), toggler(true).on_toggle(Message::Toggled).style(pill)]"#,
        r#"
use iced::widget::toggler::{Status, Style};
fn pill(theme: &Theme, status: Status) -> Style {
    let palette = theme.palette();
    let base = toggler::default(theme, status);
    match status {
        Status::Active { is_toggled: true } | Status::Hovered { is_toggled: true } => Style {
            background: palette.success.base.color,
            border_radius: 0,
            ..base
        },
        _ => Style { border_radius: 0, ..base },
    }
}
"#,
    );
    styled(&adapter, "switch");
}

#[test]
fn text_input_style_functions_match_focus() {
    let adapter = boot_with(
        r#"text_input("name", "typed").on_input(Message::Edited).style(field)"#,
        r#"
use iced::widget::text_input::{Status, Style};
fn field(theme: &Theme, status: Status) -> Style {
    let palette = theme.palette();
    let rule = match status {
        Status::Focused { is_hovered } if is_hovered => palette.primary.strong.color,
        Status::Focused { .. } => palette.primary.base.color,
        Status::Active | Status::Hovered | Status::Disabled => palette.background.strong.color,
    };
    Style {
        border: border::rounded(0).width(1).color(rule),
        ..text_input::default(theme, status)
    }
}
"#,
    );
    styled(&adapter, "typed");
}

#[test]
fn slider_style_closures_extend_nested_styles() {
    let adapter = boot(
        r#"
        column![
            text("volume"),
            slider(0..=10, 5, Message::Moved).style(|theme, status| {
                let palette = theme.palette();
                let base = slider::default(theme, status);
                slider::Style {
                    rail: slider::Rail { border: border::rounded(0), width: 2, ..base.rail },
                    handle: slider::Handle { background: palette.primary.base.color, ..base.handle },
                }
            }),
        ]
        "#,
    );
    styled(&adapter, "volume");
}

#[test]
fn scrollable_style_functions_style_rails_and_scrollers() {
    let adapter = boot_with(
        r#"scrollable(column![text("scrolled")]).style(rails)"#,
        r#"
use iced::widget::scrollable::{Rail, Scroller, Status, Style};
fn rails(theme: &Theme, status: Status) -> Style {
    let palette = theme.palette();
    let base = scrollable::default(theme, status);
    let scroller = match status {
        Status::Dragged { is_vertical_scrollbar_dragged: true, .. } => palette.primary.strong.color,
        _ => palette.primary.base.color,
    };
    let rail = Rail {
        border: border::rounded(0),
        scroller: Scroller { background: scroller, border: border::rounded(0) },
        ..base.vertical_rail
    };
    Style { vertical_rail: rail, horizontal_rail: rail, ..base }
}
"#,
    );
    styled(&adapter, "scrolled");
}

#[test]
fn widget_modules_name_their_style_types() {
    let adapter = boot_with(
        r#"column![text("short"), slider(0..=10, 5, Message::Moved).style(rail), button("b").style(key)]"#,
        r#"
fn key(theme: &Theme, status: button::Status) -> button::Style {
    let base: button::Style = button::primary(theme, status);
    button::Style { border: border::rounded(0), ..base }
}
fn rail(theme: &Theme, status: slider::Status) -> slider::Style {
    let base: iced::widget::slider::Style = iced::widget::slider::default(theme, status);
    slider::Style { rail: slider::Rail { width: 2, ..base.rail }, ..base }
}
"#,
    );
    styled(&adapter, "short");
}

#[test]
fn style_functions_chosen_by_a_value() {
    let adapter = boot_with(
        r#"column![chosen(true), chosen(false)]"#,
        r#"
use iced::widget::container::Style;
fn panel(theme: &Theme) -> Style { Style::default().background(theme.palette().primary.base.color) }
fn cell(theme: &Theme) -> Style { Style::default() }
fn chosen(dark: bool) -> Element<'static, Message> {
    let f = if dark { panel } else { cell };
    container(text(if dark { "chosen" } else { "plain" })).style(f)
}
"#,
    );
    styled(&adapter, "chosen");
}

#[test]
fn padding_takes_vertical_and_horizontal_arrays() {
    let adapter = boot(
        r#"
        column![
            container(text("pair")).padding([8, 16]),
            row![text("floats")].padding([2.5, 4.0]),
        ]
        "#,
    );
    let pair = bounds(&adapter, "pair");
    assert_eq!((pair.x, pair.y), (16.0, 8.0));
    find(&adapter, "floats");
}

#[test]
fn container_styles_set_text_color_shadow_and_snap_and_read_radius_corners() {
    let adapter = boot_with(
        r#"container(text("shaded")).style(panel)"#,
        r#"
use iced::Shadow;
use iced::widget::container::Style;
fn panel(theme: &Theme) -> Style {
    let edge = border::rounded(4);
    let corner = edge.radius.top_left + edge.radius.bottom_right;
    let base = Style::default().border(edge);
    Style {
        text_color: Color::WHITE,
        shadow: Shadow { blur_radius: corner, ..base.shadow },
        snap: !base.snap,
        ..base
    }
}
"#,
    );
    styled(&adapter, "shaded");
}

#[test]
fn maybe_builders_take_an_option_message() {
    let adapter = boot(
        r#"
        column![
            button("maybe").on_press_maybe(Some(Message::Pressed)),
            button("never").on_press_maybe(None),
            text_input("name", "typed").on_input(Message::Edited).on_submit(Message::Pressed),
            text_input("other", "kept").on_submit_maybe(if true { Some(Message::Pressed) } else { None }),
        ]
        "#,
    );
    find(&adapter, "maybe");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("maybe").unwrap();
    assert_eq!(simulator.into_messages().count(), 1);
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("never").unwrap();
    assert_eq!(simulator.into_messages().count(), 0);
}

#[test]
fn optional_style_fields_take_and_read_options() {
    let adapter = boot_with(
        r#"column![button("rich").style(rich), container(text("plain")).style(plain)]"#,
        r#"
use iced::Background;
fn rich(theme: &Theme, status: button::Status) -> button::Style {
    let base = button::primary(theme, status);
    let background = if base.background.is_some() { Some(Background::Color(Color::WHITE)) } else { None };
    button::Style { background, text_color: Color::BLACK, ..base }
}
fn plain(theme: &Theme) -> container::Style {
    let base = container::Style::default();
    let text_color: Option<Color> = base.text_color;
    container::Style { text_color: text_color.or(Some(Color::WHITE)), ..base }
}
"#,
    );
    styled(&adapter, "rich");
    styled(&adapter, "plain");
}

#[test]
fn style_fields_take_colors_into_backgrounds_as_in_rust() {
    let adapter = boot_with(
        r#"column![button("keyed").style(keyed), toggler(true).style(lever), text("lever")]"#,
        r#"
fn keyed(theme: &Theme, status: button::Status) -> button::Style {
    let fill = theme.palette().primary.base;
    button::Style { background: Some(fill.color.into()), ..button::primary(theme, status) }
}
fn lever(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let palette = theme.palette();
    toggler::Style {
        background: palette.primary.base.color.into(),
        foreground: palette.primary.base.text.into(),
        border_radius: Some(0.into()),
        ..toggler::default(theme, status)
    }
}
"#,
    );
    styled(&adapter, "keyed");
    styled(&adapter, "lever");
}

#[test]
fn maybe_mapper_builders_take_an_optional_closure() {
    let adapter = boot(
        r#"
        column![
            checkbox(false).label("live").on_toggle_maybe(Some(Message::Toggled)),
            checkbox(false).label("frozen").on_toggle_maybe(None),
            checkbox(false).label("flip").on_toggle_maybe(Some(|on: bool| Message::Toggled(!on))),
            toggler(false).on_toggle_maybe(Some(Message::Toggled)),
            toggler(true).on_toggle_maybe(None),
            text_input("name", "typed").on_input_maybe(Some(Message::Edited)),
            text_input("locked", "fixed").on_input_maybe(None),
        ]
        "#,
    );
    let messages = |label: &str| {
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.click(label).unwrap();
        simulator.into_messages().count()
    };
    assert_eq!(messages("live"), 1);
    assert_eq!(messages("frozen"), 0);
    assert_eq!(messages("flip"), 1);
    assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
}

#[test]
fn pick_list_labels_options_through_to_string() {
    let adapter = boot_with(
        r#"
        column![
            "Which one?",
            pick_list(Some(Choice::B), Choice::ALL, Choice::to_string)
                .on_select(|_choice: Choice| Message::Pressed)
                .placeholder("Pick...")
                .width(200),
        ]
        "#,
        r#"
use iced::widget::pick_list;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Choice { #[default] A, B }
impl Choice { const ALL: &[Choice] = &[Choice::A, Choice::B]; }
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self { Choice::A => "Alpha", Choice::B => "Beta" })
    }
}
"#,
    );
    // A string literal child is its `text`; iced's pick list exposes no
    // text to the simulator, so `rawr-play` checks its label renders.
    find(&adapter, "Which one?");
}

#[test]
fn sliders_take_typed_messages_defaults_and_the_iced_logo() {
    let adapter = boot_with(
        r#"
        let value: u8 = 50;
        column![
            slider(1..=100, value, Message::Small).default(50).shift_step(5),
            iced::widget::vertical_slider(1..=100, value, Message::Small).default(50),
            iced::widget::iced(20),
            text(value),
        ]
        "#,
        // A function taking a `u8` where the facade's slider gives `i64`.
        "impl Message { #[allow(non_snake_case)] fn Small(value: u8) -> Message { Message::Moved(value as i64) } }",
    );
    find(&adapter, "50");
}

#[test]
fn host_values_compare_with_eq_as_in_rust() {
    let adapter = boot(
        r#"
        let red = Color::from_rgb8(255, 0, 0);
        let same = red == Color::from_rgb(1.0, 0.0, 0.0);
        let other = red != Color::BLACK;
        let themed = Theme::Dark == Theme::Dark && Theme::Dark != Theme::Light;
        text(format!("eq {same} {other} {themed}"))
        "#,
    );
    find(&adapter, "eq true true true");
}

#[test]
fn text_helpers_take_iced_into_fragment_bounds() {
    let adapter = boot_with(
        "labels()",
        r#"
use iced::widget::Text;
fn caption<'a, T: text::IntoFragment<'a>>(content: T) -> Text<'a> {
    text(content).size(11)
}
fn labels() -> Element<'static, Message> {
    let owned = String::from("owned");
    column![caption("literal"), caption(format!("formatted {}", 2)), caption(&owned), caption(7), caption(2.5), caption('c')]
}
"#,
    );
    for label in ["literal", "formatted 2", "owned", "7", "2.5", "c"] {
        find(&adapter, label);
    }
    // A type without iced's impl is rejected, as in Rust.
    let source = r#"
use iced::widget::{text, Text};
struct Plain;
fn caption<'a, T: text::IntoFragment<'a>>(content: T) -> Text<'a> { text(content) }
fn shown() -> Text<'static> { caption(Plain) }
struct State {}
#[derive(Debug, Clone)] enum Message {}
impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> iced::Element<'_, Message> { shown() }
}
pub fn main() -> iced::Result { iced::application(State::new, State::update, State::view).run() }
"#;
    let check = |source: &str| {
        rawr_iced::application::Compiler::new()
            .unwrap()
            .check_source(source)
            .map(|_| ())
            .map_err(|error| error.to_string())
    };
    check(&source.replace("caption(Plain)", "caption(\"plain\")")).unwrap();
    let error = check(source).unwrap_err();
    assert!(error.contains("IntoFragment"), "{error}");
}

#[test]
fn facade_leniencies_rust_rejects_are_rejected() {
    let check = |items: &str, view: &str| {
        let source = format!(
            r#"
use iced::{{Color, Element, Theme}};
use iced::theme::palette::Seed;
use iced::widget::{{column, text}};
struct State {{ size: i64, gap: f64 }}
#[derive(Debug, Clone)] enum Message {{}}
impl State {{
    fn new() -> State {{ State {{ size: 12, gap: 2.0 }} }}
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{ {view} }}
}}
{items}
pub fn main() -> iced::Result {{ iced::application(State::new, State::update, State::view).run() }}
"#
        );
        rawr_iced::application::Compiler::new()
            .unwrap()
            .check_source(&source)
            .map(|_| ())
            .map_err(|error| error.to_string())
    };
    let seed = |fields: &str| {
        format!("fn theme() -> Theme {{ let c = Color::BLACK; Theme::custom(\"t\", Seed {{ {fields} }}) }}")
    };
    let all = "background: c, text: c, primary: c, success: c, warning: c, danger: c";
    let fine = r#"text("x")"#;
    check(&seed(all), fine).unwrap();
    let error = check(&seed("background: c, text: c"), fine).unwrap_err();
    assert!(error.contains("missing fields") && error.contains("`primary`"), "{error}");
    let error = check("fn seed() -> Seed { Seed::default() }", fine).unwrap_err();
    assert!(error.contains("Default"), "{error}");

    // Literals and typed `f32`/`u32` pass; a typed `i64` or `f64` is E0308.
    check("", r#"let n: u32 = 3; let w: f32 = 1.5; column![text("x").size(11), text("y").size(n)].spacing(w).padding(4.5)"#).unwrap();
    let error = check("", r#"text("x").size(self.size)"#).unwrap_err();
    assert!(error.contains("i64") && error.contains("E0308"), "{error}");
    let error = check("", r#"column![text("x")].spacing(self.gap)"#).unwrap_err();
    assert!(error.contains("f64") && error.contains("E0308"), "{error}");
}

#[test]
fn container_styles_read_their_border() {
    let adapter = boot_with(
        r#"container(text("parsed")).style(panel)"#,
        r#"
use iced::widget::container::Style;
fn panel(theme: &Theme) -> Style {
    let base = Style::default().border(border::rounded(4));
    let corner = base.border.radius.top_left;
    Style { shadow: iced::Shadow { blur_radius: corner, ..base.shadow }, ..base }
}
"#,
    );
    styled(&adapter, "parsed");
}

#[test]
fn radios_select_by_value_and_take_status_styles() {
    let adapter = boot_with(
        r#"
        let selected = Some(Choice::B);
        column![
            text("choices"),
            radio("Alpha", Choice::A, selected, |_choice: Choice| Message::Pressed),
            radio("Beta", Choice::B, selected, |_choice| Message::Pressed)
                .size(14)
                .spacing(8)
                .text_size(13)
                .width(Fill)
                .style(ring),
            radio("Gamma", Choice::C, None, |_choice| Message::Pressed).style(radio::default),
        ]
        "#,
        r#"
use iced::widget::radio;
use iced::widget::radio::{Status, Style};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice { A, B, C }
fn ring(theme: &Theme, status: Status) -> Style {
    let palette = theme.palette();
    match status {
        Status::Active { is_selected: true } | Status::Hovered { is_selected: true } => Style {
            dot_color: palette.primary.strong.color,
            border_width: 2.0,
            ..radio::default(theme, status)
        },
        _ => Style { text_color: Some(palette.background.strong.text), ..radio::default(theme, status) },
    }
}
"#,
    );
    styled(&adapter, "choices");
    // iced's radio exposes no text to the simulator: click under the title.
    let below = bounds(&adapter, "choices").height + 8.0;
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.point_at(iced::Point::new(8.0, below));
    let _ = simulator.simulate(iced_test::simulator::click());
    assert_eq!(simulator.into_messages().count(), 1);
}

#[test]
fn slider_handles_take_iced_handle_shapes() {
    let adapter = boot_with(
        r#"column![text("faders"), slider(0..=100, 50, Message::Moved).style(square)]"#,
        r#"
use iced::widget::slider::{Handle, HandleShape, Status, Style};
fn square(theme: &Theme, status: Status) -> Style {
    let base = slider::default(theme, status);
    let shape = match status {
        Status::Dragged => HandleShape::Circle { radius: 8.0 },
        _ => HandleShape::Rectangle { width: 6, border_radius: 0.into() },
    };
    Style { handle: Handle { shape, ..base.handle }, ..base }
}
"#,
    );
    styled(&adapter, "faders");
}

#[test]
fn closures_that_capture_pass_to_host_callables_with_their_values() {
    let adapter = boot(
        r#"
        let offset = 40;
        let tint = Color::from_rgb(0.9, 0.2, 0.2);
        column![
            checkbox(false).label("shift").on_toggle(move |on| if on && offset == 40 { Message::Moved(offset) } else { Message::Pressed }),
            button("tinted").style(move |theme, status| button::Style { text_color: tint, ..button::primary(theme, status) }),
        ]
        "#,
    );
    styled(&adapter, "tinted");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("shift").unwrap();
    let messages = simulator.into_messages().collect::<Vec<_>>();
    assert_eq!(messages.len(), 1);
    let payload = format!("{:?}", messages[0].payload());
    // The capture comes before the closure's own argument.
    assert!(payload.contains("Moved"), "{payload}");
}

#[test]
fn facade_unit_enums_match_by_value_and_derive_default() {
    let adapter = boot_with(
        r#"
        column![
            text(name(Position::Left)),
            text(name(Holder::default().position)),
            iced::widget::tooltip(button("hover"), "tip", Position::Bottom).gap(4),
        ]
        .align_x(iced::alignment::Horizontal::Center)
        "#,
        r#"
use iced::widget::tooltip::Position;
#[derive(Default)]
struct Holder { position: Position, padding: Padding }
fn name<'a>(position: Position) -> &'a str {
    match position {
        Position::Top => "top",
        Position::Bottom => "bottom",
        Position::Left => "left",
        Position::Right => "right",
        Position::FollowCursor => "follow",
    }
}
"#,
    );
    find(&adapter, "left");
    find(&adapter, "top");
    find(&adapter, "hover");
}

#[test]
fn trait_objects_and_mut_trait_parameters_change_state_in_update() {
    let source = r#"
use iced::Element;
use iced::widget::{button, column, text};
trait Counter { fn bump(&mut self, by: &mut i64); fn label(&self) -> String; }
#[derive(Debug, Clone, Default)]
struct Plain { n: i64 }
impl Counter for Plain {
    fn bump(&mut self, by: &mut i64) { self.n += *by; *by += 1; }
    fn label(&self) -> String { format!("plain {}", self.n) }
}
#[derive(Default)]
struct State { items: Vec<Box<dyn Counter>>, step: i64 }
#[derive(Debug, Clone)] enum Message { Bump }
impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Bump => {
                if self.items.is_empty() {
                    self.items.push(Box::new(Plain::default()));
                    self.items.push(Box::new(Plain { n: 10 }));
                    self.step = 1;
                }
                let mut step = self.step;
                for c in self.items.iter_mut() { c.bump(&mut step); }
                self.items[0].bump(&mut step);
                self.step = step;
            }
        }
    }
    fn view(&self) -> Element<'_, Message> {
        column![
            text(format!("{} step {}", self.items.iter().map(|c| c.label()).collect::<Vec<_>>().join(", "), self.step)),
            button("Bump").on_press(Message::Bump),
        ]
    }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#;
    let mut adapter = rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(source)
        .unwrap_or_else(|error| panic!("{error}"))
        .prepare()
        .unwrap()
        .into_adapter();
    for _ in 0..2 {
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.click("Bump").unwrap();
        let message = simulator.into_messages().next().unwrap();
        adapter.update(message).unwrap();
    }
    find(&adapter, "plain 14, plain 17 step 7");
}

#[test]
fn sliders_checkboxes_and_togglers_take_iced_sizing_builders() {
    let adapter = boot(
        r#"
        column![
            slider(0..=10, 5, Message::Moved).width(200).height(30),
            slider(0.0..=1.0, 0.5, |v: f64| Message::Moved(v as i64)).width(Fill),
            iced::widget::vertical_slider(0..=10, 5, Message::Moved).width(30).height(120),
            checkbox(true).label("sized").size(20).width(Fill).spacing(10),
            toggler(true).label("wide").width(Fill).spacing(8),
        ]
        "#,
    );
    find(&adapter, "sized");
}

#[test]
fn crate_conversions_into_facade_types_apply_where_rust_applies_them() {
    let adapter = boot_with(
        r#"column![
            text("shade").color(Shade::Light),
            text("from").color(Color::from(Shade::Dark)),
            text("into").color({ let color: Color = Shade::Light.into(); color }),
            text(format!("brightness {}", Color::BLACK.brightness())),
        ]"#,
        r#"
trait Brightness { fn brightness(&self) -> f32; }
impl Brightness for iced::Color {
    fn brightness(&self) -> f32 { 0.5 }
}
#[derive(Debug, Clone, Copy)]
enum Shade { Dark, Light }
impl From<Shade> for iced::Color {
    fn from(shade: Shade) -> Self {
        match shade {
            Shade::Dark => Color::from_rgb8(0, 0, 0),
            Shade::Light => Color::WHITE,
        }
    }
}
"#,
    );
    find(&adapter, "shade");
    find(&adapter, "into");
    find(&adapter, "brightness 0.5");
}

#[test]
fn a_std_trait_for_a_facade_type_is_rejected_as_rustc_does() {
    let source = r#"
use iced::Widget;
use iced::widget::text;
impl Default for iced::Color {
    fn default() -> Self { iced::Color::BLACK }
}
struct State {}
#[derive(Debug, Clone)] enum Message {}
impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> impl Widget<Message> { text("a") }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#;
    let Err(error) = rawr_iced::application::Compiler::new().unwrap().check_source(source) else {
        panic!("Rust rejects `impl Default for iced::Color` (E0117)");
    };
    assert!(error.to_string().contains("E0117"), "{error}");
}
