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

/// std's `PathBuf`, `Path`, `OsStr` and `OsString`, as iced's editor and
/// todos examples use them.
const PATHS: &str = r#"
use std::ffi;
use std::path::{Path, PathBuf};
use iced::widget::{column, text};
use iced::Widget;

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
enum Message {}

impl State {
    fn update(&mut self, message: Message) {
        match message {}
    }

    fn view(&self) -> impl Widget<Message> {
        let mut path = PathBuf::from("/notes");
        path.push("todo.md");
        let file = Some(path.clone());
        let extension = file
            .clone()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
            .unwrap_or("rs");
        let parent = path.parent().map(|dir| dir.display().to_string()).unwrap_or_default();
        let name = ffi::OsString::from("todo.md");
        let same = path.file_name() == Some(name) && self.file.is_none();
        column![
            text(format!("{} {extension} {parent}", path.display())),
            text(format!("{same} {}", path.to_path_buf().join("x").to_string_lossy())),
        ]
    }
}
"#;

#[test]
fn std_path_and_ffi_types_compute_as_rust() {
    let adapter = boot(PATHS);
    find(&adapter, "/notes/todo.md md /notes");
    find(&adapter, "true /notes/todo.md/x");
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

/// An array of the script's own enum where iced takes `impl Borrow<[T]>`,
/// as ferris passes its rotation strategies.
const ARRAY_OPTIONS: &str = r#"
use iced::widget::{column, pick_list};
use iced::Widget;

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    choice: Choice,
}

#[derive(Debug, Clone)]
enum Message {
    Picked(Choice),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Choice {
    #[default]
    Floating,
    Solid,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Choice::Floating => "floating",
            Choice::Solid => "solid",
        })
    }
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Picked(choice) => self.choice = choice,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            pick_list(Some(self.choice), [Choice::Floating, Choice::Solid], Choice::to_string)
                .on_select(Message::Picked)
        ]
    }
}
"#;

#[test]
fn master_pick_list_takes_an_array_of_script_values() {
    let adapter = boot(ARRAY_OPTIONS);
    find(&adapter, "floating");
}

/// `slider(Degrees::RANGE, ..)` over the rotation's degrees, as ferris
/// writes it, and an associated const of the script's own.
const ANGLE_SLIDER: &str = r#"
use iced::widget::{column, slider, text};
use iced::{Degrees, Radians, Rotation, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    rotation: Rotation,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    RotationChanged(Degrees),
    RadiansChanged(Radians),
}

struct Turn;

impl Turn {
    const QUARTER: f32 = 90.0;
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::RotationChanged(rotation) => self.rotation = Rotation::Floating(rotation.into()),
            Message::RadiansChanged(rotation) => self.rotation = Rotation::Solid(rotation),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            slider(Degrees::RANGE, self.rotation.degrees(), Message::RotationChanged),
            slider(Radians::RANGE, self.rotation.radians(), Message::RadiansChanged),
            text(format!("quarter {}", Turn::QUARTER)),
        ]
    }
}
"#;

#[test]
fn master_slider_moves_over_an_angle_range() {
    let adapter = boot(ANGLE_SLIDER);
    find(&adapter, "quarter 90");
}

/// `Checkbox::text_size`, and `T::default()` of iced's `Default` types,
/// as ferris writes them.
const CHECKBOX_AND_DEFAULTS: &str = r#"
use iced::widget::{checkbox, column, text};
use iced::{Color, ContentFit, Point, Vector, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone, Copy)]
enum Message {
    Toggled(bool),
}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        let point = Point::default() + Vector::default();
        column![
            checkbox(true).label("Spin!").text_size(12).on_toggle(Message::Toggled),
            text(format!(
                "{} {:?} {}",
                ContentFit::default(),
                Color::default() == Color::TRANSPARENT,
                point.x,
            )),
        ]
    }
}
"#;

#[test]
fn master_checkbox_text_size_and_facade_defaults() {
    let adapter = boot(CHECKBOX_AND_DEFAULTS);
    find(&adapter, "Spin!");
    find(&adapter, "Contain true 0");
}

