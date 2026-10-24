use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rawr::{host, Dynamic, Engine, Shared};
use rawr_iced::{program, runtime};

#[test]
fn live_form_controls_emit_exact_application_messages() {
    let mut adapter = boot(application_source());

    let text_messages = typewrite(&adapter, "rawr");
    assert_eq!(text_messages.len(), 4);
    for message in text_messages {
        assert_current(&adapter, &message);
        assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);
    }
    assert_text(&adapter, "rawr");

    let accepted = click(&adapter, "Accept changes");
    assert_current(&adapter, &accepted);
    assert_eq!(
        adapter.update(accepted).unwrap(),
        program::Delivery::Applied
    );
    assert_text(&adapter, "true");

    let mut toggler = boot(toggler_source());
    let toggled = click_at(&toggler, iced::Point::new(8.0, 8.0));
    assert_current(&toggler, &toggled);
    assert_eq!(toggler.update(toggled).unwrap(), program::Delivery::Applied);
    assert_text(&toggler, "true");
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn nested_form_control_maps_at_each_symbolic_boundary() {
    let mut adapter = boot(nested_source());
    let messages = typewrite_at(&adapter, "Nested input", "tea");

    assert_eq!(messages.len(), 3);
    for message in messages {
        assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);
    }

    assert_text(&adapter, "tea");
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn slider_emits_exact_nested_application_messages_with_integer_steps() {
    let mut adapter = boot(slider_source());
    let changed = click_at(&adapter, iced::Point::new(512.0, 8.0));

    assert_current(&adapter, &changed);
    assert_eq!(adapter.update(changed).unwrap(), program::Delivery::Applied);
    assert_text(&adapter, "49");
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn mapper_shape_errors_are_rejected_before_runtime_binding() {
    for (mapper, expected) in [
        ("wrong_text_input", "bool"),
        ("wrong_text_output", "Other"),
        ("wrong_toggle", "String"),
        ("wrong_slider_input", "i64"),
        ("wrong_slider_output", "Other"),
    ] {
        let source = invalid_mapper_source(mapper);
        let Err(error) = compiler().compile_application_descriptor(&source) else {
            panic!("invalid mapper `{mapper}` unexpectedly compiled");
        };
        assert!(
            error.to_string().contains(expected),
            "mapper `{mapper}` diagnostic did not contain `{expected}`: {error}"
        );
    }
}

#[test]
fn slider_static_operand_errors_and_runtime_constraints_retain_original_view_evidence() {
    for (control, expected) in [
        ("slider(0..=\"100\", 0, Message::Changed)", "found 'String'"),
        ("slider(0..=100, false, Message::Changed)", "i64"),
    ] {
        let source = invalid_slider_source(control);
        let Err(error) = compiler().compile_application_descriptor(&source) else {
            panic!("invalid slider `{control}` unexpectedly compiled");
        };
        assert!(error.to_string().contains(expected), "{error}");
        assert!(!error.primary_span().is_none());
    }

    for (control, expected) in [
        (
            "slider(100..=0, 50, Message::Changed)",
            "ascending inclusive range",
        ),
        (
            "slider(0..=100, 50, Message::Changed).step(0)",
            "positive value",
        ),
        (
            "slider(0..=100, 50, Message::Changed).step(-1)",
            "positive value",
        ),
    ] {
        let source = invalid_slider_source(control);
        let (line_index, line) = source
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(control))
            .expect("invalid slider source must retain the complete view expression");
        let column = line
            .find(control)
            .expect("located view line must contain the slider expression");
        let expression = rawr::Span::new(
            rawr::Position::new((line_index + 1) as u16, (column + 1) as u16),
            rawr::Position::new(
                (line_index + 1) as u16,
                (column + control.chars().count()) as u16,
            ),
        );
        let engine = compiler();
        let descriptor = engine.compile_application_descriptor(&source).unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let bound = launch.bind(registry()).unwrap();
        let declaration = bound
            .callback_evidence(bound.view().callback())
            .expect("bound view must retain its source declaration")
            .declaration();
        let adapter = program::Adapter::boot(bound).unwrap();
        assert_eq!(adapter.diagnostics().len(), 1);
        let report = adapter.diagnostics()[0].report();
        assert!(report.message().contains(expected), "{}", report.message());
        assert_eq!(report.labels().len(), 1);
        assert_eq!(declaration.start().line(), expression.start().line());
        assert_eq!(report.labels()[0].span(), expression);
        assert_eq!(report.labels()[0].message(), None);
        assert_eq!(report.labels()[0].source().text(), source);
    }

    for control in [
        "slider(5..=5, 5, Message::Changed).step(1)",
        "slider(0..=100, 200, Message::Changed).step(1)",
    ] {
        let adapter = boot(&invalid_slider_source(control));
        assert!(
            adapter.diagnostics().is_empty(),
            "valid pinned iced edge `{control}` produced a diagnostic"
        );
    }
}

