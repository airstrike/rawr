use rawr::resolve::Category;
#[cfg(feature = "internals")]
use rawr::{ASTNode, EnumCase, Expr, Stmt};
use rawr::{Dynamic, Engine, ParseError, ParseErrorType, Position, Scope, host};

#[test]
fn rust_shaped_counter_update_matches_unit_variants() {
    let engine = Engine::new();
    let ast = engine.compile(include_str!("../../examples/scripts/nominal_counter.rs")).unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
}

#[test]
fn payloads_discards_guards_blocks_and_shadowing_are_case_local() {
    let engine = Engine::new();
    let value = engine
        .eval::<i64>(
            r#"
                enum Message { Move(i64, i64), Idle }

                pub fn choose(message: Message) -> i64 {
                    let floor = 100;
                    let selected = match message {
                        Message::Move(_, floor) if floor == 7 => {
                            let adjusted = floor + 1;
                            adjusted
                        },
                        Message::Move(elevator, _) => elevator,
                        Message::Idle => 0,
                    };
                    selected + floor - 100
                }

                choose(Message::Move(5, 7))
            "#,
        )
        .unwrap();

    assert_eq!(value, 8);
}

#[test]
fn nominal_match_branch_calls_are_lazy_and_execute_once() {
    let value = Engine::new()
        .eval::<i64>(
            r#"
                enum Choice { First, Second }
                struct Probe { calls: i64 }

                impl Probe {
                    fn mark(&mut self, value: i64) -> i64 {
                        self.calls += 1;
                        value
                    }

                    fn choose(&mut self, choice: Choice) -> i64 {
                        match choice {
                            Choice::First => self.mark(40),
                            Choice::Second => self.mark(2),
                        }
                    }
                }

                let mut probe = Probe { calls: 0 };
                let selected = probe.choose(Choice::Second);
                selected + probe.calls
            "#,
        )
        .unwrap();

    assert_eq!(value, 3);
}

#[test]
fn nominal_match_assigns_a_selected_enum_variant_to_state() {
    let value = Engine::new()
        .eval::<i64>(
            r#"
                enum Tab { Start, TextInput }
                enum Message { SelectStart, SelectTextInput }
                struct State { tab: Tab }
                impl State {
                    fn update(&mut self, message: Message) {
                        match message {
                            Message::SelectStart => self.tab = Tab::Start,
                            Message::SelectTextInput => self.tab = Tab::TextInput,
                        }
                    }
                    fn page(&self) -> i64 {
                        match self.tab {
                            Tab::Start => 0,
                            Tab::TextInput => 1,
                        }
                    }
                }
                let mut state = State { tab: Tab::Start };
                state.update(Message::SelectTextInput);
                state.page()
            "#,
        )
        .unwrap();

    assert_eq!(value, 1);
}

#[test]
fn nominal_match_scrutinee_and_selected_arm_execute_once() {
    let value = Engine::new()
        .eval::<i64>(
            r#"
                enum Choice { Selected, Unselected }
                struct Probe { calls: i64 }
                impl Probe {
                    fn choose_subject(&mut self) -> Choice {
                        self.calls += 1;
                        Choice::Selected
                    }
                    fn selected(&mut self) -> i64 { self.calls += 1; 40 }
                    fn unselected(&mut self) -> i64 { panic!("unselected arm ran") }
                    fn choose(&mut self) -> i64 {
                        match self.choose_subject() {
                            Choice::Selected => self.selected(),
                            Choice::Unselected => self.unselected(),
                        }
                    }
                }
                let mut probe = Probe { calls: 0 };
                probe.choose() + probe.calls
            "#,
        )
        .unwrap();

    assert_eq!(value, 42);
}