/// std's `io::Error` and `io::ErrorKind`, as editor and markdown use them.
const IO_ERRORS: &str = r#"
use iced::widget::{column, text};
use iced::Widget;
use std::io;

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone)]
enum Message {}

#[derive(Debug, Clone)]
pub enum Error {
    DialogClosed,
    IoError(io::ErrorKind),
}

fn classify(error: io::Error) -> Error {
    Error::IoError(error.kind())
}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        let missing = classify(io::Error::new(io::ErrorKind::NotFound, String::from("gone")));
        let other = io::Error::other(format!("unsupported uri: {}", "x"));
        let same = matches!(missing, Error::IoError(io::ErrorKind::NotFound));
        column![
            text(format!("{missing:?} {same}")),
            text(format!("{} {:?} {}", other, other.kind(), io::ErrorKind::TimedOut)),
        ]
    }
}
"#;

#[test]
fn std_io_errors_compute_as_rust() {
    let adapter = boot(IO_ERRORS);
    find(&adapter, "IoError(NotFound) true");
    find(&adapter, "unsupported uri: x Other timed out");
}

/// iced's `Application` builder returned by a function of the crate, as
/// todos writes it.
const APPLICATION_BUILDER: &str = r#"
use iced::widget::{button, column, text};
use iced::{Application, Program, Theme, Widget};

pub fn main() -> iced::Result {
    application().run()
}

fn application() -> Application<impl Program<Message = Message, Theme = Theme>> {
    iced::application(Counter::default, Counter::update, Counter::view)
        .title(Counter::title)
}

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Counter {
    fn title(&self) -> String {
        format!("Count {}", self.value)
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            text(format!("value {}", self.value + 7)),
            button("+").on_press(Message::Increment),
        ]
    }
}
"#;

#[test]
fn master_application_builder_from_a_function() {
    let adapter = boot(APPLICATION_BUILDER);
    find(&adapter, "value 7");
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

const FONT_FIELDS: &str = r#"
use iced::font;
use iced::widget::{column, text};
use iced::{Font, Widget};

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
        let font = Font {
            weight: font::Weight::Bold,
            ..Font::MONOSPACE
        };
        column![
            text(format!(
                "{} {}",
                font.weight == font::Weight::Bold,
                font.family == font::Family::Monospace
            )),
            text("fields"),
        ]
    }
}
"#;

#[test]
fn master_font_fields_read_as_fields() {
    let adapter = boot(FONT_FIELDS);
    find(&adapter, "true true");
}

const STRING_IDS: &str = r#"
use iced::widget::{button, column, operation, scrollable, text, text_input};
use iced::widget::scrollable::RelativeOffset;
use iced::{Task, Widget};

const LIST: &str = "list";

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
    Input(String),
}

impl State {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Bottom => {
                operation::scrollable::snap_to(LIST, RelativeOffset::END, operation::Animation::Auto)
            }
            Message::Focus => operation::focus("input"),
            Message::Input(input) => {
                self.input = input;
                Task::none()
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            button("bottom").on_press(Message::Bottom),
            button("focus").on_press(Message::Focus),
            text_input("Type", &self.input).id("input").on_input(Message::Input),
            scrollable(text("contents")).id(LIST),
        ]
    }
}
"#;

#[test]
fn master_widget_ids_take_strings() {
    let mut adapter = boot(STRING_IDS);
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("bottom").unwrap();
    simulator.click("focus").unwrap();
    for message in simulator.into_messages() {
        let _ = adapter.update(message).unwrap();
    }
    find(&adapter, "contents");
}

const TABLE: &str = r#"
use iced::widget::{column, table, text};
use iced::{Center, Fill, Right, Widget};

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}

struct State {
    events: Vec<Event>,
}