#[test]
fn slider_mapper_throw_is_diagnostic_only_for_the_current_owner() {
    let mut current = boot(throwing_slider_mapper_source());
    let current_message = click_at(&current, iced::Point::new(512.0, 8.0));
    let error = current.update(current_message).unwrap_err();
    assert!(matches!(error, program::Error::Mapping(_)));
    assert!(error.to_string().contains("slider mapper failed"));
    assert_eq!(current.diagnostics().len(), 1);

    let old = boot(throwing_slider_mapper_source());
    let stale = click_at(&old, iced::Point::new(512.0, 8.0));
    let mut replacement = boot(throwing_slider_mapper_source());
    let transition = replacement.update(stale).unwrap();
    assert_eq!(transition.delivery(), program::Delivery::Dropped);
    assert!(replacement.diagnostics().is_empty());
}

#[test]
fn mapper_throw_is_diagnostic_only_for_the_current_owner() {
    let mut current = boot(throwing_mapper_source());
    let current_message = typewrite(&current, "x").pop().unwrap();
    let error = current.update(current_message).unwrap_err();
    assert!(matches!(error, program::Error::Mapping(_)));
    assert!(error.to_string().contains("input mapper failed"));
    assert_eq!(current.diagnostics().len(), 1);

    let old = boot(throwing_mapper_source());
    let stale = typewrite(&old, "x").pop().unwrap();
    let mut replacement = boot(throwing_mapper_source());
    let transition = replacement.update(stale).unwrap();
    assert_eq!(transition.delivery(), program::Delivery::Dropped);
    assert!(replacement.diagnostics().is_empty());
}

#[test]
fn refreshed_text_input_reuses_retained_iced_tree_state() {
    use iced::advanced::widget::operation::Focusable;
    use iced::advanced::widget::{Id, Operation, Widget as _};

    /// Focuses each focusable widget it visits, or only records whether
    /// one is focused.
    struct Focus {
        focus: bool,
        focused: bool,
    }

    impl Operation for Focus {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }

        fn focusable(
            &mut self,
            _id: Option<&Id>,
            _bounds: iced::Rectangle,
            state: &mut dyn Focusable,
        ) {
            if self.focus {
                state.focus();
            }
            self.focused |= state.is_focused();
        }
    }

    let renderer = iced::futures::executor::block_on(
        <iced::Renderer as iced::advanced::renderer::Headless>::new(
            iced::advanced::renderer::Settings::default(),
            Some("tiny-skia"),
        ),
    )
    .expect("the pinned tiny-skia renderer must be available");
    let layout = iced::advanced::Layout::new(iced::Size::ZERO);
    let viewport = iced::Rectangle::default();
    let adapter = boot(identity_source());
    let context = adapter.context();
    let mut element = adapter.display().lower(&context);
    let mut tree = iced::advanced::widget::Tree::new(&element);
    let mut focus = Focus {
        focus: true,
        focused: false,
    };
    element.operate(&mut tree, layout, &viewport, &renderer, &mut focus);
    assert!(focus.focused);

    element = adapter.display().lower(&context);
    tree.diff(&mut element);

    let mut focused = Focus {
        focus: false,
        focused: false,
    };
    element.operate(&mut tree, layout, &viewport, &renderer, &mut focused);
    assert!(focused.focused);
}

