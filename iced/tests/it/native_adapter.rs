use rawr::timing::{Outcome, Phase};
use rawr::{application, host, Engine, Shared};
use rawr_iced::{diagnostic, program};

#[test]
fn default_boot_update_and_exact_provenance_work_before_widget_lowering() {
    let (engine, application) = candidate();
    let increment = application
        .items()
        .find(|item| item.key().name() == "Increment")
        .unwrap()
        .clone();
    let descriptor = application.validate().unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let bound = bind(launch);
    let payload = bound.invoke(&increment, &mut []).unwrap();
    let mut adapter = program::Adapter::boot_observed(bound).unwrap();

    let first_error = adapter.refresh_view().unwrap_err();
    assert!(first_error.to_string().contains('0'));
    assert!(adapter.last_good_output().is_none());

    let message = adapter.context().message(payload);
    assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);
    let second_error = adapter.refresh_view().unwrap_err();
    assert!(second_error.to_string().contains('1'));
    assert_eq!(adapter.diagnostics().len(), 1);
    assert_eq!(
        adapter.diagnostics().last().unwrap().stage(),
        diagnostic::Stage::View
    );
    let timing = adapter.timing();
    assert_eq!(timing.aggregate(Phase::ViewEvaluation).count(), 4);
    assert_eq!(timing.aggregate(Phase::DescriptorLowering).count(), 0);
    assert!(timing
        .attempts()
        .iter()
        .filter(|attempt| attempt.phase() == Phase::ViewEvaluation)
        .all(|attempt| attempt.outcome() == Outcome::Failure));
}

#[test]
fn launch_binding_rejects_missing_or_mismatched_facades_and_accepts_production_bindings() {
    let (engine, application) = candidate();
    let descriptor = application.validate().unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let error = launch.bind(host::runtime::Registry::new()).unwrap_err();
    assert!(matches!(
        error,
        host::runtime::Error::MissingFacade { ref facade } if facade == "iced"
    ));

    let (engine, application) = candidate();
    let descriptor = application.validate().unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let incompatible = host::facade::Builder::new("iced", host::facade::Abi::new(2).unwrap())
        .unwrap()
        .finish()
        .unwrap();
    let mut mismatched = host::runtime::Registry::new();
    mismatched
        .install(host::runtime::Builder::new(incompatible).finish())
        .unwrap();
    let error = launch.bind(mismatched).unwrap_err();
    assert!(matches!(
        error,
        host::runtime::Error::FacadeMismatch { ref facade } if facade == "iced"
    ));

    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine
        .compile_application_descriptor(include_str!(
            "../../examples/scripts/application_counter.rs"
        ))
        .unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    launch.bind(registry()).unwrap();
}

fn candidate() -> (Shared<Engine>, application::Candidate) {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let candidate = engine.compile_application(source()).unwrap();
    (engine, candidate)
}

fn bind(launch: application::Launch) -> application::BoundLaunch {
    launch.bind(registry()).unwrap()
}

fn registry() -> host::runtime::Registry {
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry
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
enum Message { Increment }
impl Counter {
    fn update(&mut self, message: Message) {
        match message { Message::Increment => self.value += 1 }
    }
    fn view(&self) -> Element<'_, Message> { panic!("{}", self.value) }
}
"#
}
