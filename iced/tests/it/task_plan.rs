use iced_test::futures::futures::StreamExt;
use rawr::{application, host, resolve, Dynamic, Engine, ParseErrorType, Shared};
use rawr_iced::{program, runtime};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn default_boot_and_unit_update_normalize_to_none() {
    let mut adapter = boot(include_str!("../../examples/application_counter.rawr"));

    assert!(adapter.startup_plan().is_none());
    let message = click(&adapter, "Increment");
    let transition = adapter.update(message).unwrap();
    assert_eq!(transition.delivery(), program::Delivery::Applied);
    assert!(transition.plan().is_none());
    assert_text(&adapter, "1");
}

#[test]
fn explicit_state_only_boot_normalizes_to_none() {
    let adapter = boot(&explicit_source("Counter", "()"));

    assert!(adapter.startup_plan().is_none());
    assert_text(&adapter, "7");
}

#[test]
fn explicit_done_boot_is_delivered_once_with_current_provenance() {
    let adapter = boot(&explicit_source(
        "(Counter, Task<Message>)",
        "Task<Message>",
    ));
    let instance = adapter.instance();
    let generation = adapter.generation();
    let application = program::Application::from_adapter(adapter);
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);

    let message =
        one_message(&mut state, startup).expect("Task::done must emit one startup message");
    assert_eq!(message.instance(), instance);
    assert_eq!(message.generation(), generation);
    let follow_up =
        <program::Application as iced::Program>::update(&application, &mut state, message);
    assert_text(&state, "8");

    let follow_up =
        one_message(&mut state, follow_up).expect("update Task::done must emit one message");
    assert_eq!(follow_up.instance(), instance);
    assert_eq!(follow_up.generation(), generation);
    let task = <program::Application as iced::Program>::update(&application, &mut state, follow_up);
    assert!(one_message(&mut state, task).is_some());
    assert_text(&state, "9");
}

#[test]
fn update_none_lowers_to_an_empty_native_task() {
    let adapter = boot(&explicit_source("Counter", "Task<Message>"));
    let application = program::Application::from_adapter(adapter);
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);
    assert!(one_message(&mut state, startup).is_none());

    let message = click(&state, "Increment");
    let task = <program::Application as iced::Program>::update(&application, &mut state, message);
    assert!(one_message(&mut state, task).is_none());
    assert_text(&state, "8");
}

#[test]
fn batch_chain_and_map_compose_exact_symbolic_messages() {
    let adapter = boot(composed_source());
    let instance = adapter.instance();
    let generation = adapter.generation();
    let application = program::Application::from_adapter(adapter);
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);

    let emitted = messages(&mut state, startup, 3);
    assert_eq!(emitted.len(), 3);
    assert!(emitted
        .iter()
        .all(|message| message.instance() == instance && message.generation() == generation));
    for message in emitted {
        let follow_up =
            <program::Application as iced::Program>::update(&application, &mut state, message);
        assert!(messages(&mut state, follow_up, 0).is_empty());
    }
    assert_text(&state, "3");
}

#[test]
fn discarded_none_is_allowed_but_every_potential_work_plan_is_required() {
    compile(&discard_source("Task::none();", "()"));
    compile(&discard_source("(Task::none());", "()"));
    compile(&contract_source("batch![]", ""));
    compile(&discard_source(
        "let plan = Task::done(Message::Increment);",
        "plan",
    ));

    for expression in [
        "Task::done(Message::Increment);",
        "make();",
        "batch![];",
        "batch![Task::none(), Task::done(Message::Increment)];",
        "Task::done(Message::Increment).chain(Task::none());",
        "Task::done(Message::Increment).map(identity);",
    ] {
        let error = compile_error(&discard_source(expression, "()"));
        let resolve::Error::UnusedValue { expression, .. } = error else {
            panic!("expected an unused-value error, found {error}");
        };
        assert!(!expression.is_none());
    }
}