#[test]
fn tab_row_and_long_scroll_surface_layout_at_a_small_window_size() {
    let adapter = boot(shell_surface_source());
    assert!(adapter.diagnostics().is_empty());
    let mut simulator = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(360.0, 240.0),
        adapter.view(),
    );

    assert!(simulator.find("Start").unwrap().visible_bounds().is_some());
    assert!(simulator
        .find("Choices")
        .unwrap()
        .visible_bounds()
        .is_some());
    assert!(simulator
        .find("Scroll target")
        .unwrap()
        .visible_bounds()
        .is_none());

    let surface = simulator
        .find("Tour surface")
        .unwrap()
        .visible_bounds()
        .expect("the scrollable must expose its initial content in the viewport");
    simulator.point_at(surface.center());
    let statuses = simulator.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -700.0 },
    })]);

    assert_eq!(statuses, [iced::event::Status::Captured]);
    // iced's scrollable smooths wheel scrolling over time; let it settle.
    let now = iced::time::Instant::now();
    let _ = simulator.simulate([now, now + iced::time::Duration::from_secs(2)].map(|at| {
        iced::Event::Window(iced::window::Event::RedrawRequested(at))
    }));
    assert!(simulator
        .find("Scroll target")
        .unwrap()
        .visible_bounds()
        .is_some());
    assert!(simulator.find("Start").unwrap().visible_bounds().is_some());
    assert!(simulator
        .find("Choices")
        .unwrap()
        .visible_bounds()
        .is_some());
}

#[test]
fn title_failure_at_boot_uses_the_shell_fallback_and_exact_diagnostic() {
    let adapter = boot(failing_title_source());
    assert_eq!(adapter.title(), "Rawr application");
    assert_eq!(adapter.last_good_title(), None);
    assert_eq!(adapter.diagnostics().len(), 1);
    assert!(adapter.diagnostics()[0]
        .message()
        .contains("title evaluation failed"));
}

