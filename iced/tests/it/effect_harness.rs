use iced_test::futures::futures::channel::{mpsc, oneshot};
use iced_test::futures::futures::{FutureExt, StreamExt};
use rawr::{host, Dynamic, Engine, Shared};
use rawr_iced::{diagnostic, program, runtime};
use std::sync;

#[derive(Clone, Copy)]
enum Error {
    Failed,
    Retired,
}

struct Operation {
    input: i64,
    completion: oneshot::Sender<Result<Dynamic, Error>>,
}

struct Ticks {
    input: i64,
    sender: mpsc::UnboundedSender<Result<Dynamic, Error>>,
}

struct Harness {
    operations: sync::mpsc::Receiver<Operation>,
    streams: sync::mpsc::Receiver<Ticks>,
}

impl Operation {
    fn succeed(self, value: i64) -> bool {
        self.completion.send(Ok(Dynamic::from(value))).is_ok()
    }

    fn fail(self) -> bool {
        self.completion.send(Err(Error::Failed)).is_ok()
    }
}

impl Ticks {
    fn tick(&mut self, value: i64) -> bool {
        self.sender.unbounded_send(Ok(Dynamic::from(value))).is_ok()
    }
}

impl Harness {
    fn bind(effects: host::facade::Definition) -> (Self, host::runtime::Registry) {
        let operation = effects
            .items()
            .find(|item| item.path() == "effects::step")
            .expect("the fake facade must declare effects::step")
            .clone();
        let stream = effects
            .items()
            .find(|item| item.path() == "effects::ticks")
            .expect("the fake facade must declare effects::ticks")
            .clone();
        let (operation_sender, operations) = sync::mpsc::channel();
        let (stream_sender, streams) = sync::mpsc::channel();
        let mut binding = host::runtime::Builder::new(effects);
        binding
            .bind_operation(
                &operation,
                move |arguments| {
                    let input = arguments
                        .into_values()
                        .into_vec()
                        .remove(0)
                        .as_int()
                        .expect("the exact operation proof must retain one i64 input");
                    let operation_sender = operation_sender.clone();
                    async move {
                        let (completion, pending) = oneshot::channel();
                        operation_sender
                            .send(Operation { input, completion })
                            .map_err(|_| Error::Retired)?;
                        pending.await.unwrap_or(Err(Error::Retired))
                    }
                },
                |_error| Dynamic::from(true),
            )
            .unwrap();
        binding
            .bind_stream_operation(
                &stream,
                |_index, value, writer| {
                    writer.i64(value.as_int().map_err(host::stream::identity::Error::new)?);
                    Ok(())
                },
                move |arguments| {
                    let input = arguments
                        .into_values()
                        .into_vec()
                        .remove(0)
                        .as_int()
                        .expect("the exact stream proof must retain one i64 input");
                    let (sender, receiver) = mpsc::unbounded();
                    stream_sender
                        .send(Ticks { input, sender })
                        .expect("the deterministic stream harness must remain alive");
                    receiver
                },
                |_error| Dynamic::from(true),
            )
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        for facade in rawr_iced::runtime_facades().unwrap() {
            registry.install(facade).unwrap();
        }
        registry.install(binding.finish()).unwrap();
        (
            Self {
                operations,
                streams,
            },
            registry,
        )
    }

    fn operations(
        &self,
        stream: &mut iced_test::futures::BoxStream<iced_test::runtime::Action<runtime::Message>>,
        expected: usize,
    ) -> Vec<Operation> {
        let mut operations = Vec::new();
        for _ in 0..32 {
            operations.extend(self.operations.try_iter());
            if operations.len() == expected {
                return operations;
            }
            match stream.next().now_or_never() {
                None => {}
                Some(Some(action)) => {
                    panic!("controlled operation emitted before completion: {action:?}")
                }
                Some(None) => panic!("controlled task ended before every operation activated"),
            }
        }
        panic!(
            "controlled task activated {} operation(s), expected {expected}",
            operations.len()
        );
    }
}

