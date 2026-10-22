#[cfg(feature = "internals")]
use rawr::{ASTNode, Expr};
use rawr::{Dynamic, Engine, INT, ParseErrorType, Scope};

#[derive(Debug, Clone, Eq, PartialEq)]
enum Message {
    Idle(INT),
    FloorButtonPressed(INT, INT),
    PassingFloor(INT, INT, String),
    Tick,
}

#[derive(Debug, Clone)]
struct Other(INT);

#[derive(Debug, Clone)]
enum Drift {
    Left(INT),
    Right(INT),
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
fn matches_payload_variants_and_discard_bindings() {
    let engine = engine();
    let mut scope = Scope::new();

    scope.push("message", Message::Idle(4));
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "switch message { Message::Idle(elevator) => elevator, _ => -1 }",).unwrap(), 4);

    scope.set_value("message", Message::FloorButtonPressed(4, 7));
    assert_eq!(
        engine
            .eval_with_scope::<INT>(&mut scope, "switch message { Message::FloorButtonPressed(_, floor) => floor, _ => -1 }",)
            .unwrap(),
        7
    );
}

#[test]
fn matches_bare_and_parenthesized_zero_payload_variants() {
    let engine = engine();
    let mut scope = Scope::new();
    scope.push("message", Message::Tick);

    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "switch message { Message::Tick => 1, _ => 0 }",).unwrap(), 1);
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "switch message { Message::Tick() => 2, _ => 0 }",).unwrap(), 2);
}

#[test]
fn guards_can_read_payload_bindings_and_decline_a_case() {
    let engine = engine();
    let mut scope = Scope::new();
    scope.push("message", Message::PassingFloor(3, 8, "down".to_string()));

    assert_eq!(
        engine
            .eval_with_scope::<INT>(
                &mut scope,
                r#"
                    switch message {
                        Message::PassingFloor(elevator, _, direction)
                            if direction == "up" => elevator,
                        Message::PassingFloor(_, floor, _) => floor,
                        _ => -1,
                    }
                "#,
            )
            .unwrap(),
        8
    );
}

#[test]
#[cfg(not(feature = "no_function"))]
fn enum_switch_works_inside_script_functions() {
    let mut engine = engine();
    engine.register_fn("message", || Message::FloorButtonPressed(2, 11));

    assert_eq!(
        engine
            .eval::<INT>(
                r#"
                    fn handle(message) {
                        switch message {
                            Message::FloorButtonPressed(elevator, floor) => elevator + floor,
                            _ => -1,
                        }
                    }
                    handle(message())
                "#,
            )
            .unwrap(),
        13
    );
}

#[test]
fn enum_bindings_shadow_outer_constants_only_inside_the_case() {
    let engine = engine();
    let mut scope = Scope::new();
    scope.push("message", Message::Idle(3));

    assert_eq!(
        engine
            .eval_with_scope::<INT>(
                &mut scope,
                r#"
                    const elevator = 99;
                    let matched = switch message {
                        Message::Idle(elevator) => elevator,
                        _ => 0,
                    };
                    matched * 100 + elevator
                "#,
            )
            .unwrap(),
        399
    );
}

#[test]
fn literal_and_enum_cases_can_be_mixed() {
    let engine = engine();
    let mut scope = Scope::new();
    scope.push("value", Message::Idle(6));

    let script = r#"
        switch value {
            42 => 1,
            Message::Idle(elevator) => elevator,
            "other" => 2,
            _ => -1,
        }
    "#;

    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, script).unwrap(), 6);

    scope.set_value("value", 42 as INT);
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, script).unwrap(), 1);
}

#[test]
fn a_different_rust_type_falls_through_to_default() {
    let engine = engine();
    let mut scope = Scope::new();
    scope.push("value", Other(42));

    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "switch value { Message::Idle(elevator) => elevator, _ => 7 }",).unwrap(), 7);
    assert_eq!(scope.get_value::<Other>("value").unwrap().0, 42);
}

#[test]
fn a_compiled_enum_switch_does_not_silently_fall_back_without_its_registration() {
    let compiler = engine();
    let ast = compiler.compile("switch message { Message::Idle(elevator) => elevator, _ => 7 }").unwrap();
    let runtime = Engine::new();
    let mut scope = Scope::new();
    scope.push("message", Message::Idle(42));

    let error = runtime.eval_ast_with_scope::<INT>(&mut scope, &ast).unwrap_err();

    assert!(error.to_string().contains("Message"));
    assert!(error.to_string().contains("not available"));
}

#[test]
fn a_compiled_enum_switch_rejects_reordered_runtime_registration() {
    let _other_variant = Drift::Right(0);
    let mut compiler = Engine::new();
    compiler.register_enum::<Drift>("Drift", &[("Left", 1), ("Right", 1)], |value| match value {
        Drift::Left(value) => (0, vec![(*value).into()]),
        Drift::Right(value) => (1, vec![(*value).into()]),
    });
    let ast = compiler.compile("switch value { Drift::Left(value) => value, _ => 7 }").unwrap();

    let mut runtime = Engine::new();
    runtime.register_enum::<Drift>("Drift", &[("Right", 1), ("Left", 1)], |value| match value {
        Drift::Left(value) => (1, vec![(*value).into()]),
        Drift::Right(value) => (0, vec![(*value).into()]),
    });
    let mut scope = Scope::new();
    scope.push("value", Drift::Left(42));

    let error = runtime.eval_ast_with_scope::<INT>(&mut scope, &ast).unwrap_err();
    let message = error.to_string();

    assert!(message.contains("Drift::Left"), "{message}");
    assert!(message.contains("changed shape"), "{message}");
}

