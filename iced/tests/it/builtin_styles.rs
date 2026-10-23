use rawr::timing::Phase;
use rawr::{Engine, ParseError};
use rawr_iced::{application, control, preview};

#[test]
fn every_builtin_style_binds_and_lowers_through_an_application() {
    let checked = application::Compiler::new()
        .unwrap()
        .check_source_observed(application_source())
        .into_result()
        .unwrap();
    let mut adapter = checked.prepare().unwrap().into_adapter();
    assert!(adapter.diagnostics().is_empty());

    let theme = adapter.theme().clone();
    let before = adapter.timing();
    for _ in 0..4 {
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.find("button primary").unwrap();
        simulator.find("button text").unwrap();
        simulator.find("text danger").unwrap();
        simulator.find("container danger").unwrap();
    }
    let after = adapter.timing();
    assert_eq!(
        after.aggregate(Phase::ViewEvaluation).count(),
        before.aggregate(Phase::ViewEvaluation).count(),
        "native lowering must not re-enter the Rawr view"
    );
    assert_eq!(
        after.aggregate(Phase::DescriptorLowering).count(),
        before.aggregate(Phase::DescriptorLowering).count() + 4
    );
    assert_eq!(
        after.aggregate(Phase::Installation).count(),
        before.aggregate(Phase::Installation).count(),
        "native lowering must not rebuild the host"
    );
    assert_eq!(
        adapter.theme(),
        &theme,
        "native lowering must retain the theme"
    );

    adapter.refresh_view().unwrap();
    assert_eq!(
        adapter.timing().aggregate(Phase::ViewEvaluation).count(),
        before.aggregate(Phase::ViewEvaluation).count() + 1
    );
}

#[test]
fn preview_boot_accepts_conditional_private_style_values() {
    let mut prepared = preview::Compiler::new()
        .unwrap()
        .check_named_source("styled-preview.rs", preview_source())
        .unwrap()
        .prepare()
        .unwrap();
    let control = prepared.control();

    assert_eq!(control.previews().len(), 1);
    assert!(control.status().is_active());
    assert!(matches!(
        control.status().selection(),
        control::Selection::Preview(_)
    ));
    assert!(control.diagnostics().is_empty());
}

#[test]
fn script_functions_and_cross_widget_styles_fail_at_the_source_position() {
    for (expression, expected) in [
        (
            "button(\"bad script\").style(State::fake)",
            "widget::Button::style",
        ),
        (
            "button(\"bad widget\").style(text::primary)",
            "widget::Button::style",
        ),
    ] {
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let source = invalid_source(expression);
        let error = engine.compile_application_descriptor(&source).unwrap_err();
        assert_positioned_style_error(&error, expected);
    }
}

fn assert_positioned_style_error(error: &ParseError, expected: &str) {
    assert!(
        error.to_string().contains(expected),
        "style diagnostic did not contain `{expected}`: {error}"
    );
    assert!(error.position().line().is_some());
}

fn invalid_source(expression: &str) -> String {
    format!(
        r#"
use iced::Element;
use iced::widget::{{button, text}};

struct State;
#[derive(Debug, Clone, Copy)] enum Message {{ Tick }}

impl State {{
    fn new() -> State {{ State {{}} }}
    fn fake(value: i64) -> i64 {{ value }}
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{ {expression} }}
}}

pub fn main() -> iced::Result {{
    iced::application(State::new, State::update, State::view).run()
}}
"#
    )
}

fn application_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::{button, column, container, text};
struct State { selected: bool }
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn new() -> State { State { selected: true } }
    fn update(&mut self, message: Message) {}

    fn view(&self) -> Element<'_, Message> {
        let conditional = if self.selected {
            button::primary
        } else {
            button::secondary
        };
        column![
            button("conditional").style(conditional),
            button("button primary").style(button::primary),
            button("button secondary").style(button::secondary),
            button("button success").style(button::success),
            button("button warning").style(button::warning),
            button("button danger").style(button::danger),
            button("button text").style(button::text),
            text("text primary").style(text::primary),
            text("text secondary").style(text::secondary),
            text("text success").style(text::success),
            text("text warning").style(text::warning),
            text("text danger").style(text::danger),
            container(text("container transparent")).style(container::transparent),
            container(text("container rounded box")).style(container::rounded_box),
            container(text("container bordered box")).style(container::bordered_box),
            container(text("container dark")).style(container::dark),
            container(text("container primary")).style(container::primary),
            container(text("container secondary")).style(container::secondary),
            container(text("container success")).style(container::success),
            container(text("container warning")).style(container::warning),
            container(text("container danger")).style(container::danger),
        ]
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#
}

fn preview_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::{button, column, container, text};

#[preview(name = "Styled")]
pub fn styled() -> Element<'_, ()> {
    let selected = true;
    let style = if selected { button::primary } else { button::secondary };
    container(column![
        button("preview tab").style(style),
        text("preview status").style(text::success),
    ]).style(container::rounded_box)
}
"#
}
