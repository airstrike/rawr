use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rawr::{capability, host, resolve, Dynamic, Engine, ParseError, ParseErrorType, Scope, Shared};
use rawr_iced::program;

#[derive(Clone)]
struct LeftDescriptor {
    witness: host::runtime::Witness,
}

#[derive(Clone)]
struct RightDescriptor {
    witness: host::runtime::Witness,
}

#[test]
fn exact_counter_view_lowers_registered_intrinsics() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let golden = include_str!("../../examples/scripts/application_counter.rs");
    let descriptor = engine.compile_application_descriptor(golden).unwrap();

    assert_eq!(descriptor.state().display_name(), "main::Counter");
    assert_eq!(descriptor.message().display_name(), "main::Message");
    // `impl Widget<Message>` is the erased Element itself, so nothing projects.
    assert!(descriptor.view().projection().is_none());

    // The same counter with its view typed as the `Column` it builds projects
    // that Column to an Element.
    let column = golden
        .replace("{Widget, button", "{Column, button")
        .replace("impl Widget<Message>", "Column<'_, Message>");
    let descriptor = engine.compile_application_descriptor(column).unwrap();
    assert_eq!(
        descriptor.view().projection().unwrap().target(),
        "iced::Element"
    );
}

#[test]
fn canonical_value_return_chain_stays_within_source_complexity_limit() {
    compile_value_probe(
        r#"
column![
    button("Increment").on_press(Message::Increment),
    text(0).size(50),
    button("Decrement").on_press(Message::Decrement),
]
.align_x(Center)
"#,
    )
    .unwrap();
}

#[test]
fn wide_and_nested_columns_have_bounded_private_call_depth() {
    let children = (0..48)
        .map(|value| format!("text({value}).size(18)"))
        .collect::<Vec<_>>()
        .join(",\n");
    compile_value_probe(&format!("column![{children},].padding(24).align_x(Center)")).unwrap();

    let nested = (0..24).fold("text(0)".to_owned(), |child, _| format!("column![{child}]"));
    compile_value_probe(&format!("{nested}.padding(24).align_x(Center)")).unwrap();
}

#[test]
fn canonical_row_and_container_layout_compiles_with_owned_builders() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine
        .compile(
            r#"
use iced::{Center, Length};
use iced::Element;
use iced::widget::{column, container, row, text};
#[derive(Debug, Clone, Copy)]
enum Message { Tick }
pub fn probe() -> Element<'_, Message> {
    container(
        row![
            container(column![text("first")].spacing(8))
                .padding(16)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Center)
                .align_y(Center)
                .style(container::bordered_box),
            container(column![text("second")])
                .padding(16)
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .spacing(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Center),
    )
    .padding(24)
    .width(Length::Fill)
    .height(Length::Fill)
}
"#,
        )
        .unwrap();
}

#[test]
fn if_projects_row_and_column_branches_to_one_value() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine
        .compile(
            r#"
use iced::Element;
use iced::widget::{column, row, text};
#[derive(Debug, Clone, Copy)] enum Message { Tick }
pub fn adaptive(horizontal: bool) -> Element<'_, Message> {
    if horizontal {
        row![text("first"), text("second")]
    } else {
        column![text("first"), text("second")]
    }
}
"#,
        )
        .unwrap();
}

#[test]
fn text_and_space_infer_fresh_message_types_per_projection_site() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine
        .compile(
            r#"
use iced::Element;
use iced::widget::{Space, Text, space, text};

#[derive(Debug, Clone, Copy)] enum First { Tick }
#[derive(Debug, Clone, Copy)] enum Second { Tick }

pub fn concrete_text() -> Text { text("concrete") }
pub fn concrete_space() -> Space { space() }
pub fn first_text() -> Element<First> { text("first") }
pub fn second_text() -> Element<Second> { text("second") }
pub fn first_space() -> Element<First> { space() }
pub fn second_space() -> Element<Second> { space() }
"#,
        )
        .unwrap();
}