#[test]
fn batch_and_map_reject_unproven_or_mismatched_message_types() {
    let mixed = contract_source(
        "batch![Task::done(Message::Increment), Task::done(Child::Tick)]",
        "pub fn identity(message: Message) -> Message { message }",
    );
    assert!(matches!(
        compile_error(&mixed),
        resolve::Error::TypeMismatch { .. }
    ));

    let heterogeneous_chain = contract_source(
        "Task::done(Message::Increment).chain(Task::done(Child::Tick))",
        "",
    );
    assert!(matches!(
        compile_error(&heterogeneous_chain),
        resolve::Error::TypeMismatch { .. }
    ));

    let wrong_input = contract_source(
        "Task::done(Message::Increment).map(child)",
        "pub fn child(message: Child) -> Message { Message::Increment }",
    );
    assert!(matches!(
        compile_error(&wrong_input),
        resolve::Error::TypeMismatch { .. }
    ));

    let wrong_output = contract_source(
        "Task::done(Message::Increment).map(to_child)",
        "pub fn to_child(message: Message) -> Child { Child::Tick }",
    );
    assert!(matches!(
        compile_error(&wrong_output),
        resolve::Error::TypeMismatch { .. }
    ));

    let foreign = contract_source("Task::done(Message::Increment).map(42)", "");
    assert!(matches!(
        compile_error(&foreign),
        resolve::Error::TypeMismatch { .. }
    ));

    let unknown = contract_source("Task::done(Message::Increment).map(missing)", "");
    assert!(matches!(
        compile_error(&unknown),
        resolve::Error::UnsupportedNominalSyntax { .. }
    ));
}

#[test]
fn foreign_delivery_is_dropped_without_a_follow_up_plan_or_diagnostic() {
    let mut adapter = boot(&explicit_source("Counter", "Task<Message>"));
    let current = click(&adapter, "Increment");
    let foreign = boot(&explicit_source("Counter", "Task<Message>"));
    let stale = foreign.context().message(current.payload().clone());
    let diagnostics = adapter.diagnostics().to_vec();

    let transition = adapter.update(stale).unwrap();
    assert_eq!(transition.delivery(), program::Delivery::Dropped);
    assert!(transition.plan().is_none());
    assert_eq!(adapter.diagnostics(), diagnostics);
    adapter.refresh_view().unwrap();
    assert_text(&adapter, "7");
    assert_eq!(adapter.diagnostics(), diagnostics);
}

#[test]
fn foreign_startup_done_is_dropped_without_mutation_or_diagnostic() {
    let source = explicit_source("(Counter, Task<Message>)", "Task<Message>");
    let mut adapter = boot(&source);
    let foreign = program::Application::from_adapter(boot(&source));
    let (mut foreign_state, startup) = <program::Application as iced::Program>::boot(&foreign);
    let stale =
        one_message(&mut foreign_state, startup).expect("foreign Task::done must emit one message");
    let diagnostics = adapter.diagnostics().to_vec();

    let transition = adapter.update(stale).unwrap();
    assert_eq!(transition.delivery(), program::Delivery::Dropped);
    assert!(transition.plan().is_none());
    assert_eq!(adapter.diagnostics(), diagnostics);
    adapter.refresh_view().unwrap();
    assert_text(&adapter, "7");
    assert_eq!(adapter.diagnostics(), diagnostics);
}

#[test]
fn startup_envelope_cannot_escape_through_an_ordinary_script_call() {
    let mut source = explicit_source("(Counter, Task<Message>)", "Task<Message>");
    source.push_str("\npub fn leak() -> (Counter, Task<Message>) { Counter::boot() }\n");
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();

    let error = engine.compile_application_descriptor(source).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("tuple-valued associated calls outside the explicit boot boundary"),
        "{error}"
    );
}

#[test]
fn startup_envelope_cannot_escape_through_generic_bound_invocation() {
    let source = explicit_source("(Counter, Task<Message>)", "Task<Message>");
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let bound = launch.bind(registry()).unwrap();
    let callback = match bound.boot() {
        application::Boot::Explicit { callback, .. } => callback.clone(),
        application::Boot::Default => panic!("source declares explicit boot"),
    };

    let error = bound.invoke(callback.item(), &mut []).unwrap_err();
    assert!(matches!(
        error,
        rawr::program::Error::ApplicationBoundary {
            boundary: "application boot",
            ..
        }
    ));
}

#[test]
fn perform_startup_lowers_once_and_delivers_its_exact_nominal_message() {
    let effects = effects_facade();
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(effects.clone()).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine
        .compile_application_descriptor(perform_source())
        .unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let adapter =
        program::Adapter::boot(launch.bind(operation_registry(effects)).unwrap()).unwrap();
    let application = program::Application::from_adapter(adapter);
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);

    let message =
        one_message(&mut state, startup).expect("the registered operation must emit once");
    let follow_up =
        <program::Application as iced::Program>::update(&application, &mut state, message);

    assert!(one_message(&mut state, follow_up).is_none());
    assert_text(&state, "14");
}