#[derive(Clone)]
struct Event {
    name: String,
    price: f32,
}

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn new() -> Self {
        Self {
            events: vec![
                Event { name: "Concert".to_string(), price: 20.5 },
                Event { name: "Picnic".to_string(), price: 0.0 },
            ],
        }
    }

    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        let columns = [
            table::column(text("Name"), |event: &Event| text(&event.name)).width(Fill.max(400)),
            table::column(text("Price"), |event: &Event| {
                if event.price > 0.0 {
                    text!("${:.2}", event.price)
                } else {
                    text("Free").width(Fill).center()
                }
            })
            .align_x(Right)
            .align_y(Center),
        ];
        column![
            table(columns, &self.events)
                .padding_x(10.0)
                .padding_y(5.0)
                .separator_x(1.0)
                .separator_y(1.0),
        ]
    }
}
"#;

#[test]
fn master_table_draws_a_cell_per_column_and_row() {
    let adapter = boot(TABLE);
    find(&adapter, "Name");
    find(&adapter, "Picnic");
    find(&adapter, "$20.50");
    find(&adapter, "Free");
}

const TEXT_DEFAULT_STYLE: &str = r#"
use iced::widget::{column, text};
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
        let minutes = 95;
        column![
            text!("{minutes} min").style(if minutes > 90 { text::warning } else { text::default }),
            text("inherited").style(text::default),
        ]
    }
}
"#;

#[test]
fn master_text_takes_the_default_style() {
    let adapter = boot(TEXT_DEFAULT_STYLE);
    find(&adapter, "95 min");
    find(&adapter, "inherited");
}

const DISPLAY_LABELS: &str = r#"
use iced::font;
use iced::widget::{column, pick_list, text};
use iced::{ContentFit, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone)]
enum Message {
    Family(font::Family),
    Fit(ContentFit),
}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        column![
            pick_list(
                Some(font::Family::Monospace),
                [font::Family::Serif, font::Family::Monospace],
                font::Family::to_string,
            )
            .on_select(Message::Family),
            pick_list(
                Some(ContentFit::Contain),
                [ContentFit::Contain, ContentFit::Cover],
                ContentFit::to_string,
            )
            .on_select(Message::Fit),
            text(ContentFit::ScaleDown.to_string()),
        ]
    }
}
"#;

#[test]
fn master_families_and_fits_label_themselves() {
    let adapter = boot(DISPLAY_LABELS);
    find(&adapter, "Scale Down");
}

const ROTATION: &str = r#"
use iced::widget::{column, image, text};
use iced::{Degrees, Radians, Rotation, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    rotation: Rotation,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Turned(Degrees),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Turned(rotation) => {
                self.rotation = match self.rotation {
                    Rotation::Floating(_) => Rotation::Floating(rotation.into()),
                    Rotation::Solid(_) => Rotation::Solid(rotation.into()),
                };
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let solid = Rotation::Solid(Radians::PI);
        let label = match solid {
            Rotation::Floating(_) => "floating",
            Rotation::Solid(_) => "solid",
        };
        column![
            text(label),
            text(format!("{}", solid.degrees() == Degrees(180.0))),
            image("ferris.png").rotation(self.rotation).width(100),
        ]
    }
}
"#;

#[test]
fn master_rotations_match_and_turn_images() {
    let adapter = boot(ROTATION);
    find(&adapter, "solid");
    find(&adapter, "true");
}

/// iced master's layout example sizes a square by `impl Into<Length> + Copy`.
#[test]
fn master_impl_into_length_takes_a_number() {
    let adapter = boot(
        r#"
use iced::widget::{column, container, text};
use iced::{Length, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State;

#[derive(Debug, Clone)]
enum Message {}

fn square(size: impl Into<Length> + Copy) -> impl Widget<Message> {
    container(text("square")).width(size).height(size)
}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        column![square(50), square(40.5), square(Length::Fill)]
    }
}
"#,
    );
    find(&adapter, "square");
}