#[test]
fn text_and_space_branches_lower_fresh_native_elements_repeatedly() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine
        .compile_application_descriptor(
            r#"
use iced::{Element, Length};
use iced::widget::{button, column, space, text};

struct State { empty: bool }
#[derive(Debug, Clone, Copy)] enum Message { Toggle }

impl State {
    fn new() -> State { State { empty: false } }
    fn update(&mut self, message: Message) { self.empty = self.empty == false }
    fn content(&self) -> Element<Message> {
        if self.empty { space().width(Length::Fill) } else { text("Fresh text") }
    }
    fn view(&self) -> Element<Message> {
        column![self.content(), button("Toggle content").on_press(Message::Toggle)]
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#,
        )
        .unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    let mut adapter = program::Adapter::boot(launch.bind(registry).unwrap()).unwrap();

    for _ in 0..2 {
        let mut rendered = iced_test::simulator(adapter.view());
        rendered.find("Fresh text").unwrap();
    }

    let mut rendered = iced_test::simulator(adapter.view());
    rendered.click("Toggle content").unwrap();
    let message = rendered.into_messages().next().unwrap();
    assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);
    let mut empty = iced_test::simulator(adapter.view());
    assert!(empty.find("Fresh text").is_err());
    empty.click("Toggle content").unwrap();
    let message = empty.into_messages().next().unwrap();
    assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);

    let mut restored = iced_test::simulator(adapter.view());
    restored.find("Fresh text").unwrap();
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn row_and_column_branches_both_project_and_render_natively() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine
        .compile_application_descriptor(
            r#"
use iced::Element;
use iced::widget::{button, column, row, text};

struct State { horizontal: bool }
#[derive(Debug, Clone, Copy)] enum Message { Toggle }

impl State {
    fn new() -> State { State { horizontal: true } }
    fn update(&mut self, message: Message) { self.horizontal = self.horizontal == false }
    fn view(&self) -> Element<Message> {
        if self.horizontal {
            row![text("Horizontal"), button("Toggle layout").on_press(Message::Toggle)]
        } else {
            column![text("Vertical"), button("Toggle layout").on_press(Message::Toggle)]
        }
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#,
        )
        .unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    let mut adapter = program::Adapter::boot(launch.bind(registry).unwrap()).unwrap();

    let mut initial = iced_test::simulator(adapter.view());
    initial.find("Horizontal").unwrap();
    for expected in ["Vertical", "Horizontal"] {
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.click("Toggle layout").unwrap();
        let message = simulator.into_messages().next().unwrap();
        assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);

        let mut rendered = iced_test::simulator(adapter.view());
        rendered.find(expected).unwrap();
    }
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn selected_branch_projection_failure_retains_the_exact_tail_expression() {
    for (selected, expression) in [
        ("true", "left(Message::Idle)"),
        ("false", "right(Message::Idle)"),
    ] {
        let source = r#"
use branch_projection::{left, right};
use iced::Element;

struct State { left: bool }
#[derive(Debug, Clone, Copy)] enum Message { Idle }

impl State {
    fn new() -> State { State { left: __SELECTED__ } }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> {
        if self.left {
            left(Message::Idle)
        } else {
            right(Message::Idle)
        }
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#
        .replace("__SELECTED__", selected);
        let iced = rawr_iced::facade().unwrap();
        let value = iced.export_type("Element").unwrap();
        let facade = branch_projection_facade(value);
        let mut engine = Engine::new();
        engine.register_host_facade(iced).unwrap();
        engine.register_host_facade(facade.clone()).unwrap();
        let descriptor = engine.compile_application_descriptor(&source).unwrap();
        let engine = Shared::new(engine);
        let launch = engine.launch_application(descriptor).unwrap();

        let left_calls = Arc::new(AtomicUsize::new(0));
        let right_calls = Arc::new(AtomicUsize::new(0));
        let observed_left = left_calls.clone();
        let observed_right = right_calls.clone();
        let mut binding = host::runtime::Builder::new(facade);
        binding
            .bind_type_attestation("branch_projection::Left", |value, expected| {
                let descriptor = value.read_lock::<LeftDescriptor>()?;
                expected.attest_witness(Some(&descriptor.witness))
            })
            .unwrap();
        binding
            .bind_type_attestation("branch_projection::Right", |value, expected| {
                let descriptor = value.read_lock::<RightDescriptor>()?;
                expected.attest_witness(Some(&descriptor.witness))
            })
            .unwrap();
        binding
            .bind_with_context("branch_projection::left", |call, args| {
                let _ = args[0].take();
                let witness = call.output_witness().map_err(|error| {
                    rawr::EvalAltResult::ErrorRuntime(
                        error.to_string().into(),
                        rawr::Position::NONE,
                    )
                })?;
                Ok(Dynamic::from(LeftDescriptor { witness }))
            })
            .unwrap();
        binding
            .bind_with_context("branch_projection::right", |call, args| {
                let _ = args[0].take();
                let witness = call.output_witness().map_err(|error| {
                    rawr::EvalAltResult::ErrorRuntime(
                        error.to_string().into(),
                        rawr::Position::NONE,
                    )
                })?;
                Ok(Dynamic::from(RightDescriptor { witness }))
            })
            .unwrap();
        binding
            .bind_projection("branch_projection::Left", "iced::Element", move |_| {
                observed_left.fetch_add(1, Ordering::SeqCst);
                Err(rawr::EvalAltResult::ErrorRuntime(
                    "selected left projection failed".into(),
                    rawr::Position::NONE,
                )
                .into())
            })
            .unwrap();
        binding
            .bind_projection("branch_projection::Right", "iced::Element", move |_| {
                observed_right.fetch_add(1, Ordering::SeqCst);
                Err(rawr::EvalAltResult::ErrorRuntime(
                    "selected right projection failed".into(),
                    rawr::Position::NONE,
                )
                .into())
            })
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        for facade in rawr_iced::runtime_facades().unwrap() {
            registry.install(facade).unwrap();
        }
        registry.install(binding.finish()).unwrap();
        let adapter = program::Adapter::boot(launch.bind(registry).unwrap()).unwrap();

        let expected_left = usize::from(selected == "true");
        let expected_right = usize::from(selected == "false");
        assert_eq!(left_calls.load(Ordering::SeqCst), expected_left);
        assert_eq!(right_calls.load(Ordering::SeqCst), expected_right);
        assert_eq!(adapter.diagnostics().len(), 1);
        let diagnostic = &adapter.diagnostics()[0];
        assert_eq!(diagnostic.stage(), rawr_iced::diagnostic::Stage::View);
        assert!(diagnostic.message().contains("selected"));
        let (line, statement) = source
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(expression))
            .unwrap();
        let column = statement.find(expression).unwrap();
        assert_eq!(diagnostic.report().labels().len(), 1);
        assert_eq!(diagnostic.report().labels()[0].source().text(), source);
        assert_eq!(
            diagnostic.report().labels()[0].span(),
            rawr::Span::new(
                rawr::Position::new((line + 1) as u16, (column + 1) as u16),
                rawr::Position::new((line + 1) as u16, (column + expression.len()) as u16),
            )
        );
    }
}

#[test]
fn list_grammar_accepts_empty_text_only_and_trailing_comma_columns() {
    for body in ["column![]", "column![text(42),]"] {
        compile_probe(body).unwrap();
    }
}

#[test]
fn intrinsic_evaluation_requires_a_phase_four_runtime_binding() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let executable = engine
        .compile_executable(
            r#"
use iced::widget::{Column, column, text};
#[derive(Debug, Clone, Copy)]
enum Message { Tick }
pub fn probe() -> Column<'_, Message> {
    column![text(42)]
}
"#,
        )
        .unwrap();
    let probe = executable
        .items()
        .find(|item| item.key().name() == "probe")
        .unwrap()
        .clone();
    let authority = engine.authorize(executable).unwrap();
    let error = authority.invoke(&probe, &mut []).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("missing runtime binding for `iced::widget::text`"),
        "{error}"
    );
}

