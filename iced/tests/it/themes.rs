use std::path::PathBuf;

use rawr::{diagnostic as rawr_diagnostic, host, project, Engine, Shared};
use rawr_iced::{diagnostic, program, runtime, theme};

#[test]
fn built_in_theme_values_are_selected_from_exact_application_metadata() {
    let adapter = boot_source(built_in_source());

    assert_eq!(adapter.theme().native(), &iced::Theme::Dark);
    assert!(adapter.last_good_theme().is_some());
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn dependency_package_builds_a_palette_theme_and_exact_widget_styles() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/package_themes");
    let target = project::Target::discover(&root).unwrap();
    let mut adapter = rawr_iced::application::Compiler::new()
        .unwrap()
        .check(&target)
        .unwrap()
        .prepare()
        .unwrap()
        .into_adapter();

    assert_eq!(adapter.theme().native().to_string(), "Midnight package");
    assert!(
        adapter.diagnostics().is_empty(),
        "unexpected package-theme diagnostics: {:?}",
        adapter.diagnostics()
    );
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.find("0").unwrap();
    simulator.click("Increment").unwrap();
    let message = simulator.into_messages().next().unwrap();
    assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.find("1").unwrap();
}

#[test]
fn standalone_preview_evaluates_a_palette_only_custom_theme() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let catalog = engine
        .compile_preview_catalog_source(
            rawr_diagnostic::Source::new("palette-preview.rs", preview_source()).unwrap(),
        )
        .unwrap();
    let engine = Shared::new(engine);
    let launch = engine.launch_previews(catalog).unwrap();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    let bound = launch.bind(registry).unwrap();
    let preview = &bound.previews()[0];
    let theme = bound.theme(preview.key(), 0, None).unwrap();
    let theme = theme::Value::from_dynamic(theme).unwrap();

    assert_eq!(theme.native().to_string(), "Preview palette");
}

#[test]
fn theme_failure_retains_the_last_good_value_and_recovers() {
    let mut adapter = boot_source(recovering_source());
    let initial = adapter.theme().clone();

    let fail = click(&adapter, "Fail theme");
    assert_eq!(adapter.update(fail).unwrap(), program::Delivery::Applied);
    assert_eq!(adapter.theme(), &initial);
    assert_eq!(adapter.diagnostics().len(), 1);
    assert_eq!(adapter.diagnostics()[0].stage(), diagnostic::Stage::Theme);
    assert!(
        adapter.diagnostics()[0].message().contains("primary color"),
        "{}",
        adapter.diagnostics()[0].message()
    );

    let recover = click(&adapter, "Recover theme");
    assert_eq!(adapter.update(recover).unwrap(), program::Delivery::Applied);
    assert_eq!(adapter.theme(), &initial);
    assert!(adapter.last_good_theme().is_some());
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn initial_theme_failure_uses_native_default_then_recovers() {
    let mut adapter = boot_source(initially_failing_source());

    assert_eq!(adapter.theme().native(), &iced::Theme::Light);
    assert!(adapter.last_good_theme().is_none());
    assert_eq!(adapter.diagnostics().len(), 1);
    assert_eq!(adapter.diagnostics()[0].stage(), diagnostic::Stage::Theme);

    let recover = click(&adapter, "Recover theme");
    assert_eq!(adapter.update(recover).unwrap(), program::Delivery::Applied);
    assert_eq!(adapter.theme().native().to_string(), "Initial recovery");
    assert!(adapter.last_good_theme().is_some());
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn wrong_theme_owner_and_output_are_compile_time_errors() {
    for (source, expected) in [
        (wrong_owner_source(), "theme state"),
        (wrong_output_source(), "theme output"),
    ] {
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let error = engine.compile_application_descriptor(source).unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "diagnostic did not contain `{expected}`: {error}"
        );
        assert!(error.position().line().is_some());
    }
}

#[test]
fn repeated_native_view_and_style_work_never_invoke_the_theme_callback() {
    let mut adapter = boot_source(recovering_source());
    let initial = adapter.theme().clone();
    let fail = click(&adapter, "Fail theme");
    adapter.update(fail).unwrap();
    let report = adapter.diagnostics()[0].report().clone();
    for _ in 0..4 {
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.find("Fail theme").unwrap();
    }
    assert_eq!(adapter.theme(), &initial);
    assert_eq!(adapter.diagnostics().len(), 1);
    assert_eq!(adapter.diagnostics()[0].report(), &report);
}

fn boot_source(source: &str) -> program::Adapter {
    rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(source)
        .unwrap()
        .prepare()
        .unwrap()
        .into_adapter()
}

fn click(adapter: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(label).unwrap();
    simulator.into_messages().next().unwrap()
}

fn built_in_source() -> &'static str {
    r#"
use iced::Theme;
use iced::Element;
use iced::widget::text;
struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { text("built-in") }
    fn theme(&self) -> Theme { Theme::Dark }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).theme(State::theme).run()
}
"#
}