#[test]
fn startup_maps_nested_components_while_batch_and_chain_keep_iced_ordering() {
    let effects = effects_facade();
    let (harness, registry) = Harness::bind(effects.clone());
    let launch = bind(composition_source(), effects, registry);
    let application = program::Application::from_adapter(program::Adapter::boot(launch).unwrap());
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);
    let mut stream = iced_test::runtime::task::into_stream(startup)
        .expect("the controlled startup plan must lower to one native stream");

    let mut activated = harness.operations(&mut stream, 2);
    activated.sort_by_key(|operation| operation.input);
    assert_eq!(
        activated
            .iter()
            .map(|operation| operation.input)
            .collect::<Vec<_>>(),
        [1, 3]
    );
    let first = activated.remove(0);
    let parallel = activated.remove(0);
    assert!(parallel.succeed(3));
    let message = next_script_message(&mut state, &mut stream);
    apply(&application, &mut state, message);
    assert!(harness.operations.try_recv().is_err());

    assert!(first.succeed(1));
    let message = next_script_message(&mut state, &mut stream);
    apply(&application, &mut state, message);
    let second = harness
        .operations(&mut stream, 1)
        .pop()
        .expect("the chained second operation must activate after the first ends");
    assert_eq!(second.input, 2);
    assert!(second.succeed(2));
    let message = next_script_message(&mut state, &mut stream);
    apply(&application, &mut state, message);

    let completion = next_task_message(&mut stream);
    let transition = state.update(completion).unwrap();
    assert_eq!(transition.delivery(), program::Delivery::Dropped);
    assert!(transition.plan().is_none());
    assert!(iced_test::futures::futures::executor::block_on(stream.next()).is_none());
    assert_text(&state, "3");
}

#[test]
fn controlled_failure_is_diagnostic_and_never_reaches_the_mapper_or_update() {
    let effects = effects_facade();
    let (harness, registry) = Harness::bind(effects.clone());
    let launch = bind(failure_source(), effects, registry);
    let application = program::Application::from_adapter(program::Adapter::boot(launch).unwrap());
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);
    let mut stream = iced_test::runtime::task::into_stream(startup)
        .expect("the controlled startup plan must lower to one native stream");
    let operation = harness
        .operations(&mut stream, 1)
        .pop()
        .expect("the fake operation must activate once");

    assert_eq!(operation.input, 9);
    assert!(operation.fail());
    let failure = next_script_message(&mut state, &mut stream);
    let follow_up =
        <program::Application as iced::Program>::update(&application, &mut state, failure);
    assert!(iced_test::runtime::task::into_stream(follow_up).is_none());
    assert_text(&state, "0");
    assert!(state.diagnostics().iter().any(|entry| {
        entry.stage() == diagnostic::Stage::Task && entry.message().contains("invalid output")
    }));

    let completion = next_task_message(&mut stream);
    assert_eq!(
        state.update(completion).unwrap(),
        program::Delivery::Dropped
    );
    assert!(iced_test::futures::futures::executor::block_on(stream.next()).is_none());
}

#[test]
fn owner_drop_aborts_a_pending_controlled_operation_without_a_wall_clock() {
    let effects = effects_facade();
    let (harness, registry) = Harness::bind(effects.clone());
    let launch = bind(single_source(), effects, registry);
    let application = program::Application::from_adapter(program::Adapter::boot(launch).unwrap());
    let (state, startup) = <program::Application as iced::Program>::boot(&application);
    let instance = state.instance();
    let generation = state.generation();
    let mut stream = iced_test::runtime::task::into_stream(startup)
        .expect("the controlled startup plan must lower to one native stream");
    let operation = harness
        .operations(&mut stream, 1)
        .pop()
        .expect("the fake operation must activate once");

    drop(state);
    let completion = next_task_message(&mut stream);
    assert_eq!(completion.instance(), instance);
    assert_eq!(completion.generation(), generation);
    assert!(iced_test::futures::futures::executor::block_on(stream.next()).is_none());
    assert!(
        !operation.succeed(7),
        "retirement must drop the controlled operation future"
    );
}

#[test]
fn a_controlled_late_result_is_rejected_by_replacement_ownership() {
    let effects = effects_facade();
    let (harness, registry) = Harness::bind(effects.clone());
    let launch = bind(single_source(), effects.clone(), registry);
    let old = program::Application::from_adapter(program::Adapter::boot(launch.clone()).unwrap());
    let (mut old_state, startup) = <program::Application as iced::Program>::boot(&old);
    let mut stream = iced_test::runtime::task::into_stream(startup)
        .expect("the controlled startup plan must lower to one native stream");
    let operation = harness
        .operations(&mut stream, 1)
        .pop()
        .expect("the fake operation must activate once");
    assert!(operation.succeed(7));
    let late = next_task_message(&mut stream);

    let mut same_generation = program::Adapter::boot(launch).unwrap();
    assert_eq!(same_generation.generation(), late.generation());
    assert_ne!(same_generation.instance(), late.instance());
    let transition = same_generation.update(late.clone()).unwrap();
    assert_eq!(transition.delivery(), program::Delivery::Dropped);
    assert_text(&same_generation, "0");

    let (_, registry) = Harness::bind(effects.clone());
    let mut new_generation =
        program::Adapter::boot(bind(single_source(), effects, registry)).unwrap();
    assert_ne!(new_generation.generation(), late.generation());
    let transition = new_generation.update(late).unwrap();
    assert_eq!(transition.delivery(), program::Delivery::Dropped);
    assert_text(&new_generation, "0");

    let completion = next_task_message(&mut stream);
    assert_eq!(
        old_state.update(completion).unwrap(),
        program::Delivery::Dropped
    );
    assert!(iced_test::futures::futures::executor::block_on(stream.next()).is_none());
}

