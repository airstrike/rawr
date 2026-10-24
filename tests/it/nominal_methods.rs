#![cfg(not(feature = "no_function"))]

use rawr::resolve::Category;
use rawr::{Dynamic, Engine, ParseError, ParseErrorType, Position, Scope};

#[test]
fn mutable_and_shared_receivers_execute_with_typed_parameters_and_outputs() {
    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                #[derive(Default)]
                struct Counter { value: i64 }

                impl Counter {
                    fn add_twice(&mut self, delta: i64) {
                        self.add(delta);
                        self.add(delta);
                    }

                    fn add(&mut self, delta: i64) {
                        self.value += delta;
                    }

                    fn value(&self) -> i64 {
                        self.value
                    }
                }

                let mut counter = Counter::default();
                counter.add_twice(21);
                counter.value()
            "#,
        )
        .unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
}

#[test]
fn mutable_field_receivers_preserve_the_call_result_and_write_back_the_child() {
    let engine = Engine::new();
    let result = engine
        .eval::<i64>(
            r#"
                struct Child { value: i64 }
                struct Parent { child: Child }

                impl Child {
                    fn add(&mut self, delta: i64) -> i64 {
                        self.value += delta;
                        self.value
                    }

                    fn value(&self) -> i64 {
                        self.value
                    }
                }

                impl Parent {
                    fn add(&mut self, delta: i64) -> i64 {
                        self.child.add(delta)
                    }
                }

                let mut parent = Parent { child: Child { value: 1 } };
                let result = parent.add(20);
                result + parent.child.value()
            "#,
        )
        .unwrap();

    assert_eq!(result, 42);
}

#[test]
fn mutable_child_updates_compose_through_a_parent_match_arm() {
    let engine = Engine::new();
    let result = engine
        .eval::<i64>(
            r#"
                struct Child { value: i64 }
                struct Parent { child: Child }
                enum Message { Increment }

                impl Child {
                    fn update(&mut self) {
                        self.value += 1;
                    }

                    fn value(&self) -> i64 {
                        self.value
                    }
                }

                impl Parent {
                    fn update(&mut self, message: Message) {
                        match message {
                            Message::Increment => self.child.update(),
                        }
                    }
                }

                let mut parent = Parent { child: Child { value: 41 } };
                parent.update(Message::Increment);
                parent.child.value()
            "#,
        )
        .unwrap();

    assert_eq!(result, 42);
}

#[test]
fn receiver_references_do_not_coerce_to_owned_values() {
    for (source, actual) in [
        ("struct Counter { value: i64 } impl Counter { fn reject(&self) -> Counter { self } }", "found '&Counter'"),
        ("struct Counter { value: i64 } impl Counter { fn reject(&mut self) -> Counter { self } }", "found '&mut Counter'"),
    ] {
        let error = resolution_error(&Engine::new(), source, Category::TypeMismatch, "expected 'Counter'");
        assert!(error.err_type().to_string().contains(actual), "{error}");
    }
}

#[test]
fn associated_functions_and_derived_default_are_both_callable() {
    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                #[derive(Default)]
                struct Counter { value: i64 }

                impl Counter {
                    fn new(value: i64) -> Counter {
                        Counter { value }
                    }

                    fn value(&self) -> i64 {
                        self.value
                    }
                }

                let initial = Counter::default();
                Counter::new(42).value() + initial.value()
            "#,
        )
        .unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
}

#[test]
fn nominal_operations_locals_and_shadowing_work_inside_method_bodies() {
    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                struct Counter { value: i64 }

                impl Counter {
                    fn shifted(&self, input: i64) -> i64 {
                        let value = input + 1;
                        self.value + value
                    }
                }

                let value = 100;
                let counter = Counter { value: 1 };
                counter.shifted(40) + value - 100
            "#,
        )
        .unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
}

#[test]
fn method_owner_arguments_arity_and_call_form_are_compile_checked() {
    let cases = [
        (
            "struct Counter { value: i64 } impl Counter { fn set(&mut self, value: i64) {} } let mut counter = Counter { value: 0 }; counter.set(true);",
            Category::TypeMismatch,
            "expected 'i64', found 'bool'",
        ),
        (
            "struct Counter { value: i64 } impl Counter { fn set(&mut self, value: i64) {} } let mut counter = Counter { value: 0 }; counter.set();",
            Category::InvalidMethodCall,
            "expects 1 argument(s), found 0",
        ),
        (
            "struct Counter { value: i64 } struct Other { value: i64 } impl Counter { fn value(&self) -> i64 { self.value } } Other { value: 1 }.value()",
            Category::UnknownMember,
            "Other::value",
        ),
        (
            "struct Counter { value: i64 } impl Counter { fn make(value: i64) -> Counter { Counter { value } } } Counter { value: 0 }.make(1)",
            Category::InvalidMethodCall,
            "associated functions must be called",
        ),
        ("struct Counter { value: i64 } impl Counter { fn value(&self) -> i64 { self.value } } Counter::value()", Category::InvalidMethodCall, "receiver methods must be called"),
    ];

    for (source, category, message) in cases {
        assert_resolution(&Engine::new(), source, category, message);
    }
}

