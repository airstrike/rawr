//! iced master's own examples, as written there, check and run in Rawr:
//! `impl Widget<Message>` views, `.boxed()` where arms or values erase,
//! `.map(..)` on any widget, and no `.into()` into `Element`.

use rawr_iced::{program, runtime};

fn boot(source: &str) -> program::Adapter {
    rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(source)
        .unwrap_or_else(|error| panic!("{error}"))
        .prepare()
        .unwrap()
        .into_adapter()
}

fn rejected(source: &str) -> String {
    match rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(source)
    {
        Ok(_) => panic!("expected the source to be rejected"),
        Err(error) => error.to_string(),
    }
}

fn find(adapter: &program::Adapter, label: &str) {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.find(label).unwrap();
    assert!(
        adapter.diagnostics().is_empty(),
        "{:?}",
        adapter.diagnostics()
    );
}

fn click(adapter: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(label).unwrap();
    simulator.into_messages().next().expect("the button emits")
}

/// `examples/counter/src/main.rs`, without its `#[cfg(test)]` module.
const COUNTER: &str = r#"
use iced::Center;
use iced::widget::{Widget, button, column, text};

pub fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.value += 1;
            }
            Message::Decrement => {
                self.value -= 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            button("Decrement").on_press(Message::Decrement)
        ]
        .padding(20)
        .align_x(Center)
    }
}
"#;

#[test]
fn master_counter_runs_as_written() {
    let mut adapter = boot(COUNTER);
    find(&adapter, "0");
    for _ in 0..2 {
        let increment = click(&adapter, "Increment");
        adapter.update(increment).unwrap();
    }
    let decrement = click(&adapter, "Decrement");
    adapter.update(decrement).unwrap();
    find(&adapter, "1");
}

/// A slice of `examples/layout/src/main.rs`: its example table of
/// `fn() -> Element` views, each ending in `.boxed()`, string children, and
/// `quotes` with `impl Widget<Message> + 'a` parameters taking strings.
/// iced's `space::horizontal()` is `space().width(Fill)`, `rule` a thin
/// container, `quote` and `reply` are top-level (Rawr has no nested `fn`
/// items), `square` a styled container of a `u16` size rather than a
/// canvas, and the table a `Vec` of `Clone` (not `Copy`) `Example`s that
/// the state indexes, since a `fn` has no runtime State representation.
const LAYOUT: &str = r#"
use iced::widget::{button, center, column, container, row, space, text};
use iced::{Center, Element, Fill, Shrink, Widget};

pub fn main() -> iced::Result {
    iced::application(Layout::default, Layout::update, Layout::view).run()
}

#[derive(Default)]
struct Layout {
    example: usize,
}

#[derive(Debug, Clone)]
enum Message {
    Next,
}

impl Layout {
    fn update(&mut self, message: Message) {
        match message {
            Message::Next => {
                self.example = (self.example + 1).min(Example::list().len() - 1);
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let current = Example::list()[self.example].clone();
        let header = row![text(current.title).size(20).font(iced::Font::MONOSPACE)]
            .spacing(20)
            .align_y(Center);

        let example = center(current.view()).padding(4);

        let controls = row![
            space().width(Fill),
            button(text("Next →")).padding([5, 10]).on_press(Message::Next),
        ];

        column![header, example, controls].spacing(10).padding(20)
    }
}

#[derive(Clone)]
struct Example {
    title: &'static str,
    view: fn() -> Element<'static, Message>,
}

impl Example {
    fn list() -> Vec<Example> {
        vec![
            Example { title: "Centered", view: centered },
            Example { title: "Column", view: column_ },
            Example { title: "Row", view: row_ },
            Example { title: "Space", view: space_ },
            Example { title: "Quotes", view: quotes },
        ]
    }

    fn view(&self) -> impl Widget<Message> {
        (self.view)()
    }
}

fn centered<'a>() -> Element<'a, Message> {
    center(text("I am centered!").size(50)).boxed()
}

fn column_<'a>() -> Element<'a, Message> {
    column![
        "A column can be used to",
        "lay out widgets vertically.",
        square(50),
        square(50),
        square(50),
        "The amount of space between",
        "elements can be configured!",
    ]
    .spacing(40)
    .boxed()
}

fn row_<'a>() -> Element<'a, Message> {
    row![
        "A row works like a column...",
        square(50),
        square(50),
        square(50),
        "but lays out widgets horizontally!",
    ]
    .spacing(40)
    .boxed()
}

fn space_<'a>() -> Element<'a, Message> {
    row!["Left!", space().width(Fill), "Right!"].boxed()
}

fn quote<'a>(content: impl Widget<Message> + 'a) -> impl Widget<Message> + 'a {
    row![container(space()).width(1).height(Fill), content].spacing(10).height(Shrink)
}