#[test]
fn tracker_retains_equivalent_streams_and_replaces_changed_streams_exactly_once() {
    let effects = effects_facade();
    let (harness, registry) = Harness::bind(effects.clone());
    let launch = bind(subscription_source(), effects, registry);
    let application = program::Application::from_adapter(program::Adapter::boot(launch).unwrap());
    let (mut state, _) = <program::Application as iced::Program>::boot(&application);
    let (sender, mut messages) = mpsc::channel(8);
    let mut tracker = iced_test::futures::subscription::Tracker::new();

    let first = <program::Application as iced::Program>::subscription(&application, &state);
    let mut first = tracker.update(
        iced::advanced::subscription::into_recipes(first).into_iter(),
        sender.clone(),
    );
    assert_eq!(first.len(), 1);
    let mut first = first.remove(0);
    let mut old = harness
        .streams
        .try_recv()
        .expect("the first tracked recipe must start one stream");
    assert_eq!(old.input, 1);

    let equivalent = <program::Application as iced::Program>::subscription(&application, &state);
    assert!(tracker
        .update(
            iced::advanced::subscription::into_recipes(equivalent).into_iter(),
            sender.clone(),
        )
        .is_empty());
    assert!(harness.streams.try_recv().is_err());

    let replace = click(&state, "Replace subscription");
    apply(&application, &mut state, replace);
    let replacement = <program::Application as iced::Program>::subscription(&application, &state);
    let mut replacement = tracker.update(
        iced::advanced::subscription::into_recipes(replacement).into_iter(),
        sender.clone(),
    );
    assert_eq!(replacement.len(), 1);
    let mut replacement = replacement.remove(0);
    let mut current = harness
        .streams
        .try_recv()
        .expect("the changed recipe must start one replacement stream");
    assert_eq!(current.input, 2);
    assert!(first.as_mut().now_or_never().is_some());
    assert!(!old.tick(7), "the retired recipe stream must be dropped");

    assert!(current.tick(9));
    let tick = execution_message(&mut replacement, &mut messages);
    apply(&application, &mut state, tick);
    assert_text(&state, "9");

    let retained = <program::Application as iced::Program>::subscription(&application, &state);
    assert!(tracker
        .update(
            iced::advanced::subscription::into_recipes(retained).into_iter(),
            sender.clone(),
        )
        .is_empty());
    assert!(harness.streams.try_recv().is_err());

    assert!(tracker
        .update(
            iced::advanced::subscription::into_recipes(iced::Subscription::none()).into_iter(),
            sender,
        )
        .is_empty());
    assert!(replacement.as_mut().now_or_never().is_some());
    assert!(!current.tick(10));
}

fn apply(
    _application: &program::Application,
    state: &mut program::Adapter,
    message: runtime::Message,
) {
    let transition = state
        .update(message)
        .expect("the deterministic fixture update must be admitted");
    assert_eq!(transition.delivery(), program::Delivery::Applied);
    assert!(
        transition.plan().is_none(),
        "the deterministic fixture update must not create follow-up work"
    );
}

fn next_task_message(
    stream: &mut iced_test::futures::BoxStream<iced_test::runtime::Action<runtime::Message>>,
) -> runtime::Message {
    iced_test::futures::futures::executor::block_on(async {
        match stream.next().await {
            Some(iced_test::runtime::Action::Output(message)) => message,
            Some(action) => panic!("expected a task message, found {action:?}"),
            None => panic!("controlled task ended before its expected output"),
        }
    })
}

/// The next message the task yields once the script steps it hands back
/// have run on this thread.
fn next_script_message(
    state: &mut program::Adapter,
    stream: &mut iced_test::futures::BoxStream<iced_test::runtime::Action<runtime::Message>>,
) -> runtime::Message {
    loop {
        if let Some(message) = state.take_script_step(next_task_message(stream)) {
            return message;
        }
    }
}