/// iced master's gallery example fades with an `Animation<bool>`.
#[test]
fn master_bool_animations_go_and_interpolate() {
    let mut adapter = boot(
        r#"
use iced::animation;
use iced::time::{Instant, milliseconds};
use iced::widget::{button, column, text};
use iced::{Animation, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

struct State {
    fade_in: Animation<bool>,
    now: Instant,
}

impl Default for State {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            fade_in: Animation::new(false)
                .easing(animation::Easing::EaseIn)
                .duration(milliseconds(200)),
            now,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Show,
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Show => self.fade_in.go_mut(true, self.now),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let opacity = self.fade_in.interpolate(0.0, 1.0, self.now);
        column![
            button(text("Show")).on_press(Message::Show),
            text(format!(
                "{} {} {opacity:.1}",
                self.fade_in.value(),
                self.fade_in.is_animating(self.now)
            )),
        ]
    }
}
"#,
    );
    find(&adapter, "false false 0.0");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("Show").unwrap();
    let show = simulator.into_messages().next().expect("the button emits");
    adapter.update(show).unwrap();
    find(&adapter, "true true 0.0");
}

/// iced master's ferris example turns its rotation through
/// `*self.rotation.radians_mut() = ..`.
#[test]
fn master_rotation_radians_mut_writes_the_place() {
    let adapter = boot(
        r#"
use iced::widget::{column, text};
use iced::{Degrees, Radians, Rotation, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

struct State {
    rotation: Rotation,
}

impl Default for State {
    fn default() -> Self {
        let mut rotation = Rotation::Solid(Radians(1.0));
        *rotation.radians_mut() = Radians(2.0);
        let mut state = Self { rotation };
        *state.rotation.radians_mut() = (state.rotation.radians() + Radians::PI) % (2.0 * Radians::PI);
        state
    }
}

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        let solid = match self.rotation {
            Rotation::Solid(_) => "solid",
            Rotation::Floating(_) => "floating",
        };
        column![text(format!("{solid} {:.1}", f32::from(self.rotation.degrees())))]
    }
}
"#,
    );
    find(&adapter, "solid 294.6");
}

/// iced master's delineate example matches `Some(Point { x, y })`.
#[test]
fn master_facade_struct_patterns_bind_their_fields() {
    let adapter = boot(
        r#"
use iced::widget::{column, text};
use iced::{Point, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

struct State {
    at: Option<Point>,
}

impl Default for State {
    fn default() -> Self {
        Self { at: Some(Point::new(3.0, 4.0)) }
    }
}

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        let at = match self.at {
            Some(Point { x, y }) => format!("at {x} {y}"),
            None => "unknown".to_string(),
        };
        let Point { x, .. } = Point::ORIGIN;
        column![text(at), text(format!("origin {x}"))]
    }
}
"#,
    );
    find(&adapter, "at 3 4");
    find(&adapter, "origin 0");
}

/// iced master's table example: an `f32` slider whose closure calls a
/// function of `f32`.
#[test]
fn master_sliders_take_f32_values_and_closures() {
    let adapter = boot(
        r#"
use iced::widget::{column, slider, text};
use iced::Widget;

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    padding: (f32, f32),
}

#[derive(Debug, Clone)]
enum Message {
    Padding(f32, f32),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Padding(x, y) => self.padding = (x, y),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let labeled = |range: std::ops::RangeInclusive<f32>, (x, y), on_change: fn(f32, f32) -> Message| {
            column![
                slider(range.clone(), x, move |x| on_change(x, y)),
                slider(range, y, move |y| on_change(x, y)),
                text(format!("{x:.0} {y:.0}px")),
            ]
        };
        labeled(0.0..=30.0, self.padding, Message::Padding)
    }
}
"#,
    );
    find(&adapter, "0 0px");
}

/// iced master's vectorial_text example: a closure whose parameters a
/// slider and an `fn(f32)` message settle.
#[test]
fn master_slider_closures_take_f32_messages() {
    let adapter = boot(
        r#"
use iced::widget::{column, row, slider, text};
use iced::Widget;

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {
    size: f32,
}

#[derive(Debug, Clone)]
enum Message {
    SizeChanged(f32),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::SizeChanged(size) => self.size = size,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let slider_with_label = |label, range, value, message: fn(f32) -> _| {
            column![
                row![text(label), text!("{:.2}", value)],
                slider(range, value, message).step(0.01)
            ]
            .spacing(2)
        };
        slider_with_label("Size", 2.0..=80.0, self.size, Message::SizeChanged)
    }
}
"#,
    );
    find(&adapter, "0.00");
}