#[test]
fn impl_owners_duplicate_methods_and_self_scope_are_compile_checked() {
    assert_resolution(&Engine::new(), "impl i64 { fn value(&self) -> i64 { 1 } }", Category::InvalidImplOwner, "Cannot define inherent methods for 'i64'");

    let duplicate = resolution_error(
        &Engine::new(),
        "struct Counter { value: i64 } impl Counter { fn value(&self) -> i64 { self.value } } impl Counter { fn value(&self) -> i64 { self.value } }",
        Category::DuplicateMethod,
        "Counter::value",
    );
    assert!(duplicate.secondary_span().is_some());

    assert_resolution(&Engine::new(), "struct Counter { value: i64 } self", Category::SelfOutsideImpl, "only available inside an impl method");
}

#[test]
fn shared_receivers_and_immutable_bindings_cannot_mutate() {
    let shared = resolution_error(&Engine::new(), "struct Counter { value: i64 } impl Counter { fn break_it(&self) { self.value = 1; } }", Category::ImmutableReceiver, "shared receiver '&self'");
    assert!(shared.secondary_span().is_some());

    assert_resolution(
        &Engine::new(),
        "struct Counter { value: i64 } impl Counter { fn increment(&mut self) { self.value += 1; } } let counter = Counter { value: 0 }; counter.increment();",
        Category::ImmutableBinding,
        "immutable binding 'counter'",
    );

    let shared_field = resolution_error(
        &Engine::new(),
        "struct Child { value: i64 } struct Parent { child: Child } impl Child { fn increment(&mut self) { self.value += 1; } } impl Parent { fn reject(&self) { self.child.increment(); } }",
        Category::ImmutableReceiver,
        "shared receiver '&self'",
    );
    assert!(shared_field.secondary_span().is_some());

    assert_resolution(
        &Engine::new(),
        "struct Child { value: i64 } struct Parent { child: Child } impl Child { fn increment(&mut self) { self.value += 1; } } let parent = Parent { child: Child { value: 0 } }; parent.child.increment();",
        Category::ImmutableBinding,
        "immutable binding 'parent'",
    );
}

#[test]
fn method_return_types_are_compile_checked() {
    assert_resolution(&Engine::new(), "struct Counter { value: i64 } impl Counter { fn value(&self) -> bool { self.value } }", Category::TypeMismatch, "expected 'bool', found 'i64'");

    assert_resolution(&Engine::new(), "struct Counter { value: i64 } impl Counter { fn value(&self) -> i64 { self.value; } }", Category::TypeMismatch, "expected 'i64', found '()'");
}

#[test]
fn nominal_method_if_branches_return_one_exact_type() {
    let value = Engine::new()
        .eval::<i64>(
            r#"
                struct Counter { value: i64 }
                impl Counter {
                    fn selected(&self, active: bool) -> i64 {
                        if active { self.value } else { 0 }
                    }
                }
                let counter = Counter { value: 42 };
                counter.selected(true) + counter.selected(false)
            "#,
        )
        .unwrap();

    assert_eq!(value, 42);
}

#[test]
fn nested_if_scopes_bindings_and_evaluates_only_the_selected_branch() {
    let value = Engine::new()
        .eval::<i64>(
            r#"
                struct Probe { calls: i64 }
                impl Probe {
                    fn condition(&mut self, selected: bool) -> bool {
                        self.calls += 1;
                        selected
                    }
                    fn selected(&mut self) -> i64 { self.calls += 1; 40 }
                    fn unselected(&mut self) -> i64 { panic!("unselected branch ran") }
                    fn choose(&mut self) -> i64 {
                        if self.condition(false) {
                            self.unselected()
                        } else if self.condition(true) {
                            let local = self.selected();
                            local
                        } else {
                            self.unselected()
                        }
                    }
                }
                let mut probe = Probe { calls: 0 };
                probe.choose() + probe.calls
            "#,
        )
        .unwrap();

    assert_eq!(value, 43);

    assert_resolution(&Engine::new(), "pub fn hidden(flag: bool) -> i64 { let result = if flag { let local = 1; local } else { 0 }; local }", Category::UnknownVariable, "Unknown variable 'local'");
}