fn reply<'a>(
    original: impl Widget<Message> + 'a,
    reply: impl Widget<Message> + 'a,
) -> impl Widget<Message> {
    column![quote(original), reply].spacing(10)
}

fn quotes<'a>() -> Element<'a, Message> {
    column![
        reply(
            reply("This is the original message", "This is a reply"),
            "This is another reply",
        ),
        container(space()).width(Fill).height(1),
        text("A separator ↑"),
    ]
    .width(Shrink)
    .spacing(10)
    .boxed()
}

fn square(size: u16) -> impl Widget<Message> {
    container("").width(size).height(size).style(container::rounded_box)
}
"#;

#[test]
fn master_layout_examples_box_and_take_strings_as_widgets() {
    let mut adapter = boot(LAYOUT);
    find(&adapter, "I am centered!");
    for expected in [
        "lay out widgets vertically.",
        "but lays out widgets horizontally!",
        "Right!",
        "This is a reply",
    ] {
        let next = click(&adapter, "Next →");
        adapter.update(next).unwrap();
        find(&adapter, expected);
    }
}

/// `examples/tour/src/main.rs`'s screens named by their content type:
/// `Column<Element<'_, Message>>`, `Button<'_, Message, &str>`, match arms
/// erased with `.boxed()`, and `center_x(if .. { a.boxed() } else { .. })`.
/// The welcome screen's `rich_text!` paragraph is a plain string here, and
/// `padded_button` takes the application's Message rather than any, and
/// `view` builds its screen without a block expression.
const TOUR: &str = r#"
use iced::widget::{Button, Column};
use iced::widget::{button, center_x, center_y, column, radio, row, scrollable, slider, space, text};
use iced::{Center, Element, Fill, Widget};

pub fn main() -> iced::Result {
    iced::application(Tour::default, Tour::update, Tour::view)
        .title(Tour::title)
        .centered()
        .run()
}

#[derive(Default)]
pub struct Tour {
    screen: Screen,
    slider: u8,
    layout: Layout,
    spacing: u32,
    debug: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    BackPressed,
    NextPressed,
    SliderChanged(u8),
    LayoutChanged(Layout),
}

impl Tour {
    fn title(&self) -> String {
        format!("{} - Iced", self.screen.title())
    }

