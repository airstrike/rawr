#![cfg(not(feature = "no_function"))]

#[cfg(not(feature = "no_optimize"))]
use rawr::OptimizationLevel;
use rawr::resolve::Category;
use rawr::{Dynamic, Engine, ParseError, ParseErrorType, Scope};

#[test]
fn nominal_struct_default_fields_and_mutation_execute() {
    let engine = Engine::new();
    let ast = engine.compile(include_str!("../../examples/scripts/nominal_counter.rs")).unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
}

#[test]
fn named_struct_construction_orders_fields_by_the_resolved_schema() {
    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                struct Pair { left: i64, right: bool }
                let pair = Pair { right: true, left: 7 };
                pair.left
            "#,
        )
        .unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 7);
}

#[test]
fn recursively_proven_default_constructs_nested_nominal_fields() {
    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                #[derive(Default)]
                struct Inner { value: i64 }
                #[derive(Default)]
                struct Outer { inner: Inner }

                Outer::default().inner.value
            "#,
        )
        .unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 0);
}

#[test]
fn stale_nested_values_are_rejected_at_construction_and_field_boundaries() {
    let engine = Engine::new();
    let stale_inner = engine.eval::<Dynamic>("struct Inner { value: i64 } Inner { value: 42 }").unwrap();
    let stale_outer = engine
        .eval::<Dynamic>(
            "struct Inner { value: i64 } struct Outer { inner: Inner } \
             Outer { inner: Inner { value: 42 } }",
        )
        .unwrap();
    let ast = engine
        .compile(
            r#"
                struct Inner { value: bool }
                struct Outer { inner: Inner }

                pub fn wrap(inner: Inner) -> Outer { Outer { inner } }
                pub fn read(outer: Outer) -> bool { outer.inner.value }
            "#,
        )
        .unwrap();

    let construction = engine.call_fn::<Dynamic>(&mut Scope::new(), &ast, "wrap", (stale_inner,)).unwrap_err();
    let field = engine.call_fn::<bool>(&mut Scope::new(), &ast, "read", (stale_outer,)).unwrap_err();

    let construction = construction.to_string();
    assert!(construction.contains("member 0 failed validation"), "{construction}");
    assert!(construction.contains("main::Inner has a stale schema"), "{construction}");
    let field = field.to_string();
    assert!(field.contains("main::Inner has a stale schema"), "{field}");
}

#[test]
#[cfg(not(feature = "unchecked"))]
fn data_limits_scan_nested_nominal_fields_and_payloads() {
    let mut engine = Engine::new();
    let value = engine
        .eval::<Dynamic>(
            r#"
                struct Label { text: String }
                enum Envelope { Value(Label) }
                Envelope::Value(Label { text: "too long" })
            "#,
        )
        .unwrap();
    engine.set_max_string_size(4);

    let error = engine.ensure_data_size_within_limits(&value).unwrap_err();

    assert!(error.to_string().contains("Length of string"), "{error}");
}

#[test]
fn qualified_enum_constructors_support_bare_empty_and_payload_forms() {
    let engine = Engine::new();
    let equal = engine
        .compile(
            r#"
                #[derive(Debug, Clone, Copy, PartialEq, Eq)]
                enum Message { Tick, Move(i64, bool) }

                let bare = Message::Tick;
                let empty = Message::Tick();
                bare == empty
            "#,
        )
        .unwrap();
    assert!(engine.eval_ast::<bool>(&equal).unwrap());

    let debug = engine
        .compile(
            r#"
                #[derive(Debug, Clone, Copy, PartialEq, Eq)]
                enum Message { Tick, Move(i64, bool) }

                Message::Move(7, true).debug()
            "#,
        )
        .unwrap();
    assert_eq!(engine.eval_ast::<String>(&debug).unwrap(), "Message::Move(7, true)");
}

#[test]
fn valid_clone_copy_debug_and_equality_capabilities_execute() {
    let engine = Engine::new();
    let equality = engine
        .compile(
            r#"
                #[derive(Debug, Clone, Copy, PartialEq, Eq)]
                struct Point { x: i64, y: bool }

                let point = Point { x: 4, y: true };
                let cloned = point.clone();
                let copied = cloned.copy();
                copied == point
            "#,
        )
        .unwrap();
    assert!(engine.eval_ast::<bool>(&equality).unwrap());

    let debug = engine
        .compile(
            r#"
                #[derive(Debug)]
                struct Point { x: i64, y: bool }
                Point { x: 4, y: true }.debug()
            "#,
        )
        .unwrap();
    assert_eq!(engine.eval_ast::<String>(&debug).unwrap(), "Point { x: 4, y: true }");
}

