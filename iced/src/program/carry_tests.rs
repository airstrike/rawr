//! State carried from one runner's program to the next one's.

use rawr::{application, host, Engine, Shared};

use super::Adapter;

fn source(view: &str, extra_field: &str) -> String {
    format!(
        r#"
use iced::widget::{{button, column, text}};
use iced::{{Color, Element}};
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {{ Idle, Busy(i64) }}
struct State {{ count: i64, label: String, mode: Mode, last: Option<char>, {extra_field} }}
#[derive(Debug, Clone)]
enum Message {{ Increment }}
impl State {{
    fn new() -> State {{
        State {{ count: 0, label: "start".to_string(), mode: Mode::Idle, last: None, {extra_value} }}
    }}
    fn update(&mut self, message: Message) {{
        match message {{
            Message::Increment => {{
                self.count += 1;
                self.mode = Mode::Busy(self.count);
                self.last = Some('x');
                self.label = format!("n{{}}", self.count);
            }}
        }}
    }}
    fn view(&self) -> Element<'_, Message> {{
        column![
            button(text("Increment")).on_press(Message::Increment),
            text(format!("{view} {{}} {{}} {{:?}} {{:?}}", self.count, self.label, self.mode, self.last)),
        ]
    }}
}}
pub fn main() -> iced::Result {{
    iced::application(State::new, State::update, State::view).run()
}}
"#,
        extra_value = match extra_field.split_once(':') {
            Some((name, ty)) if ty.contains("Color") => format!("{name}: Color::BLACK"),
            Some((name, _)) => format!("{name}: 0"),
            None => String::new(),
        },
    )
}

fn boot(source: &str) -> Adapter {
    let mut engine = Engine::new();
    crate::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = host::runtime::Registry::new();
    crate::install_test_runtime(&mut registry);
    let launch: application::BoundLaunch = launch.bind(registry).unwrap();
    Adapter::boot(launch).unwrap()
}

fn increment(adapter: &mut Adapter) {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("Increment").unwrap();
    let message = simulator.into_messages().next().unwrap();
    adapter.update(message).unwrap();
}

fn shows(adapter: &Adapter, line: &str) -> bool {
    iced_test::simulator(adapter.view()).find(line).is_ok()
}

fn export(adapter: &Adapter) -> Option<Vec<u8>> {
    adapter.export_state_authority(adapter.launch.static_authority().unwrap())
}

fn seed(adapter: &mut Adapter, bytes: &[u8]) -> bool {
    let launch = adapter.launch.static_authority().unwrap().clone();
    adapter.seed_state_authority(&launch, bytes, None)
}

#[test]
fn plain_state_carries_into_the_next_program() {
    let mut old = boot(&source("old", ""));
    increment(&mut old);
    increment(&mut old);
    let bytes = export(&old).expect("plain script data is carried");

    let mut new = boot(&source("new", ""));
    assert!(shows(&new, "new 0 start Idle None"));
    assert!(seed(&mut new, &bytes));
    assert!(shows(&new, "new 2 n2 Busy(2) Some('x')"));
    increment(&mut new);
    assert!(shows(&new, "new 3 n3 Busy(3) Some('x')"));
}

#[test]
fn a_changed_state_type_boots_fresh() {
    let mut old = boot(&source("old", ""));
    increment(&mut old);
    let bytes = export(&old).unwrap();

    let mut new = boot(&source("new", "extra: i64"));
    assert!(!seed(&mut new, &bytes));
    assert!(shows(&new, "new 0 start Idle None"));
}

#[test]
fn colors_and_themes_in_state_are_carried() {
    let mut old = boot(&source("old", "tint: Color"));
    increment(&mut old);
    let bytes = export(&old).expect("a Color is plain data");
    let mut new = boot(&source("new", "tint: Color"));
    assert!(seed(&mut new, &bytes));
    assert!(shows(&new, "new 1 n1 Busy(1) Some('x')"));

    let themes = |tag: &str| {
        COLLECTIONS.replace("TAG", tag).replace(
            "boxed: Box<Option<char>> }",
            "boxed: Box<Option<char>>, themes: Vec<Theme>, tint: Option<Color> }",
        )
        .replace(
            "self.pair = (n, format!(\"p{}\", n));",
            "self.pair = (n, format!(\"p{}\", n)); self.themes = vec![Theme::Dracula, Theme::custom(\"Mine\".to_string(), Seed { primary: Color::from_rgb8(1, 2, 3), ..Seed::default() })]; self.tint = Some(Color::from_rgb8(9, 8, 7));",
        )
        .replace(
            "self.boxed)),",
            "self.boxed)), text(format!(\"{} {} {}\", self.themes.len(), self.themes.get(1).map(|t| t.palette().primary.base.color) == Some(Color::from_rgb8(1, 2, 3)), self.tint == Some(Color::from_rgb8(9, 8, 7)))),",
        )
        .replace("use iced::Element;", "use iced::{Color, Element, Theme};\nuse iced::theme::palette::Seed;")
    };
    let mut old = boot(&themes("old"));
    increment(&mut old);
    let bytes = export(&old).expect("Themes are plain data");
    let mut new = boot(&themes("new"));
    assert!(shows(&new, "0 false false"));
    assert!(seed(&mut new, &bytes));
    assert!(shows(&new, "2 true true"));
}

#[test]
fn malformed_bytes_boot_fresh() {
    let mut old = boot(&source("old", ""));
    increment(&mut old);
    let mut bytes = export(&old).unwrap();
    bytes.truncate(bytes.len() - 1);

    let mut new = boot(&source("new", ""));
    assert!(!seed(&mut new, &bytes));
    assert!(!seed(&mut new, &[]));
    assert!(shows(&new, "new 0 start Idle None"));
}

const COLLECTIONS: &str = r#"
use std::collections::{BTreeMap, HashSet};
use iced::widget::{button, column, text};
use iced::Element;
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode { Idle, Busy }
#[derive(Default)]
struct State { items: Vec<i64>, names: BTreeMap<String, Mode>, seen: HashSet<i64>, pair: (i64, String), boxed: Box<Option<char>> }
#[derive(Debug, Clone)]
enum Message { Increment }
impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                let n = self.items.len() as i64 + 1;
                self.items.push(n);
                self.names.insert(format!("k{}", n), Mode::Busy);
                self.seen.insert(n % 2);
                self.pair = (n, format!("p{}", n));
            }
        }
    }
    fn view(&self) -> Element<'_, Message> {
        column![
            button(text("Increment")).on_press(Message::Increment),
            text(format!("TAG {:?} {:?} {:?} {:?} {:?}", self.items, self.names, self.seen, self.pair, self.boxed)),
        ]
    }
}
pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
"#;

#[test]
fn collections_and_tuples_in_state_boot_and_carry() {
    let mut old = boot(&COLLECTIONS.replace("TAG", "old"));
    assert!(shows(&old, "old [] {} {} (0, \"\") None"));
    increment(&mut old);
    increment(&mut old);
    let shown = "[1, 2] {\"k1\": Busy, \"k2\": Busy} {0, 1} (2, \"p2\") None";
    assert!(shows(&old, &format!("old {shown}")));
    let bytes = export(&old).expect("collections are plain data");

    let mut new = boot(&COLLECTIONS.replace("TAG", "new"));
    assert!(seed(&mut new, &bytes));
    assert!(shows(&new, &format!("new {shown}")));
}