#[test]
fn if_unit_branches_and_divergence_boundary_are_explicit() {
    Engine::new().compile("pub fn unit(flag: bool) { if flag { () } else { () } }").unwrap();

    // Branches that panic diverge, so they fit any type, as in Rust.
    let engine = Engine::new();
    engine.compile(r#"pub fn diverges(flag: bool) -> i64 { if flag { panic!("branch") } else { panic!("branch") } }"#).unwrap();
    assert_eq!(engine.eval::<i64>(r#"fn choose(flag: bool) -> i64 { if flag { panic!("branch") } else { 42 } } choose(false)"#).unwrap(), 42);

    // `throw` is not Rust.
    assert_resolution(&engine, "pub fn rejected(flag: bool) -> i64 { if flag { throw \"branch\" } else { 0 } }", Category::Syntax, "found `throw`");
}

#[test]
fn if_block_tail_mismatch_points_to_the_endpoint() {
    let source = "pub fn choose(flag: bool) -> i64 { if flag { let x = 1; x } else { false } }";
    // As rustc, the `else` branch is held to the `if` branch's type.
    let error = resolution_error(&Engine::new(), source, Category::TypeMismatch, "expected '{integer}', found 'bool'");
    let false_column = source.find("false").unwrap() + 1;
    assert_eq!(error.primary_span().start(), Position::new(1, false_column as u16));
}

#[test]
fn compiler_generated_method_names_cannot_be_called_from_source() {
    assert_resolution(&Engine::new(), "struct Counter { value: i64 } __rawr_nominal_method_0()", Category::UnsupportedNominalSyntax, "compiler-private nominal intrinsic");
}

#[test]
fn successful_ast_uses_stable_unspellable_method_symbols() {
    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                struct Counter { value: i64 }
                impl Counter {
                    fn value(&self) -> i64 { self.value }
                }
                Counter { value: 42 }.value()
            "#,
        )
        .unwrap();
    let names: Vec<_> = ast.iter_functions().map(|function| function.name.to_string()).collect();
    let debug = format!("{ast:#?}");

    assert!(names.iter().all(|name| !name.contains("__rawr_nominal_")), "{names:?}");
    assert!(names.iter().any(|name| name.starts_with('\0') && name.contains("main::Counter::value")), "{names:?}");
    assert!(!debug.contains("__rawr_nominal_"), "{debug}");
    assert!(debug.contains("main::Counter::value"), "{debug}");
    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
}

#[test]
fn compiler_support_survives_function_only_ast_clones() {
    let engine = Engine::new();
    let ast = engine
        .compile(
            r#"
                struct Counter { value: i64 }
                pub fn answer() -> i64 { Counter { value: 42 }.value }
            "#,
        )
        .unwrap()
        .clone_functions_only();

    assert!(ast.has_functions());
    assert_eq!(engine.call_fn::<i64>(&mut Scope::new(), &ast, "answer", ()).unwrap(), 42);
}

#[test]
fn method_entry_rejects_stale_nested_receivers() {
    let engine = Engine::new();
    let stale = engine
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
                impl Outer {
                    fn read(&self) -> bool { self.inner.value }
                }
                pub fn invoke(outer: Outer) -> bool { outer.read() }
            "#,
        )
        .unwrap();

    let error = engine.call_fn::<bool>(&mut Scope::new(), &ast, "invoke", (stale,)).unwrap_err();

    assert!(error.to_string().contains("main::Inner has a stale schema"));
}

#[test]
fn field_mutation_preserves_the_dynamic_envelope() {
    let mut engine = Engine::new();
    engine.register_fn("tag", |value: &mut Dynamic| {
        value.set_tag(42);
    });
    let result = engine
        .eval::<Dynamic>(
            r#"
                struct Counter { value: i64 }
                let mut counter = Counter { value: 0 };
                tag(counter);
                counter.value = 42;
                counter
            "#,
        )
        .unwrap();

    assert_eq!(result.tag(), 42);
}

#[test]
fn evolving_nominal_counter_example_executes_end_to_end() {
    let engine = Engine::new();
    let ast = engine.compile(include_str!("../../examples/scripts/nominal_counter.rs")).unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
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
    // The structured message: Display prints the rustc-style report.
    assert!(error.err_type().to_string().contains(message), "diagnostic `{error}` did not contain `{message}`");
    assert!(!error.primary_span().is_none());
    error
}