#[test]
fn current_title_failure_retains_last_good_and_recovers_after_valid_state() {
    let source = state_title_failure_source();
    let mut adapter = boot_with_title_guard(source);
    assert_eq!(adapter.title(), "Ready 10");

    let broken = click(&adapter, "Break title");
    assert_eq!(adapter.update(broken).unwrap(), program::Delivery::Applied);
    assert_eq!(adapter.title(), "Ready 10");
    assert_eq!(adapter.last_good_title(), Some("Ready 10"));
    assert_eq!(adapter.diagnostics().len(), 1);
    let diagnostic = &adapter.diagnostics()[0];
    assert_eq!(diagnostic.stage(), rawr_iced::diagnostic::Stage::Title);
    assert_eq!(
        diagnostic.report().code(),
        rawr::diagnostic::code::RUNTIME_THROW
    );
    assert_eq!(diagnostic.report().labels().len(), 1);
    assert_eq!(diagnostic.report().labels()[0].source().text(), source);
    let (line, declaration) = source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("fn title"))
        .expect("the fixture must retain its title declaration");
    let expression = "render_title(self.divisor)";
    let column = declaration
        .find(expression)
        .expect("the title declaration must retain its bound expression");
    assert_eq!(
        diagnostic.report().labels()[0].span(),
        rawr::Span::new(
            rawr::Position::new((line + 1) as u16, (column + 1) as u16),
            rawr::Position::new((line + 1) as u16, (column + expression.len()) as u16,),
        )
    );

    let recovered = click(&adapter, "Recover title");
    assert_eq!(
        adapter.update(recovered).unwrap(),
        program::Delivery::Applied
    );
    assert_eq!(adapter.title(), "Ready 5");
    assert_eq!(adapter.last_good_title(), Some("Ready 5"));
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn selected_if_branch_intrinsic_failure_retains_its_exact_source_expression() {
    let source = r#"
use branch_test::fail_label;
use iced::Element;
use iced::widget::{button, column, text};

struct State { fail: bool }
#[derive(Debug, Clone, Copy)] enum Message { Fail }

impl State {
    fn new() -> State { State { fail: false } }
    fn update(&mut self, message: Message) { self.fail = true }
    fn view(&self) -> Element<Message> {
        let label = if self.fail {
            let input = 1;
            fail_label(input)
        } else {
            "safe"
        };
        column![text(label), button("Fail branch").on_press(Message::Fail)]
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#;
    let mut facade =
        host::facade::Builder::new("branch_test", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut signature = host::signature::Builder::new();
    signature.parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    signature.output(host::type_ref::Type::builtin(
        host::type_ref::Builtin::String,
    ));
    facade
        .item(host::item::Definition::function("fail_label", signature.finish()).unwrap())
        .unwrap();
    let facade = facade.finish().unwrap();

    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(facade.clone()).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut binding = host::runtime::Builder::new(facade);
    binding
        .bind("branch_test::fail_label", move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Err(rawr::EvalAltResult::ErrorRuntime(
                "selected intrinsic failed".into(),
                rawr::Position::NONE,
            )
            .into())
        })
        .unwrap();
    let mut registry = registry();
    registry.install(binding.finish()).unwrap();
    let mut adapter = program::Adapter::boot(launch.bind(registry).unwrap()).unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(adapter.diagnostics().is_empty());
    assert_text(&adapter, "safe");
    let fail = click(&adapter, "Fail branch");
    assert_eq!(adapter.update(fail).unwrap(), program::Delivery::Applied);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(adapter.diagnostics().len(), 1);

    let diagnostic = &adapter.diagnostics()[0];
    assert_eq!(diagnostic.stage(), rawr_iced::diagnostic::Stage::View);
    assert!(diagnostic.message().contains("selected intrinsic failed"));
    let expression = "fail_label(input)";
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

#[test]
fn same_line_guard_failures_retain_distinct_origins() {
    let source = r#"
use origin_test::{left, right};
use iced::Element;
use iced::widget::text;

enum Mode { Run }
struct State { mode: Mode }
#[derive(Debug, Clone, Copy)] enum Message { Idle }

impl State {
    fn new() -> State { State { mode: Mode::Run } }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> {
        let label = match self.mode {
            Mode::Run if left(true) == right(true) => "matched",
            Mode::Run => "fallback",
        };
        text(label)
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#;

    for (failing, expression, expected_calls) in [
        ("left", "left(true)", [1, 0]),
        ("right", "right(true)", [1, 1]),
    ] {
        let facade = origin_facade();
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        engine.register_host_facade(facade.clone()).unwrap();
        let descriptor = engine.compile_application_descriptor(source).unwrap();
        let engine = Shared::new(engine);
        let launch = engine.launch_application(descriptor).unwrap();
        let left_calls = Arc::new(AtomicUsize::new(0));
        let right_calls = Arc::new(AtomicUsize::new(0));
        let mut binding = host::runtime::Builder::new(facade);
        for (name, calls) in [("left", left_calls.clone()), ("right", right_calls.clone())] {
            let fails = name == failing;
            binding
                .bind(format!("origin_test::{name}"), move |args| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let value = args[0].as_bool()?;
                    if fails {
                        Err(rawr::EvalAltResult::ErrorRuntime(
                            format!("{name} guard failed").into(),
                            rawr::Position::NONE,
                        )
                        .into())
                    } else {
                        Ok(Dynamic::from(value))
                    }
                })
                .unwrap();
        }
        let mut registry = registry();
        registry.install(binding.finish()).unwrap();
        let adapter = program::Adapter::boot(launch.bind(registry).unwrap()).unwrap();

        assert_eq!(
            [
                left_calls.load(Ordering::SeqCst),
                right_calls.load(Ordering::SeqCst),
            ],
            expected_calls
        );
        assert_eq!(adapter.diagnostics().len(), 1);
        let diagnostic = &adapter.diagnostics()[0];
        assert!(diagnostic
            .message()
            .contains(&format!("{failing} guard failed")));
        assert_eq!(diagnostic.report().labels().len(), 1);
        assert_eq!(diagnostic.report().labels()[0].source().text(), source);
        assert_eq!(
            diagnostic.report().labels()[0].span(),
            expression_span(source, expression, "Mode::Run if")
        );
    }
}

#[test]
fn duplicate_nested_calls_keep_the_selected_origin() {
    let source = r#"
use origin_test::probe;
use iced::Element;
use iced::widget::text;

enum Mode { Run }
struct State { mode: Mode }
#[derive(Debug, Clone, Copy)] enum Message { Idle }

impl State {
    fn new() -> State { State { mode: Mode::Run } }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> {
        let before = probe(true);
        let label = if before {
            match self.mode {
                Mode::Run if probe(true) => "nested",
                Mode::Run => "fallback",
            }
        } else {
            "outside"
        };
        text(label)
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#;
    let facade = origin_facade();
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(facade.clone()).unwrap();
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let engine = Shared::new(engine);
    let launch = engine.launch_application(descriptor).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut binding = host::runtime::Builder::new(facade);
    binding
        .bind("origin_test::probe", move |args| {
            let call = observed.fetch_add(1, Ordering::SeqCst);
            if call == 1 {
                Err(rawr::EvalAltResult::ErrorRuntime(
                    "nested duplicate failed".into(),
                    rawr::Position::NONE,
                )
                .into())
            } else {
                Ok(Dynamic::from(args[0].as_bool()?))
            }
        })
        .unwrap();
    let mut registry = registry();
    registry.install(binding.finish()).unwrap();
    let adapter = program::Adapter::boot(launch.bind(registry).unwrap()).unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(adapter.diagnostics().len(), 1);
    let diagnostic = &adapter.diagnostics()[0];
    assert!(diagnostic.message().contains("nested duplicate failed"));
    assert_eq!(diagnostic.report().labels().len(), 1);
    assert_eq!(diagnostic.report().labels()[0].source().text(), source);
    assert_eq!(
        diagnostic.report().labels()[0].span(),
        expression_span(source, "probe(true)", "Mode::Run if")
    );
}

#[test]
fn normalized_condition_prelude_executes_once() {
    let source = r#"
use origin_test::condition;
use iced::Element;
use iced::widget::text;

struct State {}
#[derive(Debug, Clone, Copy)] enum Message { Idle }

impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> {
        let label = if condition(true, true) { "selected" } else { "unselected" };
        text(label)
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#;
    for fails in [false, true] {
        let facade = origin_facade();
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        engine.register_host_facade(facade.clone()).unwrap();
        let descriptor = engine.compile_application_descriptor(source).unwrap();
        let engine = Shared::new(engine);
        let launch = engine.launch_application(descriptor).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut binding = host::runtime::Builder::new(facade);
        binding
            .bind("origin_test::condition", move |args| {
                observed.fetch_add(1, Ordering::SeqCst);
                if fails {
                    Err(rawr::EvalAltResult::ErrorRuntime(
                        "normalized condition failed".into(),
                        rawr::Position::NONE,
                    )
                    .into())
                } else {
                    Ok(Dynamic::from(args[0].as_bool()? && args[1].as_bool()?))
                }
            })
            .unwrap();
        let mut registry = registry();
        registry.install(binding.finish()).unwrap();
        let adapter = program::Adapter::boot(launch.bind(registry).unwrap()).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        if fails {
            assert_eq!(adapter.diagnostics().len(), 1);
            let diagnostic = &adapter.diagnostics()[0];
            assert!(diagnostic.message().contains("normalized condition failed"));
            assert_eq!(diagnostic.report().labels().len(), 1);
            assert_eq!(diagnostic.report().labels()[0].source().text(), source);
            assert_eq!(
                diagnostic.report().labels()[0].span(),
                expression_span(source, "condition(true, true)", "let label")
            );
        } else {
            assert_text(&adapter, "selected");
            assert!(adapter.diagnostics().is_empty());
        }
    }
}

#[test]
fn stale_match_subject_schema_reports_the_exact_application_scrutinee() {
    let source = r#"
use iced::Element;
use iced::widget::text;

struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Ready(i64) }

impl State {
    fn new() -> State { State { value: 0 } }
    fn update(&mut self, message: Message) {
        let value = match message {
            Message::Ready(value) => value,
        };
        self.value = value;
    }
    fn view(&self) -> Element<Message> {
        text(self.value)
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#;
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let stale = engine
        .eval::<Dynamic>("enum Message { Ready } Message::Ready")
        .unwrap();
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let engine = Shared::new(engine);
    let launch = engine.launch_application(descriptor).unwrap();
    let bound = launch.bind(registry()).unwrap();
    let (state, task) = bound.invoke_boot().unwrap().into_parts();
    assert!(task.is_none());
    let error = bound.invoke_update(&mut [state, stale]).unwrap_err();

    assert!(error.to_string().contains("stale schema"), "{error}");
    let evidence = bound
        .callback_evidence(bound.update().callback())
        .expect("the application update must retain its source evidence");
    let report = evidence.program_report(&error).unwrap();
    assert_eq!(report.labels().len(), 1);
    assert_eq!(report.labels()[0].source().text(), source);
    let scrutinee = expression_span(source, "message", "match message");
    assert_eq!(
        report.labels()[0].span(),
        rawr::Span::new(scrutinee.start(), scrutinee.start())
    );
}

fn boot(source: &str) -> program::Adapter {
    let engine = compiler();
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let bound = launch.bind(registry()).unwrap();
    program::Adapter::boot(bound).unwrap()
}

fn compiler() -> Shared<Engine> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    Shared::new(engine)
}

fn boot_with_title_guard(source: &str) -> program::Adapter {
    let facade = title_guard_facade();
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(facade.clone()).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = registry();
    let mut binding = host::runtime::Builder::new(facade);
    binding
        .bind("test::render_title", |args| {
            let divisor = args[0].as_int()?;
            if divisor == 0 {
                Err(rawr::EvalAltResult::ErrorRuntime(
                    "current title failed".into(),
                    rawr::Position::NONE,
                )
                .into())
            } else {
                Ok(rawr::Dynamic::from(format!("Ready {}", 10 / divisor)))
            }
        })
        .unwrap();
    registry.install(binding.finish()).unwrap();
    program::Adapter::boot(launch.bind(registry).unwrap()).unwrap()
}

fn title_guard_facade() -> host::facade::Definition {
    let mut facade =
        host::facade::Builder::new("test", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut signature = host::signature::Builder::new();
    signature.parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    signature.output(host::type_ref::Type::builtin(
        host::type_ref::Builtin::String,
    ));
    facade
        .item(host::item::Definition::function("render_title", signature.finish()).unwrap())
        .unwrap();
    facade.finish().unwrap()
}

fn origin_facade() -> host::facade::Definition {
    let mut facade =
        host::facade::Builder::new("origin_test", host::facade::Abi::new(1).unwrap()).unwrap();
    let bool_type = || host::type_ref::Type::builtin(host::type_ref::Builtin::Bool);
    for name in ["left", "right", "probe"] {
        let mut signature = host::signature::Builder::new();
        signature.parameter(bool_type());
        signature.output(bool_type());
        facade
            .item(host::item::Definition::function(name, signature.finish()).unwrap())
            .unwrap();
    }
    let mut condition = host::signature::Builder::new();
    condition.parameter(bool_type());
    condition.parameter(bool_type());
    condition.output(bool_type());
    facade
        .item(host::item::Definition::function("condition", condition.finish()).unwrap())
        .unwrap();
    facade.finish().unwrap()
}

fn expression_span(source: &str, expression: &str, line_marker: &str) -> rawr::Span {
    let (line, statement) = source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(line_marker) && line.contains(expression))
        .expect("the fixture must retain the selected expression on its marked line");
    let column = statement
        .find(expression)
        .expect("the marked fixture line must retain the selected expression");
    rawr::Span::new(
        rawr::Position::new((line + 1) as u16, (column + 1) as u16),
        rawr::Position::new((line + 1) as u16, (column + expression.len()) as u16),
    )
}

fn registry() -> host::runtime::Registry {
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry
}

fn typewrite(adapter: &program::Adapter, text: &str) -> Vec<runtime::Message> {
    typewrite_at(adapter, "Type a message", text)
}

fn typewrite_at(
    adapter: &program::Adapter,
    placeholder: &str,
    text: &str,
) -> Vec<runtime::Message> {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(placeholder).unwrap();
    let _ = simulator.typewrite(text);
    simulator.into_messages().collect()
}

fn click(adapter: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(label).unwrap();
    let mut messages = simulator.into_messages();
    let message = messages.next().unwrap();
    assert!(messages.next().is_none());
    message
}

fn click_at(adapter: &program::Adapter, point: iced::Point) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.point_at(point);
    let _ = simulator.simulate(iced_test::simulator::click());
    let mut messages = simulator.into_messages();
    let message = messages.next().unwrap();
    assert!(messages.next().is_none());
    message
}

fn assert_current(adapter: &program::Adapter, message: &runtime::Message) {
    assert_eq!(message.instance(), adapter.instance());
    assert_eq!(message.generation(), adapter.generation());
}

fn assert_text(adapter: &program::Adapter, text: &str) {
    iced_test::simulator(adapter.view()).find(text).unwrap();
}

fn application_source() -> &'static str {
    r#"
use iced::Padding;
use iced::Element;
use iced::widget::{checkbox, column, text, text_input, toggler};

#[derive(Default)]
struct State { value: String, accepted: bool, private_mode: bool }

#[derive(Debug, Clone)]
enum Message { Input(String), Accepted(bool), Private(bool) }

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Input(value) => self.value = value,
            Message::Accepted(accepted) => self.accepted = accepted,
            Message::Private(private_mode) => self.private_mode = private_mode,
        }
    }

    fn view(&self) -> Element<'_, Message> {
        column![
            text_input("Type a message", &self.value)
                .on_input(Message::Input)
                .padding(Padding::new(12))
                .size(18)
                .secure(false),
            text_input("Integer padding", &self.value)
                .on_input(Message::Input)
                .padding(12),
            text(&self.value),
            checkbox(self.accepted)
                .label("Accept changes")
                .on_toggle(Message::Accepted),
            text(self.accepted),
            toggler(self.private_mode)
                .label("Private mode")
                .on_toggle(Message::Private),
            text("Private mode enabled"),
        ]
    }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn nested_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::{column, text, text_input};

#[derive(Default)] struct State { value: String }
#[derive(Debug, Clone)] enum Leaf { Changed(String) }
#[derive(Debug, Clone)] enum Child { Field(Leaf) }
#[derive(Debug, Clone)] enum Message { Editor(Child) }

pub fn field() -> Element<'_, Leaf> {
    text_input("Nested input", "").on_input(Leaf::Changed)
}

pub fn child() -> Element<'_, Child> {
    field().map(Child::Field)
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Editor(Child::Field(Leaf::Changed(value))) => self.value = value,
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let input = child().map(Message::Editor);
        column![input, text(&self.value)]
    }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn invalid_mapper_source(mapper: &str) -> String {
    let control = match mapper {
        "wrong_text_input" => {
            "text_input(\"Type a message\", \"\").on_input(Message::Toggle)".to_owned()
        }
        "wrong_text_output" => {
            "text_input(\"Type a message\", \"\").on_input(Other::Input)".to_owned()
        }
        "wrong_toggle" => "checkbox(false).on_toggle(Message::Input)".to_owned(),
        "wrong_slider_input" => "slider(0..=100, 0, Message::Toggle)".to_owned(),
        "wrong_slider_output" => "slider(0..=100, 0, Other::Changed)".to_owned(),
        _ => unreachable!("the test supplies one known invalid mapper"),
    };
    format!(
        r#"
use iced::Element;
use iced::widget::{{checkbox, slider, text_input}};
#[derive(Default)] struct State;
#[derive(Debug, Clone)] enum Message {{ Input(String), Toggle(bool), Changed(i64) }}
#[derive(Debug, Clone)] enum Other {{ Input(String), Changed(i64) }}
impl State {{
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{
        {control}
    }}
}}
pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
    )
}