/// iced master's delineate example matches a tuple variant with `{ .. }`.
#[test]
fn master_tuple_variants_match_with_braces() {
    let adapter = boot(
        r#"
use iced::event::{self, Event};
use iced::widget::text;
use iced::{window, Subscription, Widget};

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view)
        .subscription(State::subscription)
        .run()
}

#[derive(Default)]
struct State {
    resizes: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Event(Event),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Event(Event::Window(window::Event::Resized { .. })) => self.resizes += 1,
            Message::Event(_) => {}
        }
    }

    fn view(&self) -> impl Widget<Message> {
        text(format!("resizes {}", self.resizes))
    }

    fn subscription(&self) -> Subscription<Message> {
        event::listen().map(Message::Event)
    }
}
"#,
    );
    find(&adapter, "resizes 0");
}

/// iced master's gallery and markdown examples start an animation with
/// `.go(true, now)` at the end of its builders.
#[test]
fn master_animation_go_builds_a_started_animation() {
    let adapter = boot(
        r#"
use iced::animation;
use iced::time::{Instant, milliseconds};
use iced::widget::text;
use iced::{Animation, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

struct State {
    fade_in: Animation<bool>,
    now: Instant,
}

impl Default for State {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            fade_in: Animation::new(false)
                .duration(milliseconds(700))
                .easing(animation::Easing::EaseIn)
                .go(true, now),
            now,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        text(format!("{} {}", self.fade_in.value(), self.fade_in.is_animating(self.now)))
    }
}
"#,
    );
    find(&adapter, "true true");
}

/// A generative piece scales a `Vector` by an `f32` and moves a `Point` by
/// it, as iced's `Mul<f32>` and `Add<Vector>` allow.
#[test]
fn vectors_scale_by_f32_and_move_points() {
    let adapter = boot(
        r#"
use iced::widget::text;
use iced::{Point, Vector, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {}

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        let fade: f32 = 0.25 + 0.25;
        let moved = Vector::new(1.5, 2.0) * 2.0 * fade * 2.0;
        let at = Point::new(1.0, 1.0) + moved - Vector::new(1.0, 0.0);
        let back = at - Point::ORIGIN;
        text(format!("{} {} {}", moved.x, moved.y, back.y))
    }
}
"#,
    );
    find(&adapter, "3 4 5");
}

/// A generative piece fades a `color!` by an alpha it computes, as iced's
/// macro takes any `f32` there.
#[test]
fn color_takes_a_runtime_alpha() {
    let adapter = boot(
        r#"
use iced::widget::text;
use iced::{color, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {}

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        let fade: f32 = 0.25 + 0.25;
        let tint = color!(0xffc8a8, fade);
        let rgba = color!(255, 200, 168, fade * 0.5);
        text(format!("{} {} {}", tint.a, rgba.a, tint.r))
    }
}
"#,
    );
    find(&adapter, "0.5 0.25 1");
}

/// `use iced::widget::canvas::{self, ..}` imports the module only, so
/// `canvas(..)` is not the function there, as rustc says.
#[test]
fn a_module_import_does_not_bring_its_function() {
    let error = rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(
            r#"
use iced::widget::canvas::{self, Frame, Geometry};
use iced::{mouse, Rectangle, Renderer, Theme, Widget};

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}

#[derive(Default)]
struct State {}

#[derive(Debug, Clone)]
enum Message {}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        canvas(self)
    }
}

impl canvas::Program<Message> for State {
    type State = ();

    fn draw(&self, _: &(), r: &Renderer, _: &Theme, b: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        vec![Frame::new(r, b.size()).into_geometry()]
    }
}
"#,
        )
        .err()
        .expect("rustc rejects calling a module")
        .to_string();
    assert!(error.contains("canvas"), "{error}");
}
