//! Script mappers run on the thread that applies updates, never on the
//! thread that polls a task's stream.

use iced_test::futures::futures::StreamExt;
use rawr::{host, Dynamic, Engine, Shared};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, ThreadId};

use crate::program::{Adapter, Delivery};
use crate::runtime;

type Probes = Arc<Mutex<Vec<ThreadId>>>;

#[test]
fn operation_and_task_mappers_run_on_the_update_thread() {
    let probes = Probes::default();
    let starts = Arc::new(AtomicUsize::new(0));
    let mut adapter = adapter(SOURCE, probes.clone(), starts.clone());
    let plan = adapter.take_startup_plan();
    let task = plan
        .lower(
            adapter.instance(),
            adapter.generation(),
            &adapter.task_authority(),
        )
        .unwrap();

    let delivered = drive_elsewhere(task, |message| adapter.update(message).unwrap().delivery());

    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let applied = delivered
        .iter()
        .filter(|delivery| **delivery == Delivery::Applied);
    assert_eq!(applied.count(), 1);
    let probes = probes.lock().unwrap();
    // `loaded` and `child` each call the probe once.
    assert_eq!(probes.len(), 2);
    assert!(
        probes
            .iter()
            .all(|thread| *thread == thread::current().id()),
        "a script mapper ran on the thread that polled the task"
    );
}

#[test]
fn a_stale_continuation_never_runs_and_ends_its_task() {
    let probes = Probes::default();
    let starts = Arc::new(AtomicUsize::new(0));
    let mut old = adapter(SOURCE, probes.clone(), starts.clone());
    let plan = old.take_startup_plan();
    let task = plan
        .lower(old.instance(), old.generation(), &old.task_authority())
        .unwrap();
    // The update thread now belongs to a newer generation.
    let mut current = adapter(SOURCE, probes.clone(), starts.clone());

    let delivered = drive_elsewhere(task, |message| current.update(message).unwrap().delivery());

    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(!delivered.is_empty());
    assert!(delivered
        .iter()
        .all(|delivery| *delivery == Delivery::Dropped));
    assert!(probes.lock().unwrap().is_empty(), "an old continuation ran");
}

#[test]
fn a_rust_task_s_values_become_rawr_values_on_the_update_thread() {
    let probes = Probes::default();
    let starts = Arc::new(AtomicUsize::new(0));
    let mut adapter = adapter(SOURCE, probes.clone(), starts);
    let crossed = Probes::default();
    let recorded = crossed.clone();
    let rust = iced::Task::future(async { 7_i64 }).map(move |value| {
        let recorded = recorded.clone();
        Box::new(move |_: Option<&host::runtime::Witness>| {
            recorded.lock().unwrap().push(thread::current().id());
            Ok(Dynamic::from(value))
        }) as crate::task::Materialize
    });
    let plan = crate::task::Plan::discarded_rust_task(rust);
    let task = plan
        .lower(
            adapter.instance(),
            adapter.generation(),
            &adapter.task_authority(),
        )
        .unwrap();

    drive_elsewhere(task, |message| adapter.update(message).unwrap().delivery());

    assert_eq!(*crossed.lock().unwrap(), [thread::current().id()]);
}

/// Poll `task` on another thread and apply each message it yields here,
/// as iced's runtime does. Returns what each application did.
fn drive_elsewhere(
    task: iced::Task<runtime::Message>,
    mut apply: impl FnMut(runtime::Message) -> Delivery,
) -> Vec<Delivery> {
    let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
    let (sender, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        iced_test::futures::futures::executor::block_on(async move {
            while let Some(action) = stream.next().await {
                if let iced_test::runtime::Action::Output(message) = action
                    && sender.send(message).is_err()
                {
                    break;
                }
            }
        });
    });
    let mut delivered = Vec::new();
    while let Ok(message) = receiver.recv() {
        delivered.push(apply(message));
    }
    worker.join().unwrap();
    delivered
}

fn adapter(source: &str, probes: Probes, starts: Arc<AtomicUsize>) -> Adapter {
    let effects = effects_facade();
    let mut engine = Engine::new();
    crate::install(&mut engine).unwrap();
    engine.register_host_facade(effects.clone()).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    Adapter::boot(launch.bind(registry(effects, probes, starts)).unwrap()).unwrap()
}

fn registry(
    effects: host::facade::Definition,
    probes: Probes,
    starts: Arc<AtomicUsize>,
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
            move |_| {
                starts.fetch_add(1, Ordering::SeqCst);
                async move { Ok::<_, ()>(Dynamic::from(7_i64)) }
            },
            |_| Dynamic::from(-1_i64),
        )
        .unwrap();
    binding
        .bind("effects::probe", move |_| {
            probes.lock().unwrap().push(thread::current().id());
            Ok(Dynamic::from(0_i64))
        })
        .unwrap();
    let mut registry = host::runtime::Registry::new();
    crate::install_test_runtime(&mut registry);
    registry.install(binding.finish()).unwrap();
    registry
}

fn effects_facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("effects", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut load = host::signature::Builder::new();
    load.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    builder
        .item(host::item::Definition::operation("load", load.finish()).unwrap())
        .unwrap();
    let mut probe = host::signature::Builder::new();
    probe.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    builder
        .item(host::item::Definition::function("probe", probe.finish()).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

const SOURCE: &str = r#"
use effects::{load, probe};
use iced::Task;
use iced::Element;
use iced::widget::text;
#[derive(Default)] struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Leaf { Loaded(i64) }
#[derive(Debug, Clone, Copy)] enum Message { Child(Leaf) }
pub fn loaded(value: i64) -> Leaf { Leaf::Loaded(value + probe()) }
pub fn child(leaf: Leaf) -> Message { Message::Child(match leaf { Leaf::Loaded(value) => Leaf::Loaded(value + probe()) }) }
impl State {
    fn boot() -> (State, Task<Message>) {
        (State::default(), Task::perform(load(), loaded).map(child))
    }
    fn update(&mut self, message: Message) {
        match message { Message::Child(Leaf::Loaded(value)) => self.value = value }
    }
    fn view(&self) -> Element<'_, Message> { text(self.value) }
}
pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#;