fn invalid_slider_source(control: &str) -> String {
    format!(
        r#"
use iced::Element;
use iced::widget::slider;
#[derive(Default)] struct State;
#[derive(Debug, Clone)] enum Message {{ Changed(i64) }}
impl State {{
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{ {control} }}
}}
pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
    )
}

fn throwing_mapper_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text_input;
#[derive(Default)] struct State;
#[derive(Debug, Clone)] enum Message { Input(String) }
impl Message {
    fn map_input(value: String) -> Message { panic!("input mapper failed") }
}
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> {
        text_input("Type a message", "").on_input(Message::map_input)
    }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn toggler_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::{column, text, toggler};
#[derive(Default)] struct State { toggled: bool }
#[derive(Debug, Clone, Copy)] enum Message { Toggle(bool) }
impl State {
    fn update(&mut self, message: Message) {
        match message { Message::Toggle(toggled) => self.toggled = toggled }
    }
    fn view(&self) -> Element<'_, Message> {
        column![
            toggler(self.toggled).label("Private mode").on_toggle(Message::Toggle),
            text(self.toggled),
        ]
    }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn slider_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::{column, slider, text};
#[derive(Default)] struct State { value: i64 }
#[derive(Debug, Clone)] enum Leaf { Changed(i64) }
#[derive(Debug, Clone)] enum Child { Slider(Leaf) }
#[derive(Debug, Clone)] enum Message { Controls(Child) }

