//! iced master's small helpers as its examples write them: the `Right`/
//! `Bottom` shorthands, `right(..)`, `stack![..]`, `rule::horizontal`,
//! `color!`, `font::Weight` in a `Font` literal, `time::hours`, and
//! application builders that set window settings.

use rawr_iced::program;

fn boot(source: &str) -> program::Adapter {
    rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(source)
        .unwrap_or_else(|error| panic!("{error}"))
        .prepare()
        .unwrap()
        .into_adapter()
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

const HELPERS: &str = r#"
use iced::font;
use iced::time::{Duration, hours, minutes};
use iced::widget::{column, right, row, rule, stack, text, bottom, container};
use iced::{Bottom, Center, Color, Fill, Font, Right, Top, Widget, color};

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view)
        .title("Helpers")
        .window_size((400.0, 300.0))
        .font(Font::MONOSPACE)
        .exit_on_close_request(false)
        .centered()
        .run()
}

#[derive(Default)]
struct State {
    count: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Nothing,
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Nothing => {}
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let _hour: Duration = hours(1);
        let _minutes: Duration = minutes(30);
        let bold = Font {
            weight: font::Weight::Bold,
            ..Font::DEFAULT
        };
        let red: Color = color!(0xff0000);
        let header = text("Header").font(bold).color(red).align_x(Right);
        stack![
            column![
                header,
                rule::horizontal(1),
                row![text("Left"), text("Right")].align_y(Bottom),
                text(self.count),
                container(text("Top")).align_y(Top),
            ]
            .align_x(Center),
            right(text("Overlay")),
            bottom(text("Footer")),
        ]
        .width(Fill)
    }
}
"#;

#[test]
fn master_helpers_check_and_render() {
    let adapter = boot(HELPERS);
    find(&adapter, "Header");
    find(&adapter, "Overlay");
    find(&adapter, "Footer");
    find(&adapter, "0");
}

/// `examples/exit/src/main.rs` as written: `window::close` is a function
/// value.
const EXIT: &str = r#"
use iced::widget::{button, center, column, text};
use iced::window;
use iced::{Center, Task, Theme, Widget};

pub fn main() -> iced::Result {
    iced::run(Exit::update, Exit::view)
}

#[derive(Default)]
struct Exit {
    show_confirm: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Confirm,
    Exit,
}

impl Exit {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Confirm => window::latest().and_then(window::close),
            Message::Exit => {
                self.show_confirm = true;

                Task::none()
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let content = if self.show_confirm {
            column![
                "Are you sure you want to exit?",
                button("Yes, exit now")
                    .padding([10, 20])
                    .on_press(Message::Confirm),
            ]
        } else {
            let themes: Vec<String> = Theme::ALL.iter().map(Theme::to_string).collect();
            column![
                text(themes[0].clone()),
                "Click the button to exit",
                button("Exit").padding([10, 20]).on_press(Message::Exit),
            ]
        }
        .spacing(10)
        .align_x(Center);

        center(content).padding(20)
    }
}
"#;

#[test]
fn master_exit_checks_and_closes_through_a_window_task() {
    let mut adapter = boot(EXIT);
    find(&adapter, "Light");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("Exit").unwrap();
    let exit = simulator.into_messages().next().expect("the button emits");
    adapter.update(exit).unwrap();
    find(&adapter, "Are you sure you want to exit?");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("Yes, exit now").unwrap();
    let confirm = simulator.into_messages().next().expect("the button emits");
    adapter.update(confirm).unwrap();
}

const WRAPPERS: &str = r#"
use iced::widget::{button, column, container, hover, lazy, mouse_area, opaque, pin, responsive, stack, text};
use iced::{Fill, Size, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    version: i64,
    pressed: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Pressed,
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Pressed => {
                self.pressed += 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let list = lazy(self.version, |version: &i64| column![text("Lazy"), text(*version)]);
        let sized = responsive(|size: Size| text(if size.width > 0.0 { "Wide" } else { "Narrow" }));
        column![
            list,
            hover(text("Base"), text("Top")),
            opaque(text("Opaque")),
            container(sized).height(40),
            mouse_area(text("Area")).on_press(Message::Pressed),
            stack![container(text("Under")).width(Fill), pin(text("Pinned")).x(10).y(5)].height(40),
            text(self.pressed),
        ]
    }
}
"#;

