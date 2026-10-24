use rawr::{Dynamic, Engine, INT, Scope};

#[derive(Debug, Clone, Eq, PartialEq)]
#[allow(dead_code)]
enum Message {
    Idle(INT),
    FloorButtonPressed(INT, INT),
    PassingFloor(INT, INT, String),
    Tick,
}

fn engine() -> Engine {
    let mut engine = Engine::new();
    engine.register_enum::<Message>("Message", &[("Idle", 1), ("FloorButtonPressed", 2), ("PassingFloor", 3), ("Tick", 0)], |message| match message {
        Message::Idle(elevator) => (0, vec![(*elevator).into()]),
        Message::FloorButtonPressed(elevator, floor) => (1, vec![(*elevator).into(), (*floor).into()]),
        Message::PassingFloor(elevator, floor, direction) => (2, vec![(*elevator).into(), (*floor).into(), direction.clone().into()]),
        Message::Tick => (3, vec![]),
    });
    engine
}

#[test]
fn enum_values_remain_opaque_custom_values() {
    let mut engine = engine();
    engine.register_fn("is_idle", |value: Message| matches!(value, Message::Idle(_)));
    let mut scope = Scope::new();
    scope.push("message", Message::Idle(1));

    assert!(engine.eval_with_scope::<bool>(&mut scope, "is_idle(message)").unwrap());
    assert_eq!(scope.get_value::<Message>("message").unwrap(), Message::Idle(1));

    scope.set_value("message", Message::Tick);
    assert!(!engine.eval_with_scope::<bool>(&mut scope, "is_idle(message)").unwrap());
}

#[test]
#[should_panic(expected = "duplicate Rawr enum variant")]
fn duplicate_variant_registration_is_rejected() {
    Engine::new().register_enum::<Message>("Message", &[("Idle", 1), ("Idle", 1)], |_| (0, vec![Dynamic::UNIT]));
}