    fn update(&mut self, event: Message) {
        match event {
            Message::BackPressed => {
                if let Some(screen) = self.screen.previous() {
                    self.screen = screen;
                }
            }
            Message::NextPressed => {
                if let Some(screen) = self.screen.next() {
                    self.screen = screen;
                }
            }
            Message::SliderChanged(value) => {
                self.slider = value;
            }
            Message::LayoutChanged(layout) => {
                self.layout = layout;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let back = padded_button("Back").on_press_maybe(self.screen.previous().map(|_| Message::BackPressed));
        let next = padded_button("Next").on_press_maybe(self.screen.next().map(|_| Message::NextPressed));
        let controls = row![back, space().width(Fill), next];

        let content = match self.screen {
            Screen::Welcome => self.welcome(),
            Screen::Slider => self.slider(),
            Screen::RowsAndColumns => self.rows_and_columns(),
        };
        let screen = column![text(self.screen.title()).size(50), content.spacing(20)].spacing(20);

        let content = column![screen, controls].width(540).spacing(20);

        let scrollable = scrollable(center_x(if self.debug {
            content.boxed()
        } else {
            content.boxed()
        }));

        center_y(scrollable).padding(10)
    }

    fn welcome(&self) -> Column<Element<'_, Message>> {
        column![
            "This is a simple tour meant to showcase a bunch of \
        widgets that come bundled in Iced.",
            "Iced is a cross-platform GUI library for Rust focused on \
         simplicity and type-safety. It is heavily inspired by Elm.",
            "It was originally born as part of Coffee, an opinionated \
         2D game engine for Rust.",
            "On native platforms, Iced provides by default a renderer \
         built on top of wgpu, a graphics library supporting Vulkan, \
         Metal, DX11, and DX12.",
            "Additionally, this tour can also run on WebAssembly by leveraging trunk.",
            "You will need to interact with the UI in order to reach \
         the end!"
        ]
    }

    fn slider(&self) -> Column<Element<'_, Message>> {
        column![
            "A slider allows you to smoothly select a value from a range \
                of values.",
            "The following slider lets you choose an integer from \
                0 to 100:",
            slider(0..=100, self.slider, Message::SliderChanged),
            text(self.slider.to_string()).width(Fill).align_x(Center),
        ]
    }

    fn rows_and_columns(&self) -> Column<Element<'_, Message>> {
        let row_radio = radio(
            "Row",
            Layout::Row,
            Some(self.layout),
            Message::LayoutChanged,
        );

        let column_radio = radio(
            "Column",
            Layout::Column,
            Some(self.layout),
            Message::LayoutChanged,
        );

        let layout_section = match self.layout {
            Layout::Row => row![row_radio, column_radio].spacing(self.spacing).boxed(),
            Layout::Column => column![row_radio, column_radio]
                .spacing(self.spacing)
                .boxed(),
        };

        column![
            "Iced uses a layout model based on flexbox to position UI \
                elements.",
            "Rows and columns can be used to distribute content \
                horizontally or vertically, respectively.",
            layout_section,
        ]
    }
}

fn padded_button(label: &str) -> Button<'_, Message, &str> {
    button(label).padding([12, 24])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Screen {
    #[default]
    Welcome,
    Slider,
    RowsAndColumns,
}

impl Screen {
    const ALL: &'static [Self] = &[Self::Welcome, Self::Slider, Self::RowsAndColumns];

    pub fn next(self) -> Option<Screen> {
        Self::ALL
            .get(self.to_index().checked_add(1)?)
            .copied()
    }

    pub fn previous(self) -> Option<Screen> {
        Self::ALL
            .get(self.to_index().checked_sub(1)?)
            .copied()
    }

    fn to_index(self) -> usize {
        Self::ALL.iter().position(|&screen| screen == self).unwrap()
    }