#[test]
fn nested_if_and_match_values_compose_on_both_sides() {
    let value = Engine::new()
        .eval::<i64>(
            r#"
                enum Choice { First, Second }
                struct Probe { calls: i64 }
                impl Probe {
                    fn condition(&mut self, selected: bool) -> bool {
                        self.calls += 1;
                        selected
                    }
                    fn value(&mut self, selected: i64) -> i64 {
                        self.calls += 1;
                        selected
                    }
                    fn fail(&mut self) -> i64 { panic!("unselected nested branch ran") }
                    fn choose(&mut self, outer: bool, choice: Choice) -> i64 {
                        if outer {
                            match choice {
                                Choice::First => if self.condition(true) { self.value(10) } else { self.fail() },
                                Choice::Second => if self.condition(true) { self.value(20) } else { self.fail() },
                            }
                        } else {
                            match choice {
                                Choice::First => if self.condition(false) { self.fail() } else { self.value(30) },
                                Choice::Second => if self.condition(false) { self.fail() } else { self.value(40) },
                            }
                        }
                    }
                }
                let mut probe = Probe { calls: 0 };
                let total = probe.choose(true, Choice::First)
                    + probe.choose(true, Choice::Second)
                    + probe.choose(false, Choice::First)
                    + probe.choose(false, Choice::Second);
                total + probe.calls
            "#,
        )
        .unwrap();

    assert_eq!(value, 108);
}

#[test]
fn a_guarded_variant_can_decline_to_a_later_arm() {
    let engine = Engine::new();
    let value = engine
        .eval::<i64>(
            r#"
                enum Message { Value(i64) }

                pub fn choose(message: Message) -> i64 {
                    match message {
                        Message::Value(value) if value == 0 => 0,
                        Message::Value(value) => value,
                    }
                }

                choose(Message::Value(42))
            "#,
        )
        .unwrap();

    assert_eq!(value, 42);
}

#[test]
fn zero_payload_variants_accept_empty_parentheses() {
    let value = Engine::new()
        .eval::<i64>(
            r#"
                enum Message { Tick }
                let message = Message::Tick;
                match message { Message::Tick() => 42 }
            "#,
        )
        .unwrap();

    assert_eq!(value, 42);
}

#[test]
fn recursive_enum_patterns_bind_discard_guard_and_fall_through_atomically() {
    let value = Engine::new()
        .eval::<i64>(
            r#"
                enum Child { Keep(i64), Skip(i64) }
                enum Envelope { Pair(i64, Child) }

                pub fn choose(message: Envelope) -> i64 {
                    match message {
                        Envelope::Pair(prefix, Child::Keep(value)) if value == 40 => prefix + value,
                        Envelope::Pair(prefix, Child::Skip(_)) => prefix,
                        _ => 0,
                    }
                }

                choose(Envelope::Pair(2, Child::Keep(40)))
                    + choose(Envelope::Pair(7, Child::Skip(99)))
            "#,
        )
        .unwrap();

    assert_eq!(value, 49);
}

#[test]
fn recursively_irrefutable_single_variant_patterns_are_exhaustive() {
    let value = Engine::new()
        .eval::<i64>(
            r#"
                enum Leaf { Value(i64) }
                enum Middle { Wrap(Leaf) }
                enum Root { Wrap(Middle) }

                let message = Root::Wrap(Middle::Wrap(Leaf::Value(42)));
                match message {
                    Root::Wrap(Middle::Wrap(Leaf::Value(value))) => value,
                }
            "#,
        )
        .unwrap();

    assert_eq!(value, 42);
}

