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

/// `examples/exit/src/main.rs`, with `and_then(window::close)` written as a
/// closure: Rawr does not yet take a library function as a value.
const EXIT: &str = r#"
use iced::widget::{button, center, column};
use iced::window;
use iced::{Center, Task, Widget};

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
            Message::Confirm => window::latest().and_then(|id| window::close(id)),
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
            column![
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