#[test]
fn perform_returned_from_update_lowers_at_the_same_host_edge() {
    let effects = effects_facade();
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(effects.clone()).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine
        .compile_application_descriptor(update_perform_source())
        .unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let adapter =
        program::Adapter::boot(launch.bind(operation_registry(effects)).unwrap()).unwrap();
    let application = program::Application::from_adapter(adapter);
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);
    assert!(one_message(&mut state, startup).is_none());

    let start = click(&state, "Load");
    let pending = <program::Application as iced::Program>::update(&application, &mut state, start);
    let loaded = one_message(&mut state, pending).expect("update operation must emit once");
    let follow_up =
        <program::Application as iced::Program>::update(&application, &mut state, loaded);

    assert!(one_message(&mut state, follow_up).is_none());
    assert_text(&state, "6");
}

#[test]
fn invalid_operation_output_reports_task_failure_without_invoking_the_mapper() {
    let effects = effects_facade();
    let starts = Arc::new(AtomicUsize::new(0));
    let adapter = boot_with_operation(
        &perform_source().replace(
            "pub fn loaded(value: i64) -> Message { Message::Loaded(value) }",
            "pub fn loaded(value: i64) -> Message { throw \"mapper must not run\" }",
        ),
        effects.clone(),
        operation_registry_with(effects, starts.clone(), Some(Dynamic::from(true))),
    );
    let application = program::Application::from_adapter(adapter);
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);

    let failure =
        one_message(&mut state, startup).expect("invalid output must report one task failure");
    let follow_up =
        <program::Application as iced::Program>::update(&application, &mut state, failure);

    assert!(one_message(&mut state, follow_up).is_none());
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(state.diagnostics().iter().any(|entry| {
        entry.stage() == rawr_iced::diagnostic::Stage::Task
            && entry.message().contains("invalid output")
            && !entry.message().contains("mapper must not run")
    }));
}

#[test]
fn mapper_failure_reports_task_diagnostic_after_one_factory_start() {
    let effects = effects_facade();
    let starts = Arc::new(AtomicUsize::new(0));
    let source = perform_source().replace(
        "pub fn loaded(value: i64) -> Message { Message::Loaded(value) }",
        "pub fn loaded(value: i64) -> Message { throw \"mapper failed\" }",
    );
    let adapter = boot_with_operation(
        &source,
        effects.clone(),
        operation_registry_with(effects, starts.clone(), None),
    );
    let application = program::Application::from_adapter(adapter);
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);

    let failure =
        one_message(&mut state, startup).expect("mapper failure must emit one task failure");
    let follow_up =
        <program::Application as iced::Program>::update(&application, &mut state, failure);

    assert!(one_message(&mut state, follow_up).is_none());
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(state.diagnostics().iter().any(|entry| {
        entry.stage() == rawr_iced::diagnostic::Stage::Task
            && entry.message().contains("mapper failed")
    }));
}

#[test]
fn perform_batch_chain_and_map_preserve_exact_issuer_and_composition() {
    let effects = effects_facade();
    let starts = Arc::new(AtomicUsize::new(0));
    let adapter = boot_with_operation(
        perform_composition_source(),
        effects.clone(),
        operation_registry_with(effects, starts.clone(), None),
    );
    let instance = adapter.instance();
    let generation = adapter.generation();
    let application = program::Application::from_adapter(adapter);
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);

    let emitted = messages(&mut state, startup, 3);
    assert_eq!(emitted.len(), 3);
    assert!(emitted
        .iter()
        .all(|message| message.instance() == instance && message.generation() == generation));
    for message in emitted {
        let follow_up =
            <program::Application as iced::Program>::update(&application, &mut state, message);
        assert!(one_message(&mut state, follow_up).is_none());
    }

    assert_eq!(starts.load(Ordering::SeqCst), 3);
    assert_text(&state, "12");
}

fn boot(source: &str) -> program::Adapter {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    program::Adapter::boot(launch.bind(registry()).unwrap()).unwrap()
}

fn registry() -> host::runtime::Registry {
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry
}

fn operation_registry(effects: host::facade::Definition) -> host::runtime::Registry {
    operation_registry_with(effects, Arc::new(AtomicUsize::new(0)), None)
}

fn operation_registry_with(
    effects: host::facade::Definition,
    starts: Arc<AtomicUsize>,
    output: Option<Dynamic>,
) -> host::runtime::Registry {
    let operation = effects
        .items()
        .find(|item| item.path() == "effects::load")
        .unwrap()
        .clone();
    let mut binding = host::runtime::Builder::new(effects);
    binding
        .bind_operation(
            &operation,
            move |arguments| {
                starts.fetch_add(1, Ordering::SeqCst);
                let output = output.clone();
                async move {
                    if let Some(output) = output {
                        return Ok::<_, ()>(output);
                    }
                    let mut values = arguments.into_values().into_vec();
                    let value = values.remove(0).as_int().map_err(|_| ())?;
                    Ok::<_, ()>(Dynamic::from(value * 2))
                }
            },
            |_| Dynamic::from(-1_i64),
        )
        .unwrap();
    let mut registry = registry();
    registry.install(binding.finish()).unwrap();
    registry
}

