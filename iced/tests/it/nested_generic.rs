use std::sync::{Arc, Mutex};

use rawr::{capability, host, Dynamic, Engine, EvalAltResult, Position, Shared};
use rawr_iced::{program, runtime};

#[test]
fn nested_generic_state_and_message_values_use_the_launch_bound_validator() {
    let mut adapter = boot(false).unwrap();
    let message = click(&adapter, "Nested");

    assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn one_outer_value_cannot_reuse_a_different_logical_argument_witness() {
    let error = match boot(true) {
        Ok(_) => panic!("a bool witness must not authorize Value<i64>"),
        Err(error) => error,
    };

    assert!(
        error.to_string().contains("failed exact type attestation"),
        "{error}"
    );
}

fn boot(reuse_wrong_witness: bool) -> Result<program::Adapter, Box<program::Error>> {
    let definition = vault::facade();
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(definition.clone()).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source()).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry
        .install(vault::runtime(definition, reuse_wrong_witness))
        .unwrap();
    program::Adapter::boot(launch.bind(registry).unwrap()).map_err(Box::new)
}

fn click(adapter: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(label).unwrap();
    simulator
        .into_messages()
        .next()
        .expect("the nested generic message button must emit once")
}

fn source() -> &'static str {
    r#"
use iced::widget::{Button, button};
use vault::{Value, make};

struct State { nested: Value<i64> }

#[derive(Clone)]
struct Message { nested: Value<i64> }

impl State {
    fn poison() -> Value<bool> { make(true) }

    fn boot() -> State {
        State::poison();
        State { nested: make(7) }
    }

    fn update(&mut self, message: Message) {
        self.nested = message.nested;
    }

    fn view(&self) -> Button<'_, Message> {
        button("Nested").on_press(Message { nested: make(9) })
    }
}

pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#
}

mod vault {
    use super::*;

    #[derive(Clone)]
    struct Value {
        payload: Dynamic,
        witness: host::runtime::Witness,
    }

    pub fn facade() -> host::facade::Definition {
        let mut builder =
            host::facade::Builder::new("vault", host::facade::Abi::new(1).unwrap()).unwrap();
        builder
            .type_::<Value>(
                "Value",
                &["T"],
                &[
                    capability::Kind::Clone,
                    capability::Kind::Send,
                    capability::Kind::Static,
                ],
            )
            .unwrap();
        let mut signature = host::signature::Builder::new();
        signature.type_parameter("T", &[]).unwrap();
        signature.parameter(host::type_ref::Type::parameter("T").unwrap());
        signature.output(
            host::type_ref::Type::applied("Value", [host::type_ref::Type::parameter("T").unwrap()])
                .unwrap(),
        );
        builder
            .item(host::item::Definition::function("make", signature.finish()).unwrap())
            .unwrap();
        builder.finish().unwrap()
    }

    pub fn runtime(
        definition: host::facade::Definition,
        reuse_wrong_witness: bool,
    ) -> host::runtime::Facade {
        let mut builder = host::runtime::Builder::new(definition);
        builder
            .bind_type_attestation("vault::Value", |value, expected| {
                let value = value.read_lock::<Value>()?;
                if expected.argument_count() != 1 || !expected.argument_matches(0, &value.payload) {
                    return None;
                }
                expected.attest_witness(Some(&value.witness))
            })
            .unwrap();
        let first = Arc::new(Mutex::new(None::<host::runtime::Witness>));
        builder
            .bind_with_context("vault::make", move |call, args| {
                if args.len() != 1 {
                    return Err(runtime_error(format!(
                        "vault::make received {} operand(s), expected 1",
                        args.len()
                    )));
                }
                let issued = call
                    .output_witness()
                    .map_err(|error| runtime_error(error.to_string()))?;
                let witness = if reuse_wrong_witness {
                    let mut first = first.lock().expect("test witness slot is not poisoned");
                    match first.as_ref() {
                        Some(witness) => witness.clone(),
                        None => {
                            *first = Some(issued.clone());
                            issued
                        }
                    }
                } else {
                    issued
                };
                Ok(Dynamic::from(Value {
                    payload: args[0].take(),
                    witness,
                }))
            })
            .unwrap();
        builder.finish()
    }

    fn runtime_error(message: String) -> Box<EvalAltResult> {
        EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
    }
}