    fn title(self) -> &'static str {
        match self {
            Screen::Welcome => "Welcome",
            Screen::Slider => "Slider",
            Screen::RowsAndColumns => "Rows and columns",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    Row,
    Column,
}
"#;

#[test]
fn master_tour_names_columns_by_their_content() {
    let mut adapter = boot(TOUR);
    find(&adapter, "Welcome");
    find(
        &adapter,
        "You will need to interact with the UI in order to reach the end!",
    );
    let next = click(&adapter, "Next");
    adapter.update(next).unwrap();
    find(&adapter, "Slider");
    find(
        &adapter,
        "The following slider lets you choose an integer from 0 to 100:",
    );
    let next = click(&adapter, "Next");
    adapter.update(next).unwrap();
    find(&adapter, "Rows and columns");
    find(
        &adapter,
        "Iced uses a layout model based on flexbox to position UI elements.",
    );
}

/// `examples/color_palette/src/main.rs`'s pickers: each `view` is an
/// `impl Widget<C>` of its own color space, mapped into the application's
/// Message with `.map(Message::X)` on the row it returns, and its sliders are
/// named `Slider<'a, f64, C>`. Rawr calls a facade function only at concrete
/// types, so the generic `ColorPicker<C: ColorSpace>` is one picker function
/// per space; the `palette` crate spaces are script `Rgb` and `Hsl`, the
/// slider closures name `v: f64`, and the canvas is left out.
const COLOR_PALETTE: &str = r#"
use iced::widget::{Slider, column, row, slider, text};
use iced::{Center, Widget};

pub fn main() -> iced::Result {
    iced::application(ColorPalette::default, ColorPalette::update, ColorPalette::view).run()
}

#[derive(Default)]
pub struct ColorPalette {
    base: Rgb,
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    RgbColorChanged(Rgb),
    HslColorChanged(Hsl),
}

impl ColorPalette {
    fn update(&mut self, message: Message) {
        self.base = match message {
            Message::RgbColorChanged(rgb) => rgb,
            Message::HslColorChanged(hsl) => Rgb::new(hsl.lightness, hsl.lightness, hsl.lightness),
        };
    }

    fn view(&self) -> impl Widget<Message> {
        let base = self.base;
        let hsl = Hsl::new(0.0, 0.0, base.r);

        column![
            rgb_picker(base).map(Message::RgbColorChanged),
            hsl_picker(hsl).map(Message::HslColorChanged),
        ]
        .padding(10)
        .spacing(10)
    }
}

trait ColorSpace {
    const LABEL: &'static str;

    fn new(a: f32, b: f32, c: f32) -> Self;

    fn components(&self) -> [f32; 3];