#[test]
fn intrinsic_macro_errors_are_positioned_and_name_the_registered_item() {
    for (body, expected_item, expected_reason) in [
        (
            "column![text(1) text(2)]",
            "iced::widget::column",
            "separate the elements with commas",
        ),
        (
            "button![text(1)]",
            "iced::widget::button",
            "`button` is a function here; write `button(..)`",
        ),
        (
            "column! text(1)",
            "iced::widget::column",
            "write `column![..]` with square brackets",
        ),
        (
            "missing![text(1)]",
            "missing",
            "no macro of this name is in scope",
        ),
        ("column![42]", "iced::Element", "Type mismatch"),
    ] {
        let error = compile_probe(body).unwrap_err();
        assert!(!error.primary_span().is_none(), "{error}");
        assert!(error.to_string().contains(expected_item), "{error}");
        assert!(error.to_string().contains(expected_reason), "{error}");
    }

    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let error = engine.compile("use iced::widget::missing;").unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::UnknownImport { path, .. })
            if path == "iced::widget::missing"
    ));
    assert!(!error.primary_span().is_none());
}

#[test]
fn column_is_a_macro_and_a_function_as_in_iced() {
    for body in [
        "column![text(1), button(\"ok\").on_press(Message::Tick)]",
        "column(vec![text(1), text(2)])",
        "column([text(1), text(2)])",
        "column((0..3).map(|i| text(i)))",
        "iced::widget::column(vec![text(1)])",
        "iced::widget::column![text(1)]",
        "column!(text(1), text(2))",
        "column! { text(1) }",
        "Column::new().push(text(1)).push(button(\"ok\"))",
        "Column::with_children(vec![text(1)])",
        "iced::widget::Column::from_vec(vec![text(1)])",
        "column(vec![text(1)]).extend([text(2), text(3)])",
        "let mut children = Vec::new(); for i in [1, 2, 3].into_iter() { children.push(text(i)); } Column::from_vec(children)",
    ] {
        compile_probe(body).unwrap_or_else(|error| panic!("{body}: {error}"));
    }
    let error = compile_probe("column(text(1))").unwrap_err();
    assert!(error.to_string().contains("Type mismatch"), "{error}");
}