#[test]
fn recursive_constructor_matrix_proves_nested_unions_and_cross_products() {
    let value = Engine::new()
        .eval::<i64>(
            r#"
                enum Child { First, Second }
                enum Root { Wrap(Child) }
                enum Bit { Zero, One }
                enum Bits { Pair(Bit, Bit) }

                pub fn inspect(value: Root) -> i64 {
                    match value {
                        Root::Wrap(Child::First) => 1,
                        Root::Wrap(Child::Second) => 2,
                    }
                }

                pub fn classify(value: Bits) -> i64 {
                    match value {
                        Bits::Pair(Bit::Zero, Bit::Zero) => 1,
                        Bits::Pair(Bit::Zero, Bit::One) => 2,
                        Bits::Pair(Bit::One, Bit::Zero) => 4,
                        Bits::Pair(Bit::One, Bit::One) => 8,
                    }
                }

                inspect(Root::Wrap(Child::First))
                    + inspect(Root::Wrap(Child::Second))
                    + classify(Bits::Pair(Bit::Zero, Bit::Zero))
                    + classify(Bits::Pair(Bit::Zero, Bit::One))
                    + classify(Bits::Pair(Bit::One, Bit::Zero))
                    + classify(Bits::Pair(Bit::One, Bit::One))
            "#,
        )
        .unwrap();

    assert_eq!(value, 18);
}

#[test]
fn recursive_constructor_matrix_reports_nested_missing_witnesses_and_guarded_gaps() {
    let partial = assert_resolution(
        r#"
            enum Bit { Zero, One }
            enum Bits { Pair(Bit, Bit) }
            pub fn inspect(value: Bits) -> i64 {
                match value {
                    Bits::Pair(Bit::Zero, Bit::Zero) => 1,
                    Bits::Pair(Bit::Zero, Bit::One) => 2,
                    Bits::Pair(Bit::One, Bit::Zero) => 3,
                }
            }
        "#,
        Category::InvalidMatch,
        "non-exhaustive patterns; missing Bits::Pair(Bit::One, Bit::One)",
    );
    assert!(!partial.primary_span().is_none());

    let guarded = assert_resolution(
        r#"
            enum Child { First, Second }
            enum Root { Wrap(Child) }
            pub fn inspect(value: Root) -> i64 {
                match value {
                    Root::Wrap(Child::First) if true => 1,
                    Root::Wrap(Child::Second) => 2,
                }
            }
        "#,
        Category::InvalidMatch,
        "non-exhaustive patterns; missing Root::Wrap(Child::First)",
    );
    assert!(!guarded.primary_span().is_none());
}

#[test]
fn recursive_constructor_matrix_rejects_binding_and_discard_subsumption() {
    let cases = [
        r#"
            enum Child { First, Second }
            enum Root { Wrap(Child) }
            pub fn inspect(value: Root) -> i64 {
                match value {
                    Root::Wrap(child) => 1,
                    Root::Wrap(Child::First) => 2,
                }
            }
        "#,
        r#"
            enum Child { First, Second }
            enum Root { Wrap(Child) }
            pub fn inspect(value: Root) -> i64 {
                match value {
                    Root::Wrap(_) => 1,
                    Root::Wrap(Child::First) => 2,
                }
            }
        "#,
    ];

    for source in cases {
        let error = assert_resolution(source, Category::InvalidMatch, "pattern `Root::Wrap(Child::First)` is unreachable after an unguarded arm");
        // As in rustc, the unreachable pattern is reported from its path.
        assert_eq!(error.primary_span().start(), position_of(source, "Root::Wrap(Child::First) => 2"));
    }
}

