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