#[test]
fn master_wrappers_lower_their_content() {
    let mut adapter = boot(WRAPPERS);
    let _ = iced_test::simulator(adapter.view());
    assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
    for label in ["Lazy", "Base", "Opaque", "Wide", "Area", "Under", "Pinned", "0"] {
        find(&adapter, label);
    }
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("Area").unwrap();
    let pressed = simulator.into_messages().next().expect("the area emits");
    adapter.update(pressed).unwrap();
    find(&adapter, "1");
}

/// `examples/qr_code/src/main.rs`, with its `RangeInclusive<f32>` constant
/// inlined and no `data.truncate(100)`: Rawr does not import
/// `std::ops::RangeInclusive` or have `String::truncate` yet, and takes
/// `Theme::to_string` as the closure `|theme| theme.to_string()`; its
/// `theme` returns a `Theme`, as a Rawr theme function does, not an
/// `Option<Theme>`.
const QR_CODE: &str = r#"
use iced::widget::{center, column, pick_list, qr_code, row, slider, text, text_input, toggler};
use iced::{Center, Theme, Widget};


pub fn main() -> iced::Result {
    iced::application(QRGenerator::default, QRGenerator::update, QRGenerator::view)
        .theme(QRGenerator::theme)
        .run()
}

#[derive(Default)]
struct QRGenerator {
    data: String,
    qr_code: Option<qr_code::Data>,
    total_size: Option<f32>,
    theme: Option<Theme>,
}

#[derive(Debug, Clone)]
enum Message {
    DataChanged(String),
    ToggleTotalSize(bool),
    TotalSizeChanged(f32),
    ThemeChanged(Theme),
}

impl QRGenerator {

    fn update(&mut self, message: Message) {
        match message {
            Message::DataChanged(data) => {

                self.qr_code = if data.is_empty() {
                    None
                } else {
                    qr_code::Data::new(&data).ok()
                };

                self.data = data;
            }
            Message::ToggleTotalSize(enabled) => {
                self.total_size = enabled.then_some(300.0);
            }
            Message::TotalSizeChanged(total_size) => {
                self.total_size = Some(total_size);
            }
            Message::ThemeChanged(theme) => {
                self.theme = Some(theme);
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let title = text("QR Code Generator").size(70);

        let input = text_input("Type the data of your QR code here...", &self.data)
            .on_input(Message::DataChanged)
            .size(30)
            .padding(15);

        let toggle_total_size = toggler(self.total_size.is_some())
            .on_toggle(Message::ToggleTotalSize)
            .label("Limit Total Size");

        let choose_theme = row![
            text("Theme:"),
            pick_list(self.theme.as_ref(), Theme::ALL, |theme: &Theme| theme.to_string())
                .on_select(Message::ThemeChanged)
                .placeholder("Theme")
        ]
        .spacing(10)
        .align_y(Center);

        let content = column![
            title,
            input,
            row![toggle_total_size, choose_theme]
                .spacing(20)
                .align_y(Center),
            self.total_size.map(|total_size| {
                slider(200.0..=400.0, total_size, Message::TotalSizeChanged)
            }),
            self.qr_code.as_ref().map(|data| {
                if let Some(total_size) = self.total_size {
                    qr_code(data).total_size(total_size)
                } else {
                    qr_code(data).cell_size(10.0)
                }
            })
        ]
        .width(700)
        .spacing(20)
        .align_x(Center);

        center(content).padding(20)
    }

    fn theme(&self) -> Theme {
        self.theme.clone().unwrap_or(Theme::Light)
    }
}
"#;

#[test]
fn master_qr_code_draws_its_data() {
    let mut adapter = boot(QR_CODE);
    find(&adapter, "QR Code Generator");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("Type the data of your QR code here...").unwrap();
    simulator.typewrite("rawr");
    for message in simulator.into_messages() {
        adapter.update(message).unwrap();
    }
    let _ = iced_test::simulator(adapter.view());
    assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
}

const MEDIA: &str = r#"
use iced::widget::{column, image, svg, text};
use iced::{ContentFit, Fill, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone, Copy)]
enum Message {
    Nothing,
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Nothing => {}
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            text("Media"),
            image("ferris.png").width(100).content_fit(ContentFit::Contain),
            image(image::Handle::from_path("ferris.png")).opacity(0.5),
            svg(svg::Handle::from_path("tiger.svg")).width(Fill).height(Fill),
        ]
    }
}
"#;