#[test]
fn recursive_pattern_owner_variant_arity_type_and_binding_failures_are_positioned() {
    let cases = [
        ("enum Inner { Value } enum Outer { Wrap(Inner) } pub fn f(value: Outer) { match value { Outer::Wrap(Missing::Value) => () } }", "Missing", Category::UnknownType, "Missing"),
        (
            "enum Inner { Value } enum Outer { Wrap(Inner) } pub fn f(value: Outer) { match value { Outer::Wrap(Inner::Missing) => () } }",
            "Missing",
            Category::UnknownMember,
            "Inner::Missing",
        ),
        (
            "enum Inner { Value(i64) } enum Outer { Wrap(Inner) } pub fn f(value: Outer) { match value { Outer::Wrap(Inner::Value) => () } }",
            "Value) =>",
            Category::InvalidMatch,
            "expects 1 binding(s), found 0",
        ),
        (
            "enum Inner { Value } enum Other { Value } enum Outer { Wrap(Inner) } pub fn f(value: Outer) { match value { Outer::Wrap(Other::Value) => () } }",
            "Other::Value",
            Category::TypeMismatch,
            "expected 'Inner', found 'Other'",
        ),
        (
            "enum Inner { Value } enum Outer { Wrap(i64) } pub fn f(value: Outer) { match value { Outer::Wrap(Inner::Value) => () } }",
            "Inner::Value",
            Category::TypeMismatch,
            "expected 'i64', found 'Inner'",
        ),
        (
            "enum Inner { Value(i64) } enum Outer { Pair(i64, Inner) } pub fn f(value: Outer) { match value { Outer::Pair(value, Inner::Value(value)) => () } }",
            "value))",
            Category::InvalidMatch,
            "binding `value` appears more than once",
        ),
    ];

    for (source, marker, category, message) in cases {
        let error = assert_resolution(source, category, message);
        assert_eq!(error.primary_span().start(), position_of(source, marker));
    }
}

#[test]
fn stale_recursive_payload_schema_is_rejected_before_nested_bindings() {
    let engine = Engine::new();
    let stale = engine
        .eval::<Dynamic>(
            r#"
                enum Inner { Value(i64) }
                enum Outer { Pair(i64, Inner) }
                Outer::Pair(1, Inner::Value(2))
            "#,
        )
        .unwrap();
    let ast = engine
        .compile(
            r#"
                enum Inner { Value(bool) }
                enum Outer { Pair(i64, Inner) }
                pub fn inspect(value: Outer) -> i64 {
                    match value {
                        Outer::Pair(prefix, Inner::Value(_)) => prefix,
                    }
                }
            "#,
        )
        .unwrap();

    let error = engine.call_fn::<i64>(&mut Scope::new(), &ast, "inspect", (stale,)).unwrap_err();
    let message = error.to_string();

    assert!(message.contains("Outer"), "{message}");
    assert!(message.contains("payload"), "{message}");
}

#[test]
fn stale_recursive_variant_shape_is_rejected_before_nested_bindings() {
    let engine = Engine::new();
    let stale = engine
        .eval::<Dynamic>(
            r#"
                enum Inner { Old }
                enum Outer { Pair(i64, Inner) }
                Outer::Pair(1, Inner::Old)
            "#,
        )
        .unwrap();
    let ast = engine
        .compile(
            r#"
                enum Inner { New }
                enum Outer { Pair(i64, Inner) }
                pub fn inspect(value: Outer) -> i64 {
                    match value {
                        Outer::Pair(prefix, Inner::New) => prefix,
                    }
                }
            "#,
        )
        .unwrap();

    let error = engine.call_fn::<i64>(&mut Scope::new(), &ast, "inspect", (stale,)).unwrap_err();
    let message = error.to_string();

    assert!(message.contains("Outer"), "{message}");
    assert!(message.contains("payload"), "{message}");
}

#[test]
fn wildcard_is_a_runtime_fallthrough_for_a_different_nominal_identity() {
    let engine = Engine::new();
    let other = engine.eval::<Dynamic>("enum Other { Tick } Other::Tick").unwrap();
    let ast = engine
        .compile(
            r#"
                enum Message { Tick }
                pub fn inspect(message: Message) -> i64 {
                    match message {
                        Message::Tick => 1,
                        _ => 7,
                    }
                }
            "#,
        )
        .unwrap();

    assert_eq!(engine.call_fn::<i64>(&mut Scope::new(), &ast, "inspect", (other,)).unwrap(), 7);
    assert_eq!(engine.call_fn::<i64>(&mut Scope::new(), &ast, "inspect", (42_i64,)).unwrap(), 7);
}

