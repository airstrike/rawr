use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use iced::widget::Widget as _;
use rawr::{application, host, timing, Dynamic, Engine, Shared};
use rawr_iced::{program, runtime, widget};

#[test]
fn open_descriptor_round_trips_through_dynamic_and_lowers_fresh_elements() {
    let adapter = adapter();
    let lower_count = Arc::new(AtomicUsize::new(0));
    let identity_count = Arc::new(AtomicUsize::new(0));
    let identity = widget::Identity::unique();
    let value = widget::Value::new(Probe {
        identity: identity.clone(),
        label: "native extension".to_owned(),
        lower_count: lower_count.clone(),
        identity_count: identity_count.clone(),
    });

    let restored = widget::Value::from_dynamic(Dynamic::from(value.clone())).unwrap();
    assert_eq!(restored.identity(), identity);
    assert_eq!(value.clone().identity(), identity);

    let context = adapter.context();
    let mut first = iced_test::simulator(value.lower(&context));
    assert!(first.find("native extension").is_ok());
    let _ = first.click("native extension").unwrap();
    let message = first.into_messages().next().unwrap();
    assert_eq!(message.instance(), adapter.instance());
    assert_eq!(message.generation(), adapter.generation());
    assert_eq!(message.payload().as_int(), Ok(42));
    let mut second = iced_test::simulator(restored.lower(&context));
    assert!(second.find("native extension").is_ok());
    assert_eq!(lower_count.load(Ordering::Relaxed), 2);
    assert_eq!(identity_count.load(Ordering::Relaxed), 1);
    assert_eq!(
        adapter
            .timing()
            .aggregate(timing::Phase::ScriptedHookDispatch)
            .count(),
        0
    );
}

#[test]
fn iced_program_relowers_fallback_without_reentering_rawr_view() {
    let adapter = adapter();
    assert!(adapter.last_good_output().is_none());
    assert_eq!(adapter.diagnostics().len(), 1);
    let display = adapter.display().identity();
    let application = program::Application::from_adapter(adapter);
    let (state, _task) = <program::Application as iced::Program>::boot(&application);
    let entry = &state.diagnostics()[0];
    let diagnostic = entry.message().to_owned();
    assert!(diagnostic.contains("initial view failed"), "{diagnostic}");
    assert_eq!(entry.report().code(), rawr::diagnostic::code::RUNTIME_THROW);

    for _ in 0..2 {
        let element = <program::Application as iced::Program>::view(
            &application,
            &state,
            iced::window::Id::unique(),
        );
        let mut simulator = iced_test::simulator(element);
        assert!(simulator.find("Rawr view unavailable").is_ok());
        assert!(simulator.find("Rawr runtime diagnostic").is_ok());
        assert!(simulator.find(diagnostic.as_str()).is_ok());
    }

    assert_eq!(state.display().identity(), display);
    assert_eq!(state.diagnostics().len(), 1);
}

struct Probe {
    identity: widget::Identity,
    label: String,
    lower_count: Arc<AtomicUsize>,
    identity_count: Arc<AtomicUsize>,
}

impl widget::Descriptor for Probe {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        self.lower_count.fetch_add(1, Ordering::Relaxed);
        iced::widget::button(iced::widget::text(self.label.clone()))
            .on_press(context.message(Dynamic::from(42_i64)))
            .boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity_count.fetch_add(1, Ordering::Relaxed);
        self.identity.clone()
    }
}

fn adapter() -> program::Adapter {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let candidate = engine.compile_application(source()).unwrap();
    let descriptor = candidate.validate().unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    program::Adapter::boot_observed(bind(launch)).unwrap()
}

fn bind(launch: application::Launch) -> application::BoundLaunch {
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    launch.bind(registry).unwrap()
}

fn source() -> &'static str {
    r#"
use iced::Element;
pub fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}
#[derive(Default)]
struct Counter { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Message { Tick }
impl Counter {
    fn update(&mut self, message: Message) {
        match message { Message::Tick => self.value += 1 }
    }
    fn view(&self) -> Element<'_, Message> { throw "initial view failed"; }
}
"#
}