fn execution_message(
    execution: &mut iced_test::futures::BoxFuture<()>,
    messages: &mut mpsc::Receiver<runtime::Message>,
) -> runtime::Message {
    for _ in 0..16 {
        assert!(
            execution.as_mut().now_or_never().is_none(),
            "the retained stream execution must remain pending"
        );
        if let Some(Some(message)) = messages.next().now_or_never() {
            return message;
        }
    }
    panic!("the controlled stream tick was not forwarded")
}

fn click(state: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(state.view());
    simulator.click(label).unwrap();
    simulator
        .into_messages()
        .next()
        .expect("the enabled test button must emit one message")
}

fn assert_text(state: &program::Adapter, expected: &str) {
    iced_test::simulator(state.view()).find(expected).unwrap();
}

fn bind(
    source: &str,
    effects: host::facade::Definition,
    registry: host::runtime::Registry,
) -> rawr::application::BoundLaunch {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(effects).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    engine
        .launch_application(descriptor)
        .unwrap()
        .bind(registry)
        .unwrap()
}

fn effects_facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("effects", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut step = host::signature::Builder::new();
    step.parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    step.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    builder
        .item(host::item::Definition::operation("step", step.finish()).unwrap())
        .unwrap();
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

fn composition_source() -> &'static str {
    r#"
use effects::step;
use iced::Task;
use iced::Element;
use iced::widget::text;
use rawr_iced::task::batch;
struct State { deliveries: i64 }
#[derive(Debug, Clone, Copy)] enum Leaf { Loaded(i64) }
#[derive(Debug, Clone, Copy)] enum Child { Leaf(Leaf) }
#[derive(Debug, Clone, Copy)] enum Message { Child(Child) }
pub fn leaf(value: i64) -> Leaf { Leaf::Loaded(value) }
pub fn child(message: Leaf) -> Child { Child::Leaf(message) }
pub fn parent(message: Child) -> Message { Message::Child(message) }
impl State {
    fn boot() -> (State, Task<Message>) {
        let first = Task::perform(step(1), leaf).map(child).map(parent);
        let second = Task::perform(step(2), leaf).map(child).map(parent);
        let parallel = Task::perform(step(3), leaf).map(child).map(parent);
        (State { deliveries: 0 }, batch![first.chain(second), parallel])
    }
    fn update(&mut self, message: Message) {
        self.deliveries += 1;
    }
    fn view(&self) -> Element<'_, Message> { text(self.deliveries) }
}
pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#
}

fn failure_source() -> &'static str {
    r#"
use effects::step;
use iced::Task;
use iced::Element;
use iced::widget::text;
struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Loaded(i64) }
pub fn loaded(value: i64) -> Message { throw "mapper must not run" }
impl State {
    fn boot() -> (State, Task<Message>) {
        (State { value: 0 }, Task::perform(step(9), loaded))
    }
    fn update(&mut self, message: Message) { self.value = 1; }
    fn view(&self) -> Element<'_, Message> { text(self.value) }
}
pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#
}

fn single_source() -> &'static str {
    r#"
use effects::step;
use iced::Task;
use iced::Element;
use iced::widget::text;
struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Loaded(i64) }
pub fn loaded(value: i64) -> Message { Message::Loaded(value) }
impl State {
    fn boot() -> (State, Task<Message>) {
        (State { value: 0 }, Task::perform(step(7), loaded))
    }
    fn update(&mut self, message: Message) {
        match message { Message::Loaded(value) => self.value = value }
    }
    fn view(&self) -> Element<'_, Message> { text(self.value) }
}
pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#
}

fn subscription_source() -> &'static str {
    r#"
use effects::ticks;
use iced::Subscription;
use iced::Element;
use iced::widget::{button, column, text};
struct State { stream: i64, value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Replace, Tick(i64) }
pub fn tick(value: i64) -> Message { Message::Tick(value) }
impl State {
    fn new() -> State { State { stream: 1, value: 0 } }
    fn update(&mut self, message: Message) {
        match message {
            Message::Replace => self.stream = 2,
            Message::Tick(value) => self.value = value,
        }
    }
    fn view(&self) -> Element<'_, Message> {
        let replace = button("Replace subscription").on_press(Message::Replace);
        let value = text(self.value);
        column![replace, value]
    }
    fn subscription(&self) -> Subscription<Message> { ticks(self.stream).map(tick) }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .subscription(State::subscription)
        .run()
}
"#
}