#[test]
fn same_identity_with_a_stale_schema_is_a_runtime_error() {
    let engine = Engine::new();
    let stale = engine.eval::<Dynamic>("enum Message { Value(i64) } Message::Value(42)").unwrap();
    let ast = engine
        .compile(
            r#"
                enum Message { Value(i64, i64) }
                pub fn inspect(message: Message) -> i64 {
                    match message {
                        Message::Value(left, right) => left + right,
                    }
                }
            "#,
        )
        .unwrap();

    let error = engine.call_fn::<i64>(&mut Scope::new(), &ast, "inspect", (stale,)).unwrap_err();

    assert!(error.to_string().contains("Message"));
    assert!(error.to_string().contains("stale schema"));

    let wildcard_ast = engine
        .compile(
            r#"
                enum Message { Value(i64, i64) }
                pub fn inspect_any(message: Message) -> i64 {
                    match message { _ => 7 }
                }
            "#,
        )
        .unwrap();
    let error = engine
        .call_fn::<i64>(&mut Scope::new(), &wildcard_ast, "inspect_any", (engine.eval::<Dynamic>("enum Message { Value(i64) } Message::Value(1)").unwrap(),))
        .unwrap_err();
    assert!(error.to_string().contains("stale schema"));
}

#[test]
fn stale_nested_payload_schema_is_rejected_before_binding() {
    let engine = Engine::new();
    let stale = engine
        .eval::<Dynamic>(
            r#"
                struct Payload { value: i64 }
                enum Message { Value(Payload) }
                Message::Value(Payload { value: 42 })
            "#,
        )
        .unwrap();
    let ast = engine
        .compile(
            r#"
                struct Payload { value: bool }
                enum Message { Value(Payload) }
                pub fn inspect(message: Message) -> i64 {
                    match message { Message::Value(_) => 1 }
                }
            "#,
        )
        .unwrap();

    let error = engine.call_fn::<i64>(&mut Scope::new(), &ast, "inspect", (stale,)).unwrap_err();

    assert!(error.to_string().contains("Message::Value"));
    assert!(error.to_string().contains("payload"));
}

#[test]
fn names_arity_owner_and_arm_types_are_compile_checked() {
    let cases = [
        ("enum Message { Tick } pub fn f(message: Message) { match message { Missing::Tick => () } }", Category::UnknownType, "Missing"),
        ("enum Message { Tick } pub fn f(message: Message) { match message { Message::Missing => () } }", Category::UnknownMember, "Message::Missing"),
        ("enum Message { Move(i64, i64) } pub fn f(message: Message) { match message { Message::Move(value) => () } }", Category::InvalidMatch, "expects 2 binding(s), found 1"),
        (
            "enum Message { Tick, Tock } pub fn f(message: Message) -> i64 { match message { Message::Tick => 1, Message::Tock => false } }",
            Category::TypeMismatch,
            "expected '{integer}', found 'bool'",
        ),
        ("enum Message { Tick } enum Other { Tick } pub fn f(message: Message) { match message { Other::Tick => () } }", Category::TypeMismatch, "expected 'Message', found 'Other'"),
    ];

    for (source, category, message) in cases {
        let _ = assert_resolution(source, category, message);
    }

    let source = "enum Message { Tick, Tock } pub fn f(message: Message) -> i64 { match message { Message::Tick => 1, Message::Tock => false } }";
    let error = assert_resolution(source, Category::TypeMismatch, "expected '{integer}', found 'bool'");
    assert_eq!(error.primary_span().start(), position_of(source, "false"));
}

#[test]
fn multiple_common_host_projections_are_rejected_without_inference_leakage() {
    let mut engine = Engine::new();
    engine.register_host_facade(ambiguous_projection_facade()).unwrap();
    let source = r#"
use ambiguous::{left, right};
enum Choice { Left, Right }
pub fn choose(choice: Choice) {
    let selected = match choice {
        Choice::Left => left(),
        Choice::Right => right(),
    };
    ()
}
"#;
    let error = engine.compile(source).unwrap_err();
    let ParseErrorType::Resolution(resolution) = error.err_type() else {
        panic!("expected resolution error, got {error:?}");
    };
    assert_eq!(resolution.category(), Category::UnsupportedNominalSyntax);
    assert!(error.to_string().contains("more than one common host projection"), "{error}");

    engine
        .compile(
            r#"
use ambiguous::left;
pub fn exact() -> ambiguous::Left { left() }
"#,
        )
        .unwrap();
}