#[test]
fn structural_capabilities_are_proven_through_recursive_nominal_types() {
    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                #[derive(Debug, Clone, Copy, PartialEq, Eq)]
                enum List { Nil, Cons(i64, List) }

                let list = List::Cons(1, List::Nil);
                let copied = list.clone().copy();
                copied == list
            "#,
        )
        .unwrap();

    assert!(engine.eval_ast::<bool>(&ast).unwrap());
}

#[test]
fn construction_shape_and_types_are_compile_checked() {
    let cases = [
        ("struct Pair { left: i64, right: bool } Pair { left: 1 }", Category::InvalidConstruction, "missing field `right`"),
        ("struct Pair { left: i64 } Pair { left: 1, left: 2 }", Category::InvalidConstruction, "initialized more than once"),
        ("struct Pair { left: i64 } Pair { left: 1, missing: 2 }", Category::UnknownMember, "Pair::missing"),
        ("struct Pair { left: i64 } Pair { left: true }", Category::TypeMismatch, "expected 'i64', found 'bool'"),
        ("enum Message { Move(i64, bool) } Message::Move(1)", Category::InvalidConstruction, "expects 2 value(s), found 1"),
        ("enum Message { Move(i64) } Message::Move(true)", Category::TypeMismatch, "expected 'i64', found 'bool'"),
        ("enum Message { Tick } Message::Missing", Category::UnknownMember, "Message::Missing"),
        ("enum Message { Tick } Message :: Missing", Category::UnknownMember, "Message::Missing"),
    ];

    for (source, category, message) in cases {
        assert_resolution(&Engine::new(), source, category, message);
    }
}

#[test]
fn field_names_types_and_binding_mutability_are_compile_checked() {
    assert_resolution(&Engine::new(), "struct Counter { value: i64 } let counter = Counter { value: 0 }; counter.value = 1;", Category::ImmutableBinding, "immutable binding 'counter'");
    assert_resolution(&Engine::new(), "struct Counter { value: i64 } let mut counter = Counter { value: 0 }; counter.value = true;", Category::TypeMismatch, "expected 'i64', found 'bool'");
    assert_resolution(&Engine::new(), "struct Counter { value: i64 } let counter = Counter { value: 0 }; counter.missing", Category::UnknownMember, "Counter::missing");
}

#[test]
fn duplicate_fields_and_variants_report_the_previous_declaration() {
    for source in ["struct Pair { value: i64, value: bool }", "enum Message { Tick, Tick }"] {
        let error = resolution_error(&Engine::new(), source, Category::DuplicateMember, "Duplicate member");
        assert!(error.secondary_span().is_some());
        assert_ne!(error.primary_span(), error.secondary_span().unwrap());
    }
}

#[test]
fn invalid_derives_fail_at_compile_time() {
    let cases = [
        ("#[derive(Display)] struct Value { inner: i64 }", "unsupported derive"),
        ("#[derive(Default)] enum Message { Tick }", "no default declared"),
        ("#[derive(Copy)] struct Value { inner: i64 }", "Copy requires Clone"),
        ("#[derive(Eq)] struct Value { inner: i64 }", "Eq requires PartialEq"),
        ("struct Inner { value: i64 } #[derive(Clone)] struct Outer { inner: Inner }", "field or variant payload"),
        ("#[derive(Clone, Clone)] struct Value { inner: i64 }", "derived more than once"),
        ("#[derive(Default)] struct Loop { next: Loop }", "field or variant payload"),
    ];

    for (source, message) in cases {
        assert_resolution(&Engine::new(), source, Category::InvalidDerive, message);
    }
}

#[test]
fn opaque_host_members_do_not_gain_implicit_derive_capabilities() {
    #[derive(Clone)]
    struct HostValue;

    let mut engine = Engine::new();
    engine.register_type_with_name::<HostValue>("HostValue");
    assert_resolution(&engine, "#[derive(Clone)] struct Wrapper { value: HostValue }", Category::InvalidDerive, "field or variant payload");
}