#[test]
fn master_image_and_svg_take_paths_and_handles() {
    let adapter = boot(MEDIA);
    find(&adapter, "Media");
}

const OPERATIONS: &str = r#"
use iced::widget::{button, column, operation, scrollable, text, text_input, Id};
use iced::widget::scrollable::RelativeOffset;
use iced::{Task, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    input: String,
}

#[derive(Debug, Clone)]
enum Message {
    Bottom,
    Focus,
    Next,
    Input(String),
}

impl State {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Bottom => operation::scrollable::snap_to(
                Id::new("list"),
                RelativeOffset::END,
                operation::Animation::Auto,
            ),
            Message::Focus => operation::focus(Id::new("input")),
            Message::Next => operation::focus_next(),
            Message::Input(input) => {
                self.input = input;
                Task::none()
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let offset = RelativeOffset { x: 0.0, y: 0.5 };
        column![
            button("Bottom").on_press(Message::Bottom),
            button("Focus").on_press(Message::Focus),
            button("Next").on_press(Message::Next),
            text_input("Name", &self.input).id(Id::new("input")).on_input(Message::Input),
            scrollable(column![text(offset.y), text("End")]).id(Id::new("list")),
        ]
    }
}
"#;

#[test]
fn master_operations_take_widget_ids() {
    let mut adapter = boot(OPERATIONS);
    find(&adapter, "0.5");
    for label in ["Bottom", "Focus", "Next"] {
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.click(label).unwrap();
        let message = simulator.into_messages().next().expect("the button emits");
        adapter.update(message).unwrap();
    }
}

/// `Duration`'s and `Instant`'s operators as stopwatch writes them, and
/// `.clone()` of a library value that is `Clone`.
const TIME: &str = r#"
use iced::time::{Duration, Instant};
use iced::widget::{self, column, text};
use iced::Widget;

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    total: Duration,
}

#[derive(Debug, Clone, Copy)]
enum Message {}

impl State {
    fn update(&mut self, message: Message) {
        match message {}
    }

    fn view(&self) -> impl Widget<Message> {
        let mut total = self.total;
        total += Duration::from_millis(1500) + Duration::from_millis(250) - Duration::from_millis(50);
        let twice = total * 2;
        let start = Instant::now();
        let later = start + Duration::from_secs(1);
        let gap = later - start;
        let quick = start.elapsed().as_secs() < 5;
        let id = widget::Id::new("field");
        let same = id.clone() == id;
        column![
            text(format!("{} {} {}", twice.as_millis(), twice.subsec_millis(), gap.as_secs())),
            text(format!("{quick} {same}")),
        ]
    }
}
"#;

#[test]
fn master_duration_and_instant_operators_compute_as_rust() {
    let adapter = boot(TIME);
    find(&adapter, "3400 400 1");
    find(&adapter, "true true");
}

/// Closure parameters take their types from the library function's other
/// arguments, as Rust infers them.
const INFERRED: &str = r#"
use iced::widget::{column, lazy, pick_list, text};
use iced::Widget;

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    version: u64,
}

#[derive(Debug, Clone)]
enum Message {
    Selected(String),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Selected(_) => self.version += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let options = vec![String::from("alpha"), String::from("beta")];
        column![
            pick_list(Some(String::from("beta")), options, |name| name.to_uppercase())
                .on_select(|name| Message::Selected(name)),
            lazy(self.version, |version| text(format!("version {}", version + 1))),
        ]
    }
}
"#;