#[test]
fn fresh_projection_arguments_are_per_site_and_context_inferred() {
    let mut engine = Engine::new();
    engine.register_host_facade(fresh_projection_facade(false)).unwrap();
    engine
        .compile_executable(
            r#"
use poly::{Element, text, space};
enum First { Tick }
enum Second { Tick }
pub fn first() -> Element<First> { text() }
pub fn second() -> Element<Second> { text() }
pub fn choose(flag: bool) -> Element<First> {
    if flag { text() } else { space() }
}
"#,
        )
        .unwrap();
}

#[test]
fn fresh_common_projection_ambiguity_is_transactional_and_positioned() {
    let mut engine = Engine::new();
    engine.register_host_facade(fresh_projection_facade(true)).unwrap();
    let source = r#"
use poly::{text, space};
pub fn ambiguous(flag: bool) {
    let selected = if flag { text() } else { space() };
    ()
}
"#;
    let error = engine.compile_executable(source).unwrap_err();
    let ParseErrorType::Resolution(resolution) = error.err_type() else {
        panic!("expected resolution error, got {error:?}");
    };
    assert_eq!(resolution.category(), Category::UnsupportedNominalSyntax);
    assert!(error.to_string().contains("more than one common host projection"));
    assert!(!error.primary_span().is_none());

    engine
        .compile_executable(
            r#"
use poly::{Element, text};
enum Message { Tick }
pub fn exact() -> Element<Message> { text() }
"#,
        )
        .unwrap();

    let incompatible = r#"
use poly::{Element, text};
enum Message { Tick }
pub fn invalid() -> Element<Message> { if true { text() } else { false } }
"#;
    let error = engine.compile_executable(incompatible).unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolution)
            if resolution.category() == Category::TypeMismatch
    ));
    assert_eq!(error.primary_span().start(), position_of(incompatible, "false"));
}

#[test]
fn exhaustiveness_duplicates_unreachable_arms_and_binding_escape_are_rejected() {
    let cases = [
        ("enum Message { Tick, Tock } pub fn f(message: Message) { match message { Message::Tick => () } }", Category::InvalidMatch, "non-exhaustive patterns; missing Message::Tock"),
        ("enum Message { Tick } pub fn f(message: Message) { match message { Message::Tick if true => () } }", Category::InvalidMatch, "non-exhaustive patterns; missing Message::Tick"),
        (
            "enum Message { Tick } pub fn f(message: Message) { match message { Message::Tick => (), Message::Tick => () } }",
            Category::InvalidMatch,
            "unreachable after an unguarded arm",
        ),
        ("enum Message { Tick } pub fn f(message: Message) { match message { _ => (), Message::Tick => () } }", Category::InvalidMatch, "patterns after `_` are unreachable"),
        (
            "enum Message { Value(i64) } pub fn f(message: Message) -> i64 { let selected = match message { Message::Value(value) => value }; value }",
            Category::UnknownVariable,
            "Unknown variable 'value'",
        ),
    ];

    for (source, category, message) in cases {
        let _ = assert_resolution(source, category, message);
    }
}

#[test]
fn compiler_only_match_sentinels_cannot_be_authored() {
    let _ = assert_resolution("enum Message { Tick } __rawr_nominal_match_0::Tick", Category::UnsupportedNominalSyntax, "compiler-private nominal intrinsic");
}