fn recovering_source() -> &'static str {
    r##"
use iced::Theme;
use rawr_iced::theme::{Palette, custom};
use iced::Element;
use iced::widget::{button, column, text};
struct State { primary: String }
#[derive(Debug, Clone, Copy)] enum Message { Fail, Recover }
impl State {
    fn new() -> State { State { primary: "#4455aa".to_string() } }
    fn update(&mut self, message: Message) {
        match message {
            Message::Fail => self.primary = "invalid".to_string(),
            Message::Recover => self.primary = "#4455aa".to_string(),
        }
    }
    fn view(&self) -> Element<'_, Message> {
        let fail = button("Fail theme").on_press(Message::Fail);
        let recover = button("Recover theme").on_press(Message::Recover);
        column![fail, recover]
    }
    fn theme(&self) -> Theme {
        let palette = Palette::new("#10131a", "#e7eaf0", self.primary.clone(), "#78dba9", "#f4cc67", "#ff7d8b");
        custom("Recovering palette", palette)
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).theme(State::theme).run()
}
"##
}

fn initially_failing_source() -> &'static str {
    r##"
use iced::Theme;
use rawr_iced::theme::{Palette, custom};
use iced::Element;
use iced::widget::button;
struct State { primary: String }
#[derive(Debug, Clone, Copy)] enum Message { Recover }
impl State {
    fn new() -> State { State { primary: "invalid".to_string() } }
    fn update(&mut self, message: Message) {
        match message {
            Message::Recover => self.primary = "#4455aa".to_string(),
        }
    }
    fn view(&self) -> Element<'_, Message> {
        button("Recover theme").on_press(Message::Recover)
    }
    fn theme(&self) -> Theme {
        let palette = Palette::new("#10131a", "#e7eaf0", self.primary.clone(), "#78dba9", "#f4cc67", "#ff7d8b");
        custom("Initial recovery", palette)
    }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).theme(State::theme).run()
}
"##
}

fn preview_source() -> &'static str {
    r##"
use iced::Theme;
use iced::Element;
use iced::widget::button;
use rawr_iced::theme::{Palette, custom};

pub fn preview_theme() -> Theme {
    let palette = Palette::new("#10131a", "#e7eaf0", "#4455aa", "#78dba9", "#f4cc67", "#ff7d8b");
    custom("Preview palette", palette)
}

#[preview(name = "Palette", themes = [preview_theme])]
pub fn palette_preview() -> Element<'_, ()> {
    button("Palette preview").style(button::primary)
}
"##
}

fn wrong_owner_source() -> &'static str {
    r#"
use iced::Theme;
use iced::Element;
use iced::widget::text;
struct State; struct Other;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State { fn new() -> State { State {} } fn update(&mut self, message: Message) {} fn view(&self) -> Element<'_, Message> { text("x") } }
impl Other { fn theme(&self) -> Theme { Theme::Dark } }
pub fn main() -> iced::Result { iced::application(State::new, State::update, State::view).theme(Other::theme).run() }
"#
}

fn wrong_output_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text;
struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State { fn new() -> State { State {} } fn update(&mut self, message: Message) {} fn view(&self) -> Element<'_, Message> { text("x") } fn theme(&self) -> i64 { 1 } }
pub fn main() -> iced::Result { iced::application(State::new, State::update, State::view).theme(State::theme).run() }
"#
}

#[test]
fn an_optional_theme_selects_its_theme_or_keeps_the_default() {
    let source = |dark: bool| {
        format!(
            r#"
use iced::Theme;
use iced::Element;
use iced::widget::text;
struct State {{ dark: bool }}
#[derive(Debug, Clone, Copy)] enum Message {{ Tick }}
impl State {{
    fn new() -> State {{ State {{ dark: {dark} }} }}
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{ text("optional") }}
    fn theme(&self) -> Option<Theme> {{ if self.dark {{ Some(Theme::Dark) }} else {{ None }} }}
}}
pub fn main() -> iced::Result {{
    iced::application(State::new, State::update, State::view).theme(State::theme).run()
}}
"#
        )
    };
    let dark = boot_source(&source(true));
    assert_eq!(dark.theme().native(), &iced::Theme::Dark);
    assert!(dark.diagnostics().is_empty());
    let default = boot_source(&source(false));
    assert_eq!(default.theme().native(), &iced::Theme::Light);
    assert!(
        default.diagnostics().is_empty(),
        "{:?}",
        default.diagnostics()
    );
}