fn boot_with_operation(
    source: &str,
    effects: host::facade::Definition,
    registry: host::runtime::Registry,
) -> program::Adapter {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(effects).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    program::Adapter::boot(launch.bind(registry).unwrap()).unwrap()
}

fn effects_facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("effects", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut load = host::signature::Builder::new();
    load.parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    load.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    builder
        .item(host::item::Definition::operation("load", load.finish()).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

fn perform_source() -> &'static str {
    r#"
use effects::load;
use iced::Task;
use iced::widget::{Column, column, text};
pub fn main() -> iced::Result {
    iced::application(Counter::boot, Counter::update, Counter::view).run()
}
struct Counter { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Message { Loaded(i64) }
pub fn loaded(value: i64) -> Message { Message::Loaded(value) }
impl Counter {
    fn boot() -> (Counter, Task<Message>) {
        (Counter { value: 0 }, Task::perform(load(7), loaded))
    }
    fn update(&mut self, message: Message) {
        match message { Message::Loaded(value) => self.value = value }
    }
    fn view(&self) -> Column<'_, Message> { column![text(self.value)] }
}
"#
}

fn update_perform_source() -> &'static str {
    r#"
use effects::load;
use iced::Task;
use iced::widget::{Column, button, column, text};
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
#[derive(Default)] struct Counter { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Load, Loaded(i64) }
pub fn loaded(value: i64) -> Message { Message::Loaded(value) }
impl Counter {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Load => Task::perform(load(self.value + 3), loaded),
            Message::Loaded(value) => { self.value = value; Task::none() }
        }
    }
    fn view(&self) -> Column<'_, Message> {
        column![button("Load").on_press(Message::Load), text(self.value)]
    }
}
"#
}

fn perform_composition_source() -> &'static str {
    r#"
use effects::load;
use iced::Task;
use iced::widget::{Column, column, text};
use rawr_iced::task::batch;
pub fn main() -> iced::Result {
    iced::application(Counter::boot, Counter::update, Counter::view).run()
}
struct Counter { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Loaded(i64) }
pub fn loaded(value: i64) -> Message { Message::Loaded(value) }
pub fn identity(message: Message) -> Message { message }
impl Counter {
    fn boot() -> (Counter, Task<Message>) {
        let first = Task::perform(load(1), loaded);
        let second = Task::perform(load(2), loaded);
        let third = Task::perform(load(3), loaded);
        (Counter { value: 0 }, batch![first.chain(second), third].map(identity))
    }
    fn update(&mut self, message: Message) {
        match message { Message::Loaded(value) => self.value += value }
    }
    fn view(&self) -> Column<'_, Message> { column![text(self.value)] }
}
"#
}

fn assert_text(adapter: &program::Adapter, expected: &str) {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.find(expected).unwrap();
}

fn click(adapter: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(label).unwrap();
    simulator
        .into_messages()
        .next()
        .expect("the enabled button must emit one message")
}

fn one_message(
    state: &mut program::Adapter,
    task: iced::Task<runtime::Message>,
) -> Option<runtime::Message> {
    let outputs = task_outputs(task)?;
    exact_script_outputs(state, outputs, 1).pop()
}

fn messages(
    state: &mut program::Adapter,
    task: iced::Task<runtime::Message>,
    expected: usize,
) -> Vec<runtime::Message> {
    let Some(outputs) = task_outputs(task) else {
        assert_eq!(expected, 0, "task emitted no native output stream");
        return Vec::new();
    };
    exact_script_outputs(state, outputs, expected)
}

fn task_outputs(task: iced::Task<runtime::Message>) -> Option<Vec<runtime::Message>> {
    let mut stream = iced_test::runtime::task::into_stream(task)?;
    Some(iced_test::futures::futures::executor::block_on(
        async move {
            let mut outputs = Vec::new();
            while let Some(action) = stream.next().await {
                match action {
                    iced_test::runtime::Action::Output(message) => outputs.push(message),
                    action => panic!("expected message output, found {action:?}"),
                }
            }
            outputs
        },
    ))
}

