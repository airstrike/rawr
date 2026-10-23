use rawr::{host, Engine, Shared};
use rawr_iced::{program, runtime};

#[test]
fn exact_rust_shaped_counter_runs_through_real_native_widgets() {
    let mut adapter = boot();

    assert_text(&adapter, "0");
    let increment = click(&adapter, "Increment");
    assert_eq!(increment.instance(), adapter.instance());
    assert_eq!(increment.generation(), adapter.generation());
    assert_eq!(
        adapter.update(increment).unwrap(),
        program::Delivery::Applied
    );
    assert_text(&adapter, "1");

    let decrement = click(&adapter, "Decrement");
    assert_eq!(decrement.instance(), adapter.instance());
    assert_eq!(decrement.generation(), adapter.generation());
    assert_eq!(
        adapter.update(decrement).unwrap(),
        program::Delivery::Applied
    );
    assert_text(&adapter, "0");
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn foreign_button_message_is_dropped_without_mutation_or_diagnostic() {
    let mut adapter = boot();
    let foreign = boot();
    let stale = click(&foreign, "Increment");
    let diagnostics = adapter.diagnostics().to_vec();

    let transition = adapter.update(stale).unwrap();
    assert_eq!(transition.delivery(), program::Delivery::Dropped);
    assert!(transition.plan().is_none());
    assert_eq!(adapter.diagnostics(), diagnostics);
    adapter.refresh_view().unwrap();
    assert_text(&adapter, "0");
    assert_eq!(adapter.diagnostics(), diagnostics);
}

fn boot() -> program::Adapter {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine
        .compile_application_descriptor(include_str!("../../examples/scripts/application_counter.rs"))
        .unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    program::Adapter::boot(launch.bind(registry).unwrap()).unwrap()
}

fn assert_text(adapter: &program::Adapter, expected: &str) {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.find(expected).unwrap();
}

fn click(adapter: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(label).unwrap();
    let mut messages = simulator.into_messages();
    let message = messages.next().expect("the enabled button must emit once");
    assert!(messages.next().is_none());
    message
}
