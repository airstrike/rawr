use iced_test::futures::futures::StreamExt;
use rawr::{host, resolve, Dynamic, Engine, ParseErrorType, Shared};
use rawr_iced::{diagnostic, program, runtime};
use std::hash::Hasher as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn application_builder_accepts_pinned_subscription_shape_in_either_builder_order() {
    for tail in [
        ".theme(State::theme).subscription(State::subscription)",
        ".subscription(State::subscription).theme(State::theme)",
    ] {
        let adapter = boot(&builder_source(tail));
        assert!(adapter
            .subscription_plan()
            .is_some_and(host::stream::Plan::is_none));
        assert!(adapter.diagnostics().is_empty());
    }
}

#[test]
fn duplicate_application_builders_are_positioned_contract_errors() {
    for (tail, expected) in [
        (
            ".subscription(State::subscription).subscription(State::subscription)",
            ".subscription",
        ),
        (".theme(State::theme).theme(State::theme)", ".theme"),
    ] {
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let error = engine
            .compile_application_descriptor(builder_source(tail))
            .unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "diagnostic did not contain `{expected}`: {error}"
        );
        assert!(error.position().line().is_some());
    }
}

#[test]
fn wrong_subscription_owner_and_output_are_compile_time_errors() {
    for (source, expected) in [
        (wrong_owner_source(), "subscription state"),
        (wrong_output_source(), "subscription output"),
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
fn subscription_callback_failure_retires_the_plan_and_recovers_after_admitted_update() {
    let mut adapter = boot_with_failure(recovering_source());

    assert!(adapter.subscription_plan().is_none());
    assert_eq!(adapter.diagnostics().len(), 1);
    assert_eq!(
        adapter.diagnostics()[0].stage(),
        diagnostic::Stage::Subscription
    );

    let recover = click(&adapter, "Recover subscription");
    assert_eq!(adapter.update(recover).unwrap(), program::Delivery::Applied);
    assert!(adapter
        .subscription_plan()
        .is_some_and(host::stream::Plan::is_none));
    assert!(adapter.diagnostics().is_empty());

    let fail = click(&adapter, "Fail subscription");
    assert_eq!(adapter.update(fail).unwrap(), program::Delivery::Applied);
    assert!(adapter.subscription_plan().is_none());
    assert_eq!(adapter.diagnostics().len(), 1);
    assert_eq!(
        adapter.diagnostics()[0].stage(),
        diagnostic::Stage::Subscription
    );
}

#[test]
fn repeated_view_lowering_never_reinvokes_the_subscription_callback() {
    let adapter = boot_with_failure(recovering_source());
    let report = adapter.diagnostics()[0].report().clone();

    for _ in 0..4 {
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.find("Recover subscription").unwrap();
    }

    assert_eq!(adapter.diagnostics().len(), 1);
    assert_eq!(adapter.diagnostics()[0].report(), &report);
    assert!(adapter.subscription_plan().is_none());
}

#[test]
fn rejected_update_preserves_the_existing_subscription_transition() {
    let starts = Arc::new(AtomicUsize::new(0));
    let adapter = boot_with_stream(rejected_update_source(), starts, false);
    let application = program::Application::from_adapter(adapter);
    let (mut state, _) = <program::Application as iced::Program>::boot(&application);
    let initial = subscription_hash(<program::Application as iced::Program>::subscription(
        &application,
        &state,
    ));

    let fail = click(&state, "Reject update");
    assert!(state.update(fail).is_err());

    let retained = subscription_hash(<program::Application as iced::Program>::subscription(
        &application,
        &state,
    ));
    assert_eq!(retained, initial);
    assert!(state
        .diagnostics()
        .iter()
        .all(|entry| entry.stage() != diagnostic::Stage::Subscription));
    assert!(state
        .diagnostics()
        .iter()
        .any(|entry| entry.stage() == diagnostic::Stage::Update));
}

#[test]
fn facade_exposes_only_the_iced_shaped_subscription_composition_surface() {
    let facade = rawr_iced::facade().unwrap();
    let extension = rawr_iced::extension(&facade).unwrap();
    let subscription = facade
        .types()
        .find(|ty| ty.path() == "iced::Subscription")
        .unwrap();
    assert_eq!(subscription.usage(), host::usage::Policy::Required);
    for path in [
        "iced::Subscription::none",
        "iced::Subscription::map",
        "iced::Application::subscription",
    ] {
        assert!(facade.items().any(|item| item.path() == path));
    }
    assert!(extension
        .items()
        .any(|item| item.path() == "rawr_iced::subscription::batch"));
    for alias in [
        "iced::Subscription::run",
        "iced::Subscription::run_with",
        "iced::subscription",
    ] {
        assert!(facade.items().all(|item| item.path() != alias));
    }
}

#[test]
fn discarded_none_is_elidable_but_every_potential_stream_plan_is_required() {
    compile_usage("Subscription::none();");
    compile_usage("(Subscription::none());");

    for expression in [
        "batch![];",
        "Subscription::none().map(identity);",
        "ticks(1).map(tick);",
    ] {
        let error = compile_usage_error(expression);
        let resolve::Error::UnusedValue { expression, .. } = error else {
            panic!("expected an unused-value error, found {error}");
        };
        assert!(!expression.is_none());
    }
}

#[test]
fn same_owner_plan_identity_is_stable_and_factory_start_is_deferred_to_recipe_stream() {
    let starts = Arc::new(AtomicUsize::new(0));
    let adapter = boot_with_stream(stream_source(), starts.clone(), false);
    let other = boot_with_stream(stream_source(), starts.clone(), false);
    let application = program::Application::from_adapter(adapter);
    let (mut state, _) = <program::Application as iced::Program>::boot(&application);

    let first = <program::Application as iced::Program>::subscription(&application, &state);
    let second = <program::Application as iced::Program>::subscription(&application, &state);
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    let initial_hash = subscription_hash(first);
    assert_eq!(initial_hash, subscription_hash(second));

    let current = <program::Application as iced::Program>::subscription(&application, &state);
    let message = one_subscription_message(current);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let _ = <program::Application as iced::Program>::update(&application, &mut state, message);

    let changed = <program::Application as iced::Program>::subscription(&application, &state);
    let unchanged_owner_hash = subscription_hash(changed);
    assert_ne!(initial_hash, unchanged_owner_hash);
    let other_application = program::Application::from_adapter(other);
    let (other_state, _) = <program::Application as iced::Program>::boot(&other_application);
    let other_owner_hash = subscription_hash(
        <program::Application as iced::Program>::subscription(&other_application, &other_state),
    );
    assert_ne!(unchanged_owner_hash, other_owner_hash);
}

#[test]
fn invalid_native_stream_output_is_gated_before_update_and_retires_the_plan() {
    let starts = Arc::new(AtomicUsize::new(0));
    let adapter = boot_with_stream(stream_source(), starts.clone(), true);
    let application = program::Application::from_adapter(adapter);
    let (mut state, _) = <program::Application as iced::Program>::boot(&application);
    let subscription = <program::Application as iced::Program>::subscription(&application, &state);
    let message = one_subscription_message(subscription);

    let follow_up =
        <program::Application as iced::Program>::update(&application, &mut state, message);

    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(iced_test::runtime::task::into_stream(follow_up).is_none());
    assert!(state.subscription_plan().is_none());
    assert_eq!(state.diagnostics().len(), 1);
    assert_eq!(
        state.diagnostics()[0].stage(),
        diagnostic::Stage::Subscription
    );
    assert!(state.diagnostics()[0].message().contains("invalid output"));
}

fn boot(source: &str) -> program::Adapter {
    rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(source)
        .unwrap()
        .prepare()
        .unwrap()
        .into_adapter()
}

fn boot_with_failure(source: &str) -> program::Adapter {
    let failure = failure_facade();
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(failure.clone()).unwrap();
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let engine = Shared::new(engine);
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry.install(failure_runtime(failure)).unwrap();
    program::Adapter::boot(launch.bind(registry).unwrap()).unwrap()
}

fn click(adapter: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(label).unwrap();
    simulator
        .into_messages()
        .next()
        .expect("the enabled button must emit one message")
}

fn subscription_hash(subscription: iced::Subscription<runtime::Message>) -> u64 {
    let mut recipes = iced::advanced::subscription::into_recipes(subscription);
    assert_eq!(recipes.len(), 1);
    let mut hasher = iced::advanced::subscription::Hasher::default();
    recipes.remove(0).hash(&mut hasher);
    hasher.finish()
}

fn one_subscription_message(
    subscription: iced::Subscription<runtime::Message>,
) -> runtime::Message {
    let mut recipes = iced::advanced::subscription::into_recipes(subscription);
    assert_eq!(recipes.len(), 1);
    let mut stream = recipes
        .remove(0)
        .stream(Box::pin(iced::futures::stream::empty()));
    iced::futures::executor::block_on(async move {
        let message = stream.next().await.expect("stream must emit one item");
        assert!(stream.next().await.is_none());
        message
    })
}

fn boot_with_stream(
    source: &str,
    starts: Arc<AtomicUsize>,
    invalid_output: bool,
) -> program::Adapter {
    let events = events_facade();
    let failure = failure_facade();
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(events.clone()).unwrap();
    engine.register_host_facade(failure.clone()).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let operation = events
        .items()
        .find(|item| item.path() == "events::ticks")
        .unwrap()
        .clone();
    let mut binding = host::runtime::Builder::new(events);
    binding
        .bind_stream_operation(
            &operation,
            |index, value, writer| {
                if index != 0 {
                    return Err(host::stream::identity::Error::new(
                        "unexpected stream argument index",
                    ));
                }
                let value = value.as_int().map_err(|_| {
                    host::stream::identity::Error::new("expected canonical i64 argument")
                })?;
                writer.i64(value);
                Ok(())
            },
            move |arguments| {
                starts.fetch_add(1, Ordering::SeqCst);
                let mut values = arguments.into_values().into_vec();
                let value = values.remove(0);
                let output = if invalid_output {
                    Dynamic::from(true)
                } else {
                    value
                };
                iced::futures::stream::iter([Ok::<_, ()>(output)])
            },
            |_| Dynamic::from(-1_i64),
        )
        .unwrap();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry.install(binding.finish()).unwrap();
    registry.install(failure_runtime(failure)).unwrap();
    program::Adapter::boot(launch.bind(registry).unwrap()).unwrap()
}

fn failure_facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("fixture", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut guard = host::signature::Builder::new();
    guard.parameter(host::type_ref::Type::builtin(
        host::type_ref::Builtin::String,
    ));
    guard.output(host::type_ref::Type::builtin(host::type_ref::Builtin::Bool));
    builder
        .item(host::item::Definition::function("guard", guard.finish()).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

fn failure_runtime(facade: host::facade::Definition) -> host::runtime::Facade {
    let mut builder = host::runtime::Builder::new(facade);
    builder
        .bind("fixture::guard", |args| {
            if args.len() != 1 {
                return Err(runtime_error(format!(
                    "fixture::guard expected 1 argument, found {}",
                    args.len()
                )));
            }
            let value = args[0].clone().into_immutable_string().map_err(|actual| {
                runtime_error(format!("fixture::guard expected String, found {actual}"))
            })?;
            if value == "invalid" {
                return Err(runtime_error(
                    "fixture guard rejected invalid input".to_owned(),
                ));
            }
            Ok(Dynamic::from(true))
        })
        .unwrap();
    builder.finish()
}

fn runtime_error(message: String) -> Box<rawr::EvalAltResult> {
    rawr::EvalAltResult::ErrorRuntime(message.into(), rawr::Position::NONE).into()
}

fn events_facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("events", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut ticks = host::signature::Builder::new();
    ticks.parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    ticks.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    builder
        .item(
            host::item::Definition::stream_operation(
                "ticks",
                ticks.finish(),
                host::stream::Version::new(1),
            )
            .unwrap(),
        )
        .unwrap();
    builder.finish().unwrap()
}

fn compile_usage(expression: &str) {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(events_facade()).unwrap();
    engine
        .compile_application_descriptor(usage_source(expression))
        .unwrap();
}

fn compile_usage_error(expression: &str) -> resolve::Error {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(events_facade()).unwrap();
    let error = engine
        .compile_application_descriptor(usage_source(expression))
        .unwrap_err();
    let ParseErrorType::Resolution(error) = error.err_type() else {
        panic!("expected a positioned resolution error, found {error}");
    };
    error.clone()
}

fn usage_source(expression: &str) -> String {
    format!(
        r#"
use events::ticks;
use iced::Subscription;
use iced::Element;
use iced::widget::text;
use rawr_iced::subscription::batch;
pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message {{ Tick(i64) }}
pub fn identity(message: Message) -> Message {{ message }}
pub fn tick(value: i64) -> Message {{ Message::Tick(value) }}
impl State {{
    fn update(&mut self, message: Message) {{
        {expression}
    }}
    fn view(&self) -> Element<'_, Message> {{ text("subscription") }}
}}
"#
    )
}

fn builder_source(tail: &str) -> String {
    format!(
        r#"
use iced::{{Subscription, Theme}};
use iced::Element;
use iced::widget::text;
struct State;
#[derive(Debug, Clone, Copy)] enum Message {{ Tick }}
impl State {{
    fn new() -> State {{ State {{}} }}
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{ text("subscription") }}
    fn theme(&self) -> Theme {{ Theme::Dark }}
    fn subscription(&self) -> Subscription<Message> {{ Subscription::none() }}
}}
pub fn main() -> iced::Result {{
    iced::application(State::new, State::update, State::view){tail}.run()
}}
"#
    )
}

fn recovering_source() -> &'static str {
    r#"
use fixture::guard;
use iced::Subscription;
use iced::Element;
use iced::widget::{button, column};
struct State { status: String }
#[derive(Debug, Clone, Copy)] enum Message { Fail, Recover }
impl State {
    fn new() -> State { State { status: "invalid" } }
    fn update(&mut self, message: Message) {
        match message {
            Message::Fail => self.status = "invalid",
            Message::Recover => self.status = "valid",
        }
    }
    fn view(&self) -> Element<'_, Message> {
        let fail = button("Fail subscription").on_press(Message::Fail);
        let recover = button("Recover subscription").on_press(Message::Recover);
        column![fail, recover]
    }
    fn subscription(&self) -> Subscription<Message> {
        let valid = guard(self.status);
        Subscription::none()
    }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .subscription(State::subscription)
        .run()
}
"#
}

fn wrong_owner_source() -> &'static str {
    r#"
use iced::Subscription;
use iced::Element;
use iced::widget::text;
struct State; struct Other;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State { fn new() -> State { State {} } fn update(&mut self, message: Message) {} fn view(&self) -> Element<'_, Message> { text("x") } }
impl Other { fn subscription(&self) -> Subscription<Message> { Subscription::none() } }
pub fn main() -> iced::Result { iced::application(State::new, State::update, State::view).subscription(Other::subscription).run() }
"#
}

fn wrong_output_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text;
struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State { fn new() -> State { State {} } fn update(&mut self, message: Message) {} fn view(&self) -> Element<'_, Message> { text("x") } fn subscription(&self) -> i64 { 1 } }
pub fn main() -> iced::Result { iced::application(State::new, State::update, State::view).subscription(State::subscription).run() }
"#
}

fn stream_source() -> &'static str {
    r#"
use events::ticks;
use iced::Subscription;
use iced::Element;
use iced::widget::text;
struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Tick(i64) }
pub fn tick(value: i64) -> Message { Message::Tick(value) }
impl State {
    fn new() -> State { State { value: 7 } }
    fn update(&mut self, message: Message) {
        match message { Message::Tick(value) => self.value = value + 1 }
    }
    fn view(&self) -> Element<'_, Message> { text(self.value) }
    fn subscription(&self) -> Subscription<Message> { ticks(self.value).map(tick) }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .subscription(State::subscription)
        .run()
}
"#
}

fn rejected_update_source() -> &'static str {
    r#"
use events::ticks;
use fixture::guard;
use iced::Subscription;
use iced::Element;
use iced::widget::button;
struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Fail, Tick(i64) }
pub fn tick(value: i64) -> Message { Message::Tick(value) }
impl State {
    fn new() -> State { State { value: 7 } }
    fn update(&mut self, message: Message) {
        match message {
            Message::Fail => { self.value = 8; let valid = guard("invalid"); },
            Message::Tick(value) => self.value = value,
        }
    }
    fn view(&self) -> Element<'_, Message> {
        button("Reject update").on_press(Message::Fail)
    }
    fn subscription(&self) -> Subscription<Message> { ticks(self.value).map(tick) }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .subscription(State::subscription)
        .run()
}
"#
}