#[test]
fn master_closure_parameters_take_the_types_of_the_other_arguments() {
    let adapter = boot(INFERRED);
    find(&adapter, "BETA");
    find(&adapter, "version 1");
}

const RICH: &str = r#"
use iced::widget::{column, rich_text, span, text};
use iced::{Color, Font, Widget};

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view).run()
}

#[derive(Default)]
struct State {
    opened: i64,
}

#[derive(Debug, Clone)]
enum Message {
    OpenTrunk,
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::OpenTrunk => self.opened += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            rich_text![
                "Run it with ",
                span("trunk")
                    .color(Color::from_rgb(0.5, 0.5, 1.0))
                    .underline(true)
                    .font(Font::MONOSPACE)
                    .link(Message::OpenTrunk),
                "."
            ]
            .on_link_click(|link| link),
            rich_text([span("plain"), span(" spans").size(12).link(Message::OpenTrunk)])
                .on_link_click(|link| link),
            text(self.opened),
        ]
    }
}
"#;

#[test]
fn master_rich_text_takes_strings_and_spans_with_links() {
    let adapter = boot(RICH);
    find(&adapter, "0");
}

const SELECTOR: &str = r#"
use iced::widget::{self, column, container, selector, text};
use iced::{Rectangle, Task, Widget};

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view).run()
}

#[derive(Default)]
struct State {
    bounds: Option<Rectangle>,
}

#[derive(Debug, Clone)]
enum Message {
    Find,
    Found(Option<selector::Target>),
}

impl State {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Find => selector::find(widget::Id::new("outer")).map(Message::Found),
            Message::Found(target) => {
                self.bounds = target.as_ref().and_then(selector::Target::visible_bounds);
                let _whole = target.map(|target| target.bounds());
                Task::none()
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            container(text("outer")).id(widget::Id::new("outer")).padding(10),
            text(format!("{:?}", self.bounds.is_some())),
        ]
    }
}
"#;

#[test]
fn master_selector_finds_a_container_by_id() {
    let adapter = boot(SELECTOR);
    find(&adapter, "outer");
    find(&adapter, "false");
}

const SVG_STYLE: &str = r#"
use iced::widget::{column, svg, text};
use iced::{Fill, Widget, color};

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view).run()
}

#[derive(Default)]
struct State {
    tinted: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Toggle(bool),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Toggle(tinted) => self.tinted = tinted,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let icon = svg("missing.svg")
            .width(Fill)
            .style(|_theme, status| svg::Style {
                color: match status {
                    svg::Status::Hovered => Some(color!(0x0000ff)),
                    svg::Status::Idle if self.tinted => Some(color!(0x00ff00)),
                    svg::Status::Idle => None,
                },
            });
        column![icon, text("tiger")]
    }
}
"#;

#[test]
fn master_svg_takes_a_style_function() {
    let adapter = boot(SVG_STYLE);
    find(&adapter, "tiger");
}

const FONT_LIST: &str = r#"
use iced::font;
use iced::widget::text;
use iced::{Task, Widget};

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}

struct State {
    families: Vec<font::Family>,
}

#[derive(Debug, Clone)]
enum Message {
    FontsListed(Vec<font::Family>),
}

impl State {
    fn new() -> (Self, Task<Message>) {
        (
            Self { families: font::Family::VARIANTS.to_vec() },
            font::list()
                .map(Result::ok)
                .and_then(Task::done)
                .map(Message::FontsListed),
        )
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::FontsListed(families) => self.families = families,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        text(format!("{} families", self.families.len()))
    }
}
"#;

#[test]
fn master_font_list_is_a_task_of_families() {
    let adapter = boot(FONT_LIST);
    find(&adapter, "5 families");
}

#[test]
fn master_derived_default_takes_facade_fields_that_are_default() {
    let adapter = boot(
        r#"
use iced::widget::text;
use iced::time::Duration;
use iced::{Color, Font, Padding, Point, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    color: Color,
    font: Font,
    padding: Padding,
    point: Point,
    wait: Duration,
}

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        text(format!("alpha {}", self.color.a))
    }
}
"#,
    );
    find(&adapter, "alpha 0");
}