fn exact_script_outputs(
    state: &mut program::Adapter,
    mut outputs: Vec<runtime::Message>,
    expected: usize,
) -> Vec<runtime::Message> {
    let final_output = outputs
        .pop()
        .expect("tracked nonempty task must emit a final host output");
    let transition = state
        .update(final_output)
        .expect("final host task output must remain valid");
    assert_eq!(
        transition.delivery(),
        program::Delivery::Dropped,
        "tracked task must end with exactly one host output"
    );
    assert!(
        transition.plan().is_none(),
        "final host task output must not create follow-up work"
    );
    assert_eq!(
        outputs.len(),
        expected,
        "task emitted a different script-output count than expected"
    );
    outputs
}

fn compile(source: &str) {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.compile_application_descriptor(source).unwrap();
}

fn compile_error(source: &str) -> resolve::Error {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let error = engine.compile_application_descriptor(source).unwrap_err();
    let ParseErrorType::Resolution(error) = error.err_type() else {
        panic!("expected a positioned resolution error, found {error}");
    };
    error.clone()
}

fn composed_source() -> &'static str {
    r#"
use iced::Task;
use iced::widget::{Column, column, text};
use rawr_iced::task::batch;
pub fn main() -> iced::Result {
    iced::application(Counter::boot, Counter::update, Counter::view).run()
}
struct Counter { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Child { One, Two, Three }
#[derive(Debug, Clone, Copy)]
enum Message { Child(Child) }
pub fn wrap(child: Child) -> Message { Message::Child(child) }
impl Counter {
    fn boot() -> (Counter, Task<Message>) {
        (
            Counter { value: 0 },
            batch![
                Task::done(Child::One).map(wrap),
                Task::done(Child::Two).map(wrap),
            ].chain(Task::done(Child::Three).map(wrap))
        )
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        self.value += 1;
        Task::none()
    }
    fn view(&self) -> Column<'_, Message> { column![text(self.value)] }
}
"#
}

fn contract_source(plan: &str, mapper: &str) -> String {
    format!(
        r#"
use iced::Task;
use iced::widget::{{Column, column, text}};
use rawr_iced::task::batch;
pub fn main() -> iced::Result {{ iced::application(Counter::boot, Counter::update, Counter::view).run() }}
#[derive(Default)] struct Counter {{ value: i64 }}
#[derive(Debug, Clone, Copy)] enum Message {{ Increment }}
#[derive(Debug, Clone, Copy)] enum Child {{ Tick }}
{mapper}
impl Counter {{
    fn boot() -> (Counter, Task<Message>) {{ (Counter {{ value: 0 }}, {plan}) }}
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Column<'_, Message> {{ column![text(self.value)] }}
}}
"#
    )
}

fn discard_source(expression: &str, tail: &str) -> String {
    format!(
        r#"
use iced::Task;
use iced::widget::{{Column, column, text}};
use rawr_iced::task::batch;
pub fn main() -> iced::Result {{ iced::run(Counter::update, Counter::view) }}
#[derive(Default)] struct Counter {{ value: i64 }}
#[derive(Debug, Clone, Copy)] enum Message {{ Increment }}
pub fn identity(message: Message) -> Message {{ message }}
pub fn make() -> Task<Message> {{ Task::done(Message::Increment) }}
impl Counter {{
    fn update(&mut self, message: Message) -> {output} {{
        {expression}
        {tail}
    }}
    fn view(&self) -> Column<'_, Message> {{ column![text(self.value)] }}
}}
"#,
        output = if tail == "plan" {
            "Task<Message>"
        } else {
            "()"
        }
    )
}

fn explicit_source(boot_output: &str, update_output: &str) -> String {
    let boot_body = if boot_output.starts_with('(') {
        "(Counter { value: 7 }, Task::done(Message::Increment))"
    } else {
        "Counter { value: 7 }"
    };
    let update_tail = if update_output == "Task<Message>" {
        if boot_output.starts_with('(') {
            "Task::done(Message::Increment)"
        } else {
            "Task::none()"
        }
    } else {
        "()"
    };
    format!(
        r#"
use iced::Task;
use iced::widget::{{Column, button, column, text}};
pub fn main() -> iced::Result {{
    iced::application(Counter::boot, Counter::update, Counter::view).run()
}}
struct Counter {{ value: i64 }}
#[derive(Debug, Clone, Copy)]
enum Message {{ Increment }}
impl Counter {{
    fn boot() -> {boot_output} {{ {boot_body} }}
    fn update(&mut self, message: Message) -> {update_output} {{
        self.value += 1;
        {update_tail}
    }}
    fn view(&self) -> Column<'_, Message> {{
        column![button("Increment").on_press(Message::Increment), text(self.value)]
    }}
}}
"#
    )
}