#[test]
fn facade_calls_and_owned_methods_check_arity_and_types() {
    compile_probe("column![button(\"ok\").on_press(Message::Tick).padding(20)]").unwrap();

    for (body, expected) in [
        (
            "button().on_press(Message::Tick)",
            "no form of it takes ()",
        ),
        (
            "column![button(\"ok\").on_press(Message::Tick)].padding()",
            "no form of it takes ()",
        ),
        (
            "column![button(\"ok\").on_press(Message::Tick)].padding(true)",
            "no form of it takes (bool)",
        ),
    ] {
        let error = compile_probe(body).unwrap_err();
        assert!(!error.primary_span().is_none(), "{error}");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn facade_generic_requirements_apply_to_inferred_operation_types() {
    let mut builder = host::facade::Builder::new("ui", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut signature = host::signature::Builder::new();
    signature
        .type_parameter("Value", &[capability::Kind::Clone])
        .unwrap();
    signature.parameter(host::type_ref::Type::parameter("Value").unwrap());
    signature.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    builder
        .item(host::item::Definition::function("consume", signature.finish()).unwrap())
        .unwrap();

    let mut engine = Engine::new();
    engine
        .register_host_facade(builder.finish().unwrap())
        .unwrap();
    let error = engine
        .compile(
            r#"
use ui::consume;
struct Opaque { value: i64 }
pub fn probe(value: Opaque) -> i64 {
    consume(value)
}
"#,
        )
        .unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::MissingCapability { capability, .. })
            if capability == "Clone"
    ));
    assert!(error.to_string().contains("ui::consume"));
    assert!(!error.primary_span().is_none());
}