/// iced master's `Row<W>` and `Column<W>` take their child type.
#[test]
fn master_rows_and_columns_take_their_child_type() {
    let adapter = boot(
        r#"
use iced::widget::{Column, Row, column, row, text};
use iced::{Element, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    count: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Add,
}

fn header() -> Row<Element<'static, Message>> {
    row![text("left"), text("right")]
}

impl State {
    fn update(&mut self, _message: Message) {}

    fn body(&self) -> Column<Element<'_, Message>> {
        let mut items: Row<Element<'static, Message>> = Row::new();
        items = items.push(text("pushed"));
        column![header(), items, text(self.count)]
    }

    fn view(&self) -> impl Widget<Message> {
        self.body()
    }
}
"#,
    );
    find(&adapter, "pushed");
    find(&adapter, "right");
}

const RICH_UNLINKED: &str = r#"
use iced::widget::{center, column, rich_text, span, text};
use iced::{Color, Element, Font};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    roars: u32,
}

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> Element<'_, Message> {
        let pink = Color::from_rgb8(0xff, 0x2e, 0x93);
        center(column![rich_text![
            span("RAW").color(pink).size(56).font(Font::MONOSPACE),
            span(format!("{} dB", 60 + self.roars * 10)).size(20),
        ], text(self.roars)])
        .boxed()
    }
}
"#;

#[test]
fn master_spans_without_links_take_the_default_link() {
    // iced's `Span<'a, Link = ()>`: no `.link(..)` leaves the default.
    let adapter = boot(RICH_UNLINKED);
    find(&adapter, "0");
}

const ROOT_SHORTHANDS: &str = r#"
use iced::widget::{column, container, row, text};
use iced::Element;

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> Element<'_, Message> {
        container(
            column![
                row![text("a"), text("b")].align_y(iced::Center),
                text("left").align_x(iced::Left).align_y(iced::Top).width(iced::Fill),
                text("right").align_x(iced::Right).align_y(iced::Bottom).height(iced::Shrink),
                text("portion").width(iced::FillPortion(2)).height(iced::Fit),
            ]
            .align_x(iced::Center),
        )
        .center_x(iced::Fill)
        .boxed()
    }
}
"#;

#[test]
fn master_root_shorthands_read_by_their_full_paths() {
    // iced master's `pub use Alignment::Center` and kin, named as
    // `iced::Center` without a `use`.
    let adapter = boot(ROOT_SHORTHANDS);
    find(&adapter, "portion");
}

const THEME_VALUE: &str = r#"
use iced::widget::{row, space, text};
use iced::{Theme, Widget};

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view)
        .theme(Theme::Dark)
        .run()
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        row![text("left"), space::horizontal(), text("right"), space::vertical()]
    }
}
"#;

#[test]
fn master_application_takes_a_theme_value_and_space_helpers() {
    // iced master's `.theme(Theme::Dark)` (a theme is the function of the
    // State that returns it), and `space::horizontal()`.
    let adapter = boot(THEME_VALUE);
    find(&adapter, "right");
}

const STD_TIME: &str = r#"
use std::time::{Duration, Instant};
use iced::widget::text;
use iced::Widget;

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        let started = Instant::now();
        let tick = Duration::from_millis(1500);
        text(format!("{:?} {}", tick, started.elapsed().as_secs() < 5))
    }
}
"#;

#[test]
fn master_std_time_is_iced_time() {
    // iced's `time` re-exports std's `Duration` and `Instant`.
    let adapter = boot(STD_TIME);
    find(&adapter, "1.5s true");
}