    fn to_string(&self) -> String;
}

fn rgb_picker(color: Rgb) -> impl Widget<Rgb> {
    let [c1, c2, c3] = color.components();

    row![
        text(Rgb::LABEL).width(50),
        rgb_slider(c1, move |v| Rgb::new(v, c2, c3)),
        rgb_slider(c2, move |v| Rgb::new(c1, v, c3)),
        rgb_slider(c3, move |v| Rgb::new(c1, c2, v)),
        text(color.to_string()).width(185).size(12),
    ]
    .spacing(10)
    .align_y(Center)
}

fn rgb_slider<'a>(component: f32, update: impl Fn(f32) -> Rgb) -> Slider<'a, f64, Rgb> {
    slider(0.0..=1.0, f64::from(component), move |v: f64| update(v as f32)).step(0.01)
}

fn hsl_picker(color: Hsl) -> impl Widget<Hsl> {
    let [c1, c2, c3] = color.components();

    row![
        text(Hsl::LABEL).width(50),
        slider(0.0..=360.0, f64::from(c1), move |v: f64| Hsl::new(v as f32, c2, c3)),
        slider(0.0..=1.0, f64::from(c2), move |v: f64| Hsl::new(c1, v as f32, c3)),
        slider(0.0..=1.0, f64::from(c3), move |v: f64| Hsl::new(c1, c2, v as f32)),
        text(color.to_string()).width(185).size(12),
    ]
    .spacing(10)
    .align_y(Center)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Rgb {
    r: f32,
    g: f32,
    b: f32,
}

impl ColorSpace for Rgb {
    const LABEL: &'static str = "RGB";

    fn new(r: f32, g: f32, b: f32) -> Self {
        Rgb { r, g, b }
    }

    fn components(&self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }

    fn to_string(&self) -> String {
        format!(
            "rgb({:.0}, {:.0}, {:.0})",
            255.0 * self.r,
            255.0 * self.g,
            255.0 * self.b
        )
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Hsl {
    hue: f32,
    saturation: f32,
    lightness: f32,
}

impl ColorSpace for Hsl {
    const LABEL: &'static str = "HSL";

    fn new(hue: f32, saturation: f32, lightness: f32) -> Self {
        Hsl { hue, saturation, lightness }
    }

    fn components(&self) -> [f32; 3] {
        [self.hue, self.saturation, self.lightness]
    }

    fn to_string(&self) -> String {
        format!("hsl({:.1}, {:.1}%, {:.1}%)", self.hue, 100.0 * self.saturation, 100.0 * self.lightness)
    }
}
"#;

#[test]
fn master_color_palette_maps_picker_messages() {
    let adapter = boot(COLOR_PALETTE);
    find(&adapter, "RGB");
    find(&adapter, "HSL");
    find(&adapter, "rgb(0, 0, 0)");
}

#[test]
fn a_widget_into_an_element_is_rejected_with_the_boxed_fix() {
    let error = rejected(
        r#"
use iced::widget::{button, column, text};
use iced::Element;
#[derive(Default)] struct State;
#[derive(Debug, Clone)] enum Message { Pressed }
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> {
        column![text("a"), button("b").on_press(Message::Pressed)].into()
    }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#,
    );
    assert!(error.contains("use `.boxed()`"), "{error}");
    // Other conversions keep `.into()`.
    let adapter = boot(
        r#"
use iced::widget::{container, text};
use iced::Widget;
#[derive(Default)] struct State;
#[derive(Debug, Clone)] enum Message {}
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> impl Widget<Message> {
        let label: String = "converted".into();
        container(text(label))
    }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#,
    );
    find(&adapter, "converted");
}

#[test]
fn impl_widget_takes_iceds_written_shapes() {
    let adapter = boot(
        r#"
use iced::widget::{column, container, text};
use iced::{Theme, Widget};
#[derive(Default)] struct State { shown: bool }
#[derive(Debug, Clone)] enum Message {}
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> impl iced::widget::Widget<Message> {
        let banner = match self.banner() {
            Some(banner) => banner,
            None => text("hidden").boxed(),
        };
        column![framed(banner), themed(text("themed")), generic(text("generic"))]
    }
    fn banner(&self) -> Option<impl Widget<Message>> {
        if self.shown { Some(text("shown").boxed()) } else { None }
    }
}
fn framed<'a>(content: impl Widget<Message> + 'a) -> impl Widget<Message> + 'a {
    container(content).padding(4)
}
fn themed(content: impl iced::Widget<Message, Theme, iced::Renderer>) -> impl Widget<Message, Theme> {
    container(content)
}
fn generic<W: Widget<Message>>(content: W) -> impl Widget<Message> {
    container(content)
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#,
    );
    for label in ["hidden", "themed", "generic"] {
        find(&adapter, label);
    }
    let error = rejected(
        r#"
use iced::widget::{text, Widget};
#[derive(Default)] struct State;
#[derive(Debug, Clone)] enum Message {}
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Widget<Message> { text("bare") }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#,
    );
    assert!(error.contains("impl Widget"), "{error}");
}

/// A component of iced master, as Andy wrote it: a free `update` and
/// `view` over a `bool` State booted by a closure, `Option<W>` as a widget,
/// and a script type implementing `component::Component` whose `sensor`
/// shows "hi" once its `on_show` Event has reached its `update`.
const COMPONENT: &str = include_str!("../fixtures/master/component.rs");

#[test]
fn master_component_keeps_its_state_and_consumes_its_events() {
    let mut adapter = boot(COMPONENT);
    find(&adapter, "show");
    let show = click(&adapter, "show");
    adapter.update(show).unwrap();
    // The sensor's `on_show` is the component's Event: the component's
    // `update` consumes it and publishes nothing.
    let mut simulator = iced_test::simulator(adapter.view());
    for _ in 0..3 {
        simulator.simulate([iced::Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        ))]);
    }
    simulator.find("hi").unwrap();
    assert_eq!(simulator.into_messages().count(), 0);
    assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
}