#[test]
fn capability_operations_require_the_corresponding_derive() {
    let cases = [
        ("struct Value { inner: i64 } Value::default()", "Default"),
        ("struct Value { inner: i64 } Value { inner: 1 }.clone()", "Clone"),
        ("struct Value { inner: i64 } Value { inner: 1 }.debug()", "Debug"),
        ("struct Value { inner: i64 } Value { inner: 1 } == Value { inner: 1 }", "PartialEq"),
    ];

    for (source, capability) in cases {
        assert_resolution(&Engine::new(), source, Category::MissingCapability, capability);
    }
}

#[test]
fn compiler_private_nominal_transport_cannot_be_called_from_source() {
    assert_resolution(&Engine::new(), "struct Value { inner: i64 } __rawr_nominal_op_0()", Category::UnsupportedNominalSyntax, "compiler-private nominal intrinsic");

    let attempted_private = "struct Value { inner: i64 } \0rawr.nominal.0()";
    assert!(Engine::new().compile(attempted_private).is_err());
}

#[test]
fn ordinary_script_lowering_remains_available_next_to_nominal_declarations() {
    let engine = Engine::new();
    let ast = engine.compile("struct Counter { value: i64 } let answer = [40, 2]; answer[0] + answer[1]").unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
}

#[test]
fn compiler_support_survives_public_ast_statement_transforms() {
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

    assert!(!ast.has_functions());
    assert_eq!(ast.iter_functions().count(), 0);
    assert_eq!(engine.eval_ast::<i64>(&ast.clone_statements_only()).unwrap(), 42);

    let mut retained = ast.clone();
    retained.retain_functions(|_, _, _, _| false);
    assert_eq!(engine.eval_ast::<i64>(&retained).unwrap(), 42);

    let mut cleared = ast;
    cleared.clear_functions();
    assert_eq!(engine.eval_ast::<i64>(&cleared).unwrap(), 42);
}

#[test]
fn compiler_support_is_linked_for_dynamic_eval() {
    let value = Engine::new().eval::<i64>(r#"eval("struct Counter { value: i64 } Counter { value: 42 }.value")"#).unwrap();

    assert_eq!(value, 42);
}

#[test]
fn compiler_support_survives_ast_merge_across_schema_generations() {
    let engine = Engine::new();
    let first = engine.compile("struct Value { inner: i64 } Value { inner: 42 };").unwrap();
    let second = engine.compile("struct Value { inner: bool } Value { inner: true }.inner").unwrap();

    let merged = first.merge(&second);
    assert!(engine.eval_ast::<bool>(&merged).unwrap());

    let mut combined = first;
    combined.combine(second);
    assert!(engine.eval_ast::<bool>(&combined).unwrap());

    let nested_first = engine
        .compile(
            "struct Inner { value: i64 } struct Outer { inner: Inner } \
             Outer { inner: Inner { value: 42 } }.inner.value;",
        )
        .unwrap();
    let nested_second = engine
        .compile(
            "struct Inner { value: bool } struct Outer { inner: Inner } \
             Outer { inner: Inner { value: true } }.inner.value",
        )
        .unwrap();

    assert!(engine.eval_ast::<bool>(&nested_first.merge(&nested_second)).unwrap());
}

#[test]
#[cfg(not(feature = "no_optimize"))]
fn reoptimization_preserves_nominal_support_and_proofs() {
    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                struct Counter { value: i64 }
                enum Message { Add(i64) }
                let mut counter = Counter { value: 0 };
                let message = Message::Add(42);
                let amount = match message { Message::Add(value) => value };
                counter.value = amount;
                counter.value
            "#,
        )
        .unwrap();

    for level in [OptimizationLevel::Simple, OptimizationLevel::Full] {
        let optimized = engine.optimize_ast(&Scope::new(), ast.clone(), level);
        let debug = format!("{optimized:#?}");

        assert!(!debug.contains("__rawr_nominal_"), "{debug}");
        assert!(!debug.contains("Marker("), "{debug}");
        assert_eq!(engine.eval_ast::<i64>(&optimized).unwrap(), 42);
    }
}

fn assert_resolution(engine: &Engine, source: &str, category: Category, message: &str) {
    let _ = resolution_error(engine, source, category, message);
}

fn resolution_error(engine: &Engine, source: &str, category: Category, message: &str) -> ParseError {
    let error = engine.compile(source).unwrap_err();
    let ParseErrorType::Resolution(resolution) = error.err_type() else {
        panic!("expected resolution error, got {error:?}");
    };

    assert_eq!(resolution.category(), category, "{error}");
    assert!(error.to_string().contains(message), "diagnostic `{error}` did not contain `{message}`");
    assert!(!error.primary_span().is_none());
    error
}