/// A derived `Default` runs a field's own `impl Default`, as Rust's does.
/// `iced::run` then boots the State by running that code.
#[test]
fn master_derived_default_runs_a_field_hand_written_default() {
    let adapter = boot(
        r#"
use iced::widget::text;
use iced::Widget;

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

struct Level {
    value: i32,
}

impl Default for Level {
    fn default() -> Self {
        Level { value: 5 }
    }
}

#[derive(Default)]
struct State {
    level: Level,
    count: u32,
}

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        let level: Level = Default::default();
        text(format!("level {} {} {}", self.level.value, level.value, self.count))
    }
}
"#,
    );
    find(&adapter, "level 5 5 0");
}

const TEXT_CENTER: &str = r#"
use iced::widget::{button, column, text};
use iced::{Color, Element, Fill};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> Element<'_, Message> {
        let color: Option<Color> = None;
        column![
            button(text("centered").width(Fill).center()).padding(10),
            text("tinted").color_maybe(Some(Color::BLACK)),
            text("plain").color_maybe(color),
        ]
        .boxed()
    }
}
"#;

#[test]
fn master_text_centers_and_takes_an_optional_color() {
    let adapter = boot(TEXT_CENTER);
    find(&adapter, "centered");
    find(&adapter, "plain");
}

const FILTER_METHOD: &str = r#"
use iced::widget::{checkbox, column, image, text};
use iced::Widget;

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view).run()
}

struct State {
    filter_method: image::FilterMethod,
}

impl Default for State {
    fn default() -> Self {
        Self { filter_method: image::FilterMethod::Linear }
    }
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Nearest(bool),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Nearest(nearest) => {
                self.filter_method = if nearest {
                    image::FilterMethod::Nearest
                } else {
                    image::FilterMethod::Linear
                };
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            text(format!("{:?}", self.filter_method)),
            image("ferris.png").filter_method(self.filter_method).width(100),
            checkbox(self.filter_method == image::FilterMethod::Nearest)
                .label("nearest")
                .on_toggle(Message::Nearest),
        ]
    }
}
"#;

#[test]
fn master_image_takes_a_filter_method() {
    let mut adapter = boot(FILTER_METHOD);
    find(&adapter, "Linear");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("nearest").unwrap();
    for message in simulator.into_messages() {
        adapter.update(message).unwrap();
    }
    find(&adapter, "Nearest");
}

const CHECKBOX_ICON: &str = r#"
use iced::widget::{checkbox, column, text};
use iced::{Font, Widget};

const ICON_FONT: Font = Font::new("icons");

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view).run()
}

#[derive(Default)]
struct State {
    custom: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    CustomToggled(bool),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::CustomToggled(custom) => self.custom = custom,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let icon = checkbox::Icon {
            font: ICON_FONT,
            code_point: '\u{e901}',
            size: None,
            line_height: Some(text::LineHeight::Relative(1.0)),
            shaping: text::Shaping::Basic,
        };
        column![
            text(format!("{}", icon.code_point == '\u{e901}')),
            checkbox(self.custom)
                .label("Custom")
                .on_toggle(Message::CustomToggled)
                .icon(icon),
        ]
    }
}
"#;

#[test]
fn master_checkbox_takes_a_custom_icon() {
    let mut adapter = boot(CHECKBOX_ICON);
    find(&adapter, "true");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("Custom").unwrap();
    for message in simulator.into_messages() {
        adapter.update(message).unwrap();
    }
    find(&adapter, "Custom");
}

const ANGLES: &str = r#"
use iced::widget::{column, text};
use iced::{Degrees, Radians, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone, Copy)]
enum Message {
    Turned(Degrees),
}

const SPEED: Degrees = Degrees(360.0);

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        let quarter = Radians::from(Degrees(90.0));
        let turn = (quarter + SPEED * 0.5 + Radians(0.0)) % (Radians::PI * 2.0);
        let half = turn / 2.0 - Radians::from(0.0);
        column![
            text(format!("{:?}", Radians::PI == Radians::from(Degrees(180.0)))),
            text(format!("{:?}", half == Radians(1.0))),
            text(format!("{:?}", Degrees(45.0))),
        ]
    }
}
"#;

#[test]
fn master_angles_convert_and_combine() {
    let adapter = boot(ANGLES);
    find(&adapter, "true");
    find(&adapter, "Degrees(45.0)");
}