#[test]
fn enum_values_remain_opaque_custom_values() {
    let mut engine = engine();
    engine.register_fn("is_idle", |value: Message| matches!(value, Message::Idle(_)));
    let mut scope = Scope::new();
    scope.push("message", Message::Idle(1));

    assert!(engine.eval_with_scope::<bool>(&mut scope, "is_idle(message)").unwrap());
    assert_eq!(engine.eval_with_scope::<String>(&mut scope, "type_of(message)").unwrap(), "Message");
}

#[test]
fn enum_pattern_names_and_arity_are_compile_time_errors() {
    let engine = engine();

    let error = engine.compile("switch value { Missing::Idle(x) => x, _ => 0 }").unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::UnknownEnum(name) if name == "Missing"
    ));
    #[cfg(not(feature = "no_position"))]
    assert!(!error.position().is_none());
    assert!(error.to_string().contains("Missing"));

    let error = engine.compile("switch value { Message::Missing(x) => x, _ => 0 }").unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::UnknownEnumVariant(name, variant)
            if name == "Message" && variant == "Missing"
    ));
    #[cfg(not(feature = "no_position"))]
    assert!(!error.position().is_none());
    assert!(error.to_string().contains("Message"));
    assert!(error.to_string().contains("Missing"));

    let error = engine.compile("switch value { Message::FloorButtonPressed(elevator) => elevator, _ => 0 }").unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::WrongEnumVariantArity(path, 2, 1)
            if path == "Message::FloorButtonPressed"
    ));
    #[cfg(not(feature = "no_position"))]
    assert!(!error.position().is_none());
    assert!(error.to_string().contains("Message::FloorButtonPressed"));
}

#[test]
fn unqualified_variant_call_syntax_is_not_a_pattern() {
    let engine = engine();
    let error = engine.compile("switch value { Idle(elevator) => elevator, _ => 0 }").unwrap_err();

    assert!(matches!(
        error.err_type(),
        ParseErrorType::MissingToken(token, _) if token == "::"
    ));
}

#[test]
fn malformed_extractor_payload_is_a_named_runtime_error() {
    let mut engine = Engine::new();
    engine.register_enum::<Message>("Message", &[("Idle", 1)], |_| (0, vec![]));
    let mut scope = Scope::new();
    scope.push("message", Message::Idle(1));

    let error = engine.eval_with_scope::<INT>(&mut scope, "switch message { Message::Idle(elevator) => elevator, _ => 0 }").unwrap_err();
    assert!(error.to_string().contains("Message::Idle"));
    assert!(error.to_string().contains("expected 1"));
}

#[test]
#[should_panic(expected = "duplicate Rawr enum variant")]
fn duplicate_variant_registration_is_rejected() {
    Engine::new().register_enum::<Message>("Message", &[("Idle", 1), ("Idle", 1)], |_| (0, vec![Dynamic::UNIT]));
}

#[test]
#[cfg(feature = "internals")]
fn ast_walk_visits_enum_case_guards_and_bodies() {
    let engine = engine();
    let ast = engine
        .compile(
            r#"
                switch message {
                    Message::PassingFloor(elevator, _, direction)
                        if direction == expected => record(elevator),
                    _ => 0,
                }
            "#,
        )
        .unwrap();

    let mut variables = Vec::new();
    ast.walk(&mut |nodes: &[ASTNode]| {
        if let Some(ASTNode::Expr(Expr::Variable(info, ..))) = nodes.last() {
            variables.push(info.1.to_string());
        }
        true
    });

    for name in ["message", "direction", "expected", "elevator"] {
        assert!(variables.iter().any(|variable| variable == name), "walk should visit `{name}`");
    }
}

#[test]
#[cfg(feature = "grain")]
fn grain_preserves_enum_switches_as_ast_residuals() {
    use rawr::grain::{Compiler, Vm};

    let engine = engine();
    let ast = engine.compile("switch message { Message::Idle(elevator) => elevator, _ => -1 }").unwrap();
    let program = Compiler::new().compile(&ast);
    let mut scope = Scope::new();
    scope.push("message", Message::Idle(12));

    assert!(program.residual_count() > 0);
    assert_eq!(Vm::new(&engine).eval_with_scope(&mut scope, &program).unwrap().as_int().unwrap(), 12);
}

#[test]
#[cfg(feature = "grain")]
fn grain_failed_function_lowering_is_transactional_across_for_and_try() {
    use rawr::grain::{Compiler, Vm};

    let engine = engine();
    let ast = engine
        .compile(
            r#"
                fn skipped_table(message) {
                    let prefix = switch 1 { 1 => 1, _ => 0 };
                    switch message { Message::Idle(_) => prefix, _ => 0 }
                }
                fn skipped_for(message) {
                    for item in [message] {
                        switch item { Message::Idle(_) => 1, _ => 0 }
                    }
                }
                fn skipped_try(message) {
                    try {
                        switch message { Message::Idle(_) => 1, _ => 0 }
                    } catch {
                        0
                    }
                }
                fn kept() { 42 }
                kept()
            "#,
        )
        .unwrap();
    let program = Compiler::new().compile(&ast);

    assert_eq!(program.residual_count(), 0);
    assert_eq!(Vm::new(&engine).eval(&program).unwrap().as_int().unwrap(), 42);
}