pub fn control(value: i64) -> Element<'_, Leaf> {
    slider(0..=100, value, Leaf::Changed).step(7)
}

pub fn child(value: i64) -> Element<'_, Child> {
    control(value).map(Child::Slider)
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Controls(Child::Slider(Leaf::Changed(value))) => self.value = value,
        }
    }
    fn view(&self) -> Element<'_, Message> {
        column![child(self.value).map(Message::Controls), text(self.value)]
    }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn throwing_slider_mapper_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::slider;
#[derive(Default)] struct State;
#[derive(Debug, Clone)] enum Message { Changed(i64) }
impl Message {
    fn map_slider(value: i64) -> Message { panic!("slider mapper failed") }
}
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> {
        slider(0..=100, 0, Message::map_slider)
    }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn identity_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text_input;
#[derive(Default)] struct State { value: String }
#[derive(Debug, Clone)] enum Message { Input(String) }
impl State {
    fn update(&mut self, message: Message) {
        match message { Message::Input(value) => self.value = value }
    }
    fn view(&self) -> Element<'_, Message> {
        text_input("Identity input", &self.value).on_input(Message::Input)
    }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn shell_surface_source() -> &'static str {
    r#"
use iced::{Center, Length, Padding};
use iced::Element;
use iced::widget::{button, column, container, row, scrollable, space, text};

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Start, Choices }