#[test]
fn successful_nominal_match_ast_debug_contains_only_stable_pattern_names() {
    let ast = Engine::new()
        .compile(
            r#"
                enum Message { Value(i64) }
                let message = Message::Value(42);
                match message { Message::Value(value) => value }
            "#,
        )
        .unwrap();
    let debug = format!("{ast:#?}");

    assert!(!debug.contains("__rawr_nominal_match_"), "{debug}");
    assert!(debug.contains("Message"), "{debug}");
    assert_eq!(Engine::new().eval_ast::<i64>(&ast).unwrap(), 42);
}

#[test]
#[cfg(feature = "internals")]
fn nominal_pattern_walk_positions_and_hashes_retain_the_private_proof() {
    use std::hash::{DefaultHasher, Hash, Hasher};

    fn first_case(ast: &rawr::AST) -> EnumCase {
        let mut found = None;
        ast.walk(&mut |path| {
            if let Some(ASTNode::Stmt(Stmt::Switch(value, ..))) = path.last() {
                found = value.1.enum_cases.first().cloned();
            }
            true
        });
        found.expect("compiled nominal match should retain an enum case")
    }

    fn hash(case: &EnumCase) -> u64 {
        let mut hasher = DefaultHasher::new();
        case.hash(&mut hasher);
        hasher.finish()
    }

    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                enum Message { Value(i64) }
                let message = Message::Value(42);
                match message {
                    Message::Value(value) if value == 42 => value,
                    Message::Value(value) => value,
                }
            "#,
        )
        .unwrap();
    let case = first_case(&ast);

    assert_eq!(case.enum_name, "Message");
    assert_eq!(case.variant_name, "Value");
    assert_eq!(case.variant_index, 0);

    let mut visited = Vec::new();
    ast.walk(&mut |path| {
        if let Some(ASTNode::Expr(Expr::Variable(variable, ..))) = path.last() {
            visited.push((variable.1.to_string(), path.last().unwrap().position()));
        }
        true
    });
    assert!(visited.iter().any(|(name, position)| name == "value" && !position.is_none()));

    let changed_schema = engine
        .compile(
            r#"
                enum Message { Value(i64), Idle }
                let message = Message::Value(42);
                match message {
                    Message::Value(value) => value,
                    _ => 0,
                }
            "#,
        )
        .unwrap();
    let changed_case = first_case(&changed_schema);

    assert_eq!(case.enum_name, changed_case.enum_name);
    assert_eq!(case.variant_name, changed_case.variant_name);
    assert_eq!(case.variant_index, changed_case.variant_index);
    assert_ne!(hash(&case), hash(&changed_case));
}

#[test]
#[cfg(feature = "grain")]
fn grain_preserves_nominal_matches_as_ast_residuals() {
    use rawr::grain::{Compiler, Vm};

    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                enum Message { Value(i64) }
                let message = Message::Value(42);
                match message { Message::Value(value) => value }
            "#,
        )
        .unwrap();
    let program = Compiler::new().compile(&ast);

    assert!(program.residual_count() > 0);
    assert_eq!(Vm::new(&engine).eval(&program).unwrap().as_int().unwrap(), 42);
}

#[test]
#[cfg(feature = "grain")]
fn grain_links_nominal_support_for_fully_lowered_wildcards() {
    use rawr::grain::format::WriteError;
    use rawr::grain::{Compiler, Vm};

    let engine = Engine::new();
    for source in [
        r#"
            enum Message { Value(i64) }
            let message = Message::Value(42);
            match message { _ => 42 }
        "#,
        r#"
            enum Message { Value(i64) }
            let message = Message::Value(1);
            match message {
                Message::Value(_) if false => 0,
                _ => 42,
            }
        "#,
    ] {
        let ast = engine.compile(source).unwrap();
        let program = Compiler::new().compile(&ast);

        assert_eq!(Vm::new(&engine).eval(&program).unwrap().as_int().unwrap(), 42);
        assert!(matches!(program.write(), Err(WriteError::HasCompilerSupport)), "nominal support must be an explicit artifact boundary: {:?}", program.write());
    }
}

