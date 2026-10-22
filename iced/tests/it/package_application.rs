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
    fs::write(
        root.join("src/main.rawr"),
        "pub mod counter;\nuse crate::counter::Counter;\npub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }\n",
    )
    .unwrap();
    fs::write(
        root.join("src/counter.rawr"),
        "#[derive(Default)] pub struct Counter;\n#[derive(Clone)] enum Message { Tick }\nimpl Counter { pub fn update(&mut self, message: Message) {} pub fn view(&self) -> i64 { 42 } }\n",
    )
    .unwrap();
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
        .ends_with("src/counter.rawr"));
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
    fs::write(
        root.join("src/main.rawr"),
        "pub mod app;\nuse crate::app::State;\npub fn main() -> iced::Result { iced::application(State::new, State::update, State::view).title(State::title).run() }\n",
    )
    .unwrap();
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
    fs::write(root.join("src/app.rawr"), title_source).unwrap();
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
        rawr::Position::new((line + 1) as u16, (column + 1) as u16),
        rawr::Position::new((line + 1) as u16, (column + "bool".len() + 1) as u16),
    );

    assert!(label.source().identity().ends_with("src/app.rawr"));
    assert_eq!(label.source().text(), title_source);
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
