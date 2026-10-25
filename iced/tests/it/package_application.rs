use std::path::PathBuf;
use std::{fs, time};

use rawr::project;
use rawr_iced::{program, runtime};

#[test]
fn split_counter_checks_and_boots_through_the_package_graph() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/package_counter");
    let target = project::Target::discover(&root).unwrap();

    let prepared = rawr_iced::application::Compiler::new()
        .unwrap()
        .check(&target)
        .unwrap()
        .prepare()
        .unwrap();

    assert!(prepared.adapter().last_good_output().is_some());
    assert!(prepared.adapter().diagnostics().is_empty());
}

#[test]
fn dependency_component_maps_local_messages_into_the_parent_package() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/package_components");
    let target = project::Target::discover(&root).unwrap();
    let prepared = rawr_iced::application::Compiler::new()
        .unwrap()
        .check(&target)
        .unwrap()
        .prepare()
        .unwrap();
    let mut adapter = prepared.into_adapter();

    let message = click(&adapter, "Increment child");
    assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);
    assert_text(&adapter, "1");
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn package_contract_failures_retain_the_callback_module_source() {
    let root = std::env::temp_dir().join(format!(
        "rawr-package-contract-{}-{}",
        std::process::id(),
        time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(root.join("src")).unwrap();
    crate::write_marked(&root, "src/main.rs", "pub mod counter;\nuse crate::counter::Counter;\npub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }\n");
    crate::write_marked(&root, "src/counter.rs", "#[derive(Default)] pub struct Counter;\n#[derive(Clone)] enum Message { Tick }\nimpl Counter { pub fn update(&mut self, message: Message) {} pub fn view(&self) -> i64 { 42 } }\n");
    let target = project::Target::discover(&root).unwrap();

    let Err(error) = rawr_iced::application::Compiler::new()
        .unwrap()
        .check(&target)
    else {
        panic!("invalid package callback unexpectedly checked");
    };
    let report = error.report(None);

    assert!(report.labels()[0]
        .source()
        .identity()
        .ends_with("src/counter.rs"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn split_title_contract_failure_uses_the_exact_title_declaration_source() {
    let root = std::env::temp_dir().join(format!(
        "rawr-package-title-contract-{}-{}",
        std::process::id(),
        time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(root.join("src")).unwrap();
    crate::write_marked(&root, "src/main.rs", "pub mod app;\nuse crate::app::State;\npub fn main() -> iced::Result { iced::application(State::new, State::update, State::view).title(State::title).run() }\n");
    let title_source = r#"use iced::Element;
use iced::widget::text;

pub struct State {}
#[derive(Debug, Clone, Copy)] pub enum Message { Idle }

impl State {
    pub fn new() -> State { State {} }
    pub fn update(&mut self, message: Message) {}
    pub fn view(&self) -> Element<Message> { text("invalid title") }
    pub fn title(&self) -> bool { false }
}
"#;
    crate::write_marked(&root, "src/app.rs", title_source);
    let target = project::Target::discover(&root).unwrap();

    let Err(error) = rawr_iced::application::Compiler::new()
        .unwrap()
        .check(&target)
    else {
        panic!("invalid split-module title unexpectedly checked");
    };
    let report = error.report(None);
    let label = &report.labels()[0];
    let (line, declaration) = title_source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("fn title"))
        .expect("the fixture must retain its title declaration");
    let column = declaration
        .find("bool")
        .expect("the title declaration must retain its invalid output type");
    let expected = rawr::Span::new(
        rawr::Position::new((line + 2) as u16, (column + 1) as u16),
        rawr::Position::new((line + 2) as u16, (column + "bool".len() + 1) as u16),
    );

    assert!(label.source().identity().ends_with("src/app.rs"));
    assert_eq!(label.source().text(), format!("// rawr\n{title_source}"));
    assert_eq!(label.span(), expected);
    fs::remove_dir_all(root).unwrap();
}

fn assert_text(adapter: &program::Adapter, expected: &str) {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.find(expected).unwrap();
}

fn click(adapter: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(label).unwrap();
    simulator.into_messages().next().unwrap()
}

/// Rust's module and import forms across files: a module and a function of
/// one name, a trait imported from another module, nested import groups,
/// `use Trait as _`, an import in a function body through an imported
/// library module, and `view::view` as the application's view.
#[test]
fn modules_and_imports_resolve_as_rust_across_files() {
    let root = std::env::temp_dir().join(format!(
        "rawr-package-imports-{}-{}",
        std::process::id(),
        time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(root.join("src")).unwrap();
    crate::write_marked(
        &root,
        "src/main.rs",
        r#"use iced::widget::{button::{self, Status}, column};
use iced::keyboard;
use std::fmt::Write as _;

mod circle;
mod shapes;
mod view;

use circle::circle;
use shapes::{inner::{double, triple}, Area};

pub fn main() -> iced::Result {
    iced::run(update, view::view)
}

#[derive(Default)]
pub struct State {
    pub count: i64,
}

#[derive(Debug, Clone)]
pub enum Message {
    Add,
}

fn named(event: keyboard::Key) -> bool {
    use keyboard::key;
    event == keyboard::Key::Named(key::Named::Enter)
}

fn update(state: &mut State, message: Message) {
    match message {
        Message::Add => {
            state.count += circle(2) + shapes::Square(3).area() + double(1) + triple(1);
        }
    }
}
"#,
    );
    crate::write_marked(
        &root,
        "src/circle.rs",
        "pub fn circle(r: i64) -> i64 {\n    3 * r * r\n}\n",
    );
    crate::write_marked(
        &root,
        "src/shapes.rs",
        r#"pub trait Area {
    fn area(&self) -> i64;
}

pub struct Square(pub i64);

impl Area for Square {
    fn area(&self) -> i64 {
        self.0 * self.0
    }
}

pub mod inner {
    pub fn double(x: i64) -> i64 {
        x * 2
    }

    pub fn triple(x: i64) -> i64 {
        x * 3
    }
}
"#,
    );
    crate::write_marked(
        &root,
        "src/view.rs",
        r#"use iced::widget::{button, column, text};
use iced::Widget;
use crate::shapes::Area;
use crate::{Message, State};

pub fn view(state: &State) -> impl Widget<Message> {
    column![
        text(format!("count {} {}", state.count, crate::shapes::Square(2).area())),
        button("Add").on_press(Message::Add),
    ]
}
"#,
    );

    let target = project::Target::discover(&root).unwrap();
    let prepared = rawr_iced::application::Compiler::new()
        .unwrap()
        .check(&target)
        .unwrap_or_else(|error| panic!("{error}"))
        .prepare()
        .unwrap();
    let mut adapter = prepared.into_adapter();
    assert_text(&adapter, "count 0 4");
    let message = click(&adapter, "Add");
    assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);
    // 12 + 9 + 2 + 3, as the same crate built by cargo computes.
    assert_text(&adapter, "count 26 4");
    let _ = fs::remove_dir_all(root);
}