#[test]
#[cfg(feature = "grain")]
fn grain_wildcard_match_rejects_stale_nested_schema() {
    use rawr::grain::{Compiler, Vm};

    let engine = Engine::new();
    let stale = engine.eval::<Dynamic>("enum Message { Value(i64) } Message::Value(42)").unwrap();
    let ast = engine
        .compile(
            r#"
                enum Message { Value(bool) }
                pub fn inspect(message: Message) -> i64 {
                    match message { _ => 7 }
                }
            "#,
        )
        .unwrap();
    let program = Compiler::new().compile(&ast);

    let error = Vm::new(&engine).call_fn::<i64>(&mut Scope::new(), &program, "inspect", (stale,)).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("Message"), "{message}");
    assert!(message.contains("stale schema"), "{message}");
}

fn assert_resolution(source: &str, category: Category, message: &str) -> ParseError {
    let error = Engine::new().compile(source).unwrap_err();
    let ParseErrorType::Resolution(resolution) = error.err_type() else {
        panic!("expected resolution error, got {error:?}");
    };

    assert_eq!(resolution.category(), category, "{error}");
    // The structured message: Display prints the rustc-style report.
    assert!(error.err_type().to_string().contains(message), "diagnostic `{error}` did not contain `{message}`");
    assert!(!error.primary_span().is_none());
    error
}

fn position_of(source: &str, marker: &str) -> Position {
    let offset = source.find(marker).expect("diagnostic marker must exist");
    let prefix = &source[..offset];
    let line = prefix.chars().filter(|character| *character == '\n').count() + 1;
    let column = prefix.rsplit_once('\n').map_or(prefix, |(_, line)| line).chars().count() + 1;

    Position::new(u16::try_from(line).expect("test source line must fit"), u16::try_from(column).expect("test source column must fit"))
}

struct Left;
struct Right;
struct FirstTarget;
struct SecondTarget;
struct PolyElement;
struct PolyAlternate;
struct PolyText;
struct PolySpace;

fn ambiguous_projection_facade() -> host::facade::Definition {
    let mut builder = host::facade::Builder::new("ambiguous", host::facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<Left>("Left", &[], &[]).unwrap();
    builder.type_::<Right>("Right", &[], &[]).unwrap();
    builder.type_::<FirstTarget>("FirstTarget", &[], &[]).unwrap();
    builder.type_::<SecondTarget>("SecondTarget", &[], &[]).unwrap();
    for source in ["Left", "Right"] {
        builder.project(source, "FirstTarget", &[]).unwrap();
        builder.project(source, "SecondTarget", &[]).unwrap();
    }
    for (name, output) in [("left", "Left"), ("right", "Right")] {
        let mut signature = host::signature::Builder::new();
        signature.output(host::type_ref::Type::registered(output).unwrap());
        builder.item(host::item::Definition::function(name, signature.finish()).unwrap()).unwrap();
    }
    builder.finish().unwrap()
}

fn fresh_projection_facade(ambiguous: bool) -> host::facade::Definition {
    let mut builder = host::facade::Builder::new("poly", host::facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<PolyElement>("Element", &["Message"], &[]).unwrap();
    builder.type_::<PolyText>("Text", &[], &[]).unwrap();
    builder.type_::<PolySpace>("Space", &[], &[]).unwrap();
    if ambiguous {
        builder.type_::<PolyAlternate>("Alternate", &["Message"], &[]).unwrap();
    }
    for source in ["Text", "Space"] {
        builder.project_with(source, "Element", &[host::type_def::Argument::fresh()]).unwrap();
        if ambiguous {
            builder.project_with(source, "Alternate", &[host::type_def::Argument::fresh()]).unwrap();
        }
    }
    for (name, output) in [("text", "Text"), ("space", "Space")] {
        let mut signature = host::signature::Builder::new();
        signature.output(host::type_ref::Type::registered(output).unwrap());
        builder.item(host::item::Definition::function(name, signature.finish()).unwrap()).unwrap();
    }
    builder.finish().unwrap()
}