impl State {
    fn update(&mut self, message: Message) {}

    fn view(&self) -> Element<'_, Message> {
        let tabs = row![
            button("Start").padding(Padding::new(8)).on_press(Message::Start),
            button("Choices").padding(8).on_press(Message::Choices),
        ].spacing(8);
        let page = column![
            text("Tour surface").width(Length::Fill).align_x(Center),
            space().height(Length::Fixed(600)),
            text("Scroll target"),
        ].spacing(12).width(Length::Fill);
        container(column![tabs, scrollable(page)].spacing(12).width(Length::Fill))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
    }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn failing_title_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State {}
#[derive(Debug, Clone, Copy)] enum Message { Idle }

impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> { text("Fallback") }
    fn title(&self) -> String { panic!("title evaluation failed") }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .title(State::title)
        .run()
}
"#
}

fn state_title_failure_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::{button, column};
use test::render_title;

struct State { divisor: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Divisor(i64) }

impl State {
    fn new() -> State { State { divisor: 1 } }
    fn update(&mut self, message: Message) {
        match message { Message::Divisor(divisor) => self.divisor = divisor }
    }
    fn view(&self) -> Element<Message> {
        column![
            button("Break title").on_press(Message::Divisor(0)),
            button("Recover title").on_press(Message::Divisor(2)),
        ]
    }
    fn title(&self) -> String { render_title(self.divisor) }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .title(State::title)
        .run()
}
"#
}