#[test]
fn ordinary_legacy_bang_calls_keep_their_capturing_semantics() {
    let engine = Engine::new();
    let answer = engine
        .eval::<i64>(
            r#"
fn add(y) { x += y; x }
let x = 40;
add!(1) + x
"#,
        )
        .unwrap();
    assert_eq!(answer, 82);
}

#[test]
#[cfg(not(feature = "no_optimize"))]
fn intrinsic_support_survives_ast_clone_merge_and_reoptimization() {
    use rawr::OptimizationLevel;

    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let ast = engine.compile(probe_source("column![text(42)]")).unwrap();

    assert_missing_binding(&engine, &ast.clone_functions_only());
    let ordinary = engine.compile("fn unrelated() { 42 }").unwrap();
    assert_missing_binding(&engine, &ordinary.merge(&ast));
    for level in [OptimizationLevel::Simple, OptimizationLevel::Full] {
        let optimized = engine.optimize_ast(&Scope::new(), ast.clone(), level);
        assert_missing_binding(&engine, &optimized);
    }
}

#[test]
#[cfg(feature = "grain")]
fn grain_artifacts_reject_retained_intrinsic_runtime_support() {
    use rawr::grain::format::WriteError;
    use rawr::grain::Compiler;

    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let ast = engine.compile(probe_source("column![text(42)]")).unwrap();
    let program = Compiler::new().compile(&ast);

    assert!(matches!(
        program.write(),
        Err(WriteError::HasCompilerSupport)
    ));
}

fn assert_missing_binding(engine: &Engine, ast: &rawr::AST) {
    let error = engine
        .call_fn::<Dynamic>(&mut Scope::new(), ast, "probe", ())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("missing runtime binding for `iced::widget::text`"),
        "{error}"
    );
}

fn compile_probe(body: &str) -> Result<rawr::AST, ParseError> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.compile(probe_source(body))
}

fn compile_value_probe(body: &str) -> Result<rawr::AST, ParseError> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.compile(format!(
        r#"
use iced::Center;
use iced::Element;
use iced::widget::{{button, column, text}};
#[derive(Debug, Clone, Copy)]
enum Message {{ Increment, Decrement }}
pub fn probe() -> Element<'_, Message> {{
    {body}
}}
"#
    ))
}

fn probe_source(body: &str) -> String {
    format!(
        r#"
use iced::widget::{{Column, button, column, text}};
#[derive(Debug, Clone, Copy)]
enum Message {{ Tick }}
pub fn probe() -> Column<'_, Message> {{
    {body}
}}
"#
    )
}

fn branch_projection_facade(target: host::type_def::Export) -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("branch_projection", host::facade::Abi::new(1).unwrap())
            .unwrap();
    builder
        .type_::<LeftDescriptor>("Left", &["Message"], &[])
        .unwrap();
    builder
        .type_::<RightDescriptor>("Right", &["Message"], &[])
        .unwrap();
    builder
        .project_to("Left", target.clone(), &["Message"])
        .unwrap();
    builder.project_to("Right", target, &["Message"]).unwrap();

    for (name, output) in [("left", "Left"), ("right", "Right")] {
        let mut signature = host::signature::Builder::new();
        signature.type_parameter("Message", &[]).unwrap();
        signature.parameter(host::type_ref::Type::parameter("Message").unwrap());
        signature.output(
            host::type_ref::Type::applied(
                output,
                [host::type_ref::Type::parameter("Message").unwrap()],
            )
            .unwrap(),
        );
        builder
            .item(host::item::Definition::function(name, signature.finish()).unwrap())
            .unwrap();
    }
    builder.finish().unwrap()
}