const SCROLLABLE_DIRECTION: &str = r#"
use iced::widget::{button, column, radio, row, scrollable, space, text};
use iced::{Fill, Widget};

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view).run()
}

#[derive(Default)]
struct State {
    anchor: scrollable::Anchor,
    both: bool,
    offset: scrollable::RelativeOffset,
    last_source: Option<scrollable::Source>,
}

#[derive(Debug, Clone)]
enum Message {
    AlignmentChanged(scrollable::Anchor),
    Scrolled(scrollable::Scroll),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::AlignmentChanged(anchor) => {
                self.anchor = anchor;
                self.both = true;
            }
            Message::Scrolled(scroll) => {
                self.offset = scroll.viewport.relative_offset();
                self.last_source = Some(scroll.source);
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let scrollbar = scrollable::Scrollbar::new()
            .width(10)
            .margin(2)
            .padding(1)
            .scroller_width(8)
            .anchor(self.anchor);
        let content = scrollable(column![text("Beginning!"), space().height(1200), text("End!")])
            .direction(if self.both {
                scrollable::Direction::Both {
                    horizontal: scrollbar,
                    vertical: scrollbar,
                }
            } else {
                scrollable::Direction::Vertical(scrollbar)
            })
            .width(Fill)
            .height(200)
            .spacing(4)
            .on_scroll(Message::Scrolled)
            .auto_scroll(true);
        let source = match self.last_source {
            Some(source) => text(format!("Source: {source:?}")),
            None => text("Source: ?"),
        };
        column![
            row![
                radio("Start", scrollable::Anchor::Start, Some(self.anchor), Message::AlignmentChanged),
                radio("End", scrollable::Anchor::End, Some(self.anchor), Message::AlignmentChanged),
                button("flip").on_press(Message::AlignmentChanged(scrollable::Anchor::End)),
            ],
            content,
            source,
            text(format!("{:?} {}", self.anchor, self.offset.y)),
        ]
    }
}
"#;

#[test]
fn master_scrollable_takes_a_direction_and_reports_scrolls() {
    let mut adapter = boot(SCROLLABLE_DIRECTION);
    find(&adapter, "Start 0");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("flip").unwrap();
    for message in simulator.into_messages() {
        adapter.update(message).unwrap();
    }
    find(&adapter, "End 0");
    find(&adapter, "Beginning!");
}

const LENGTH_BOUNDS: &str = r#"
use iced::widget::{column, container, text};
use iced::{Fill, Fit, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        column![
            container(text("bounded")).width(Fit.max(600)),
            container(text("floored")).width(Fill.min(20.5)),
        ]
    }
}
"#;

#[test]
fn master_lengths_take_bounds() {
    let adapter = boot(LENGTH_BOUNDS);
    find(&adapter, "bounded");
    find(&adapter, "floored");
}

const FUNCTION_WITH: &str = r#"
use iced::widget::{button, column, text, text_input};
use iced::{Function, Widget};

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view).run()
}

#[derive(Default)]
struct State {
    title: String,
    last: i64,
}

#[derive(Debug, Clone)]
enum Message {
    TitleChanged(i64, String),
    Pressed(i64, bool),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::TitleChanged(id, title) => {
                self.last = id;
                self.title = title;
            }
            Message::Pressed(id, _) => self.last = id,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let id = 7;
        let pressed = Message::Pressed.with(id + 1);
        column![
            text(format!("{} {}", self.last, self.title)),
            text_input("Title", &self.title).on_input(Message::TitleChanged.with(id)),
            button("press").on_press(pressed(true)),
        ]
    }
}
"#;

#[test]
fn master_function_with_binds_the_first_input() {
    let mut adapter = boot(FUNCTION_WITH);
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("Title").unwrap();
    simulator.typewrite("hi");
    for message in simulator.into_messages() {
        adapter.update(message).unwrap();
    }
    find(&adapter, "7 hi");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("press").unwrap();
    for message in simulator.into_messages() {
        adapter.update(message).unwrap();
    }
    find(&adapter, "8 hi");
}
