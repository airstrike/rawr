#![cfg(not(feature = "no_function"))]

use rawr::program::{Aspect, Error};
use rawr::{Dynamic, Engine, Shared, item};

fn symbolic(authority: &rawr::program::Authority, name: &str, kind: item::Kind) -> item::Value {
    authority
        .items()
        .find(|value| value.key().name() == name && value.kind() == kind)
        .unwrap_or_else(|| panic!("missing symbolic {kind:?} item `{name}`"))
        .clone()
}

#[test]
fn executable_exposes_stable_function_method_and_variant_values() {
    let engine = Shared::new(Engine::new());
    let authority = engine
        .compile_authority(
            r#"
                pub fn free(value: i64) -> i64 { value }

                struct Counter { value: i64 }
                enum Message { Tick, Add(i64) }

                impl Counter {
                    fn associated(value: i64) -> i64 { value }
                    fn read(&self) -> i64 { self.value }
                }
            "#,
        )
        .unwrap();

    let free = symbolic(&authority, "free", item::Kind::Function);
    assert_eq!(free.key().to_string(), "main::free");
    assert!(free.owner().is_none());
    assert!(free.receiver().is_none());

    let associated = symbolic(&authority, "associated", item::Kind::AssociatedFunction);
    assert_eq!(associated.key().to_string(), "main::Counter::associated");
    assert!(associated.owner().is_some());
    assert!(associated.receiver().is_none());

    let method = symbolic(&authority, "read", item::Kind::Method);
    assert_eq!(method.key().to_string(), "main::Counter::read");
    assert_eq!(method.receiver(), Some(rawr::receiver::Mode::Shared));

    let tick = symbolic(&authority, "Tick", item::Kind::VariantConstructor);
    let add = symbolic(&authority, "Add", item::Kind::VariantConstructor);
    assert_eq!(tick.key().to_string(), "main::Message::Tick");
    assert_eq!(add.key().to_string(), "main::Message::Add");
    assert_eq!(tick.owner(), add.owner());
    assert_ne!(tick.signature(), add.signature());

    let persistent = Dynamic::from(method.clone());
    assert!(persistent.is::<item::Value>());
    assert_eq!(persistent.cast::<item::Value>(), method);

    let debug = format!("{free:?}{associated:?}{tick:?}");
    assert!(!debug.contains("Program"));
    assert!(!debug.contains("AST"));
    assert!(!debug.contains("Authority"));
}

#[test]
fn retained_function_rebinds_and_invokes_equivalent_new_executable() {
    let engine = Shared::new(Engine::new());
    let source = "pub fn add_one(value: i64) -> i64 { value + 1 }";
    let old = engine.compile_authority(source).unwrap();
    let retained = symbolic(&old, "add_one", item::Kind::Function);
    let executable = engine.compile_executable(source).unwrap();
    let current = engine.authorize(executable).unwrap();

    assert_ne!(old.generation(), current.generation());
    assert_eq!(current.rebind(&retained).unwrap(), retained);

    let mut args = [Dynamic::from(41_i64)];
    assert_eq!(current.invoke(&retained, &mut args).unwrap().as_int().unwrap(), 42);
}

#[test]
fn terminal_throw_satisfies_declared_output_and_preserves_runtime_failure() {
    let engine = Shared::new(Engine::new());
    let authority = engine.compile_authority(r#"pub fn impossible() -> i64 { throw "boom"; }"#).unwrap();
    let impossible = symbolic(&authority, "impossible", item::Kind::Function);

    let error = authority.invoke(&impossible, &mut []).unwrap_err();
    assert!(matches!(error, Error::Evaluation { .. }));
    assert!(error.to_string().contains("boom"));

    let error = engine.compile_authority(r#"pub fn impossible() -> i64 { throw "boom"; 42 }"#).unwrap_err();
    assert!(error.to_string().contains("non-terminal `throw`"));
}

#[test]
fn authority_invokes_associated_and_receiver_methods_through_paired_lowering() {
    let engine = Shared::new(Engine::new());
    let authority = engine
        .compile_authority(
            r#"
                struct Counter { value: i64 }
                impl Counter {
                    fn double(value: i64) -> i64 { value + value }
                    fn increment(&mut self, amount: i64) -> i64 {
                        self.value += amount;
                        self.value
                    }
                    fn value(&self) -> i64 { self.value }
                }
            "#,
        )
        .unwrap();
    let associated = symbolic(&authority, "double", item::Kind::AssociatedFunction);
    let method = symbolic(&authority, "increment", item::Kind::Method);
    let read = symbolic(&authority, "value", item::Kind::Method);

    let mut associated_args = [Dynamic::from(21_i64)];
    assert_eq!(authority.invoke(&associated, &mut associated_args).unwrap().as_int().unwrap(), 42);

    let receiver: Dynamic = engine.eval("struct Counter { value: i64 } Counter { value: 41 }").unwrap();
    let mut method_args = [receiver, Dynamic::from(1_i64)];
    assert_eq!(authority.invoke(&method, &mut method_args).unwrap().as_int().unwrap(), 42);
    assert_eq!(authority.invoke(&read, &mut method_args[..1]).unwrap().as_int().unwrap(), 42);
}

#[test]
fn changed_signature_receiver_capabilities_kind_and_owner_are_rejected() {
    let engine = Shared::new(Engine::new());

    let old = engine.compile_authority("pub fn answer() -> i64 { 42 }").unwrap();
    let retained = symbolic(&old, "answer", item::Kind::Function);
    let changed = engine.compile_authority("pub fn answer(value: i64) -> i64 { value }").unwrap();
    assert!(matches!(changed.rebind(&retained), Err(Error::Mismatch { aspect: Aspect::Signature, .. })));

    let shared = engine.compile_authority("struct Counter { value: i64 } impl Counter { fn read(&self) -> i64 { self.value } }").unwrap();
    let retained = symbolic(&shared, "read", item::Kind::Method);
    let mutable = engine
        .compile_authority("struct Counter { value: i64 } impl Counter { fn read(&mut self) -> i64 { self.value } }")
        .unwrap();
    assert!(matches!(mutable.rebind(&retained), Err(Error::Mismatch { aspect: Aspect::Receiver, .. })));

    let clone_only = engine
        .compile_authority("#[derive(Clone)] struct Counter { value: i64 } impl Counter { fn read(&self) -> i64 { self.value } }")
        .unwrap();
    let retained = symbolic(&clone_only, "read", item::Kind::Method);
    let copy = engine
        .compile_authority("#[derive(Clone, Copy)] struct Counter { value: i64 } impl Counter { fn read(&self) -> i64 { self.value } }")
        .unwrap();
    assert!(matches!(copy.rebind(&retained), Err(Error::Mismatch { aspect: Aspect::Capabilities, .. })));

    let associated = engine.compile_authority("struct Counter { value: i64 } impl Counter { fn ping() -> i64 { 1 } }").unwrap();
    let retained = symbolic(&associated, "ping", item::Kind::AssociatedFunction);
    let method = engine.compile_authority("struct Counter { value: i64 } impl Counter { fn ping(&self) -> i64 { 1 } }").unwrap();
    assert!(matches!(method.rebind(&retained), Err(Error::Mismatch { aspect: Aspect::Kind, .. })));

    let owner_a = engine
        .compile_authority("struct A { value: i64 } struct B { value: i64 } impl A { fn ping(&self) -> i64 { self.value } }")
        .unwrap();
    let retained = symbolic(&owner_a, "ping", item::Kind::Method);
    let owner_b = engine
        .compile_authority("struct A { value: i64 } struct B { value: i64 } impl B { fn ping(&self) -> i64 { self.value } }")
        .unwrap();
    assert!(matches!(owner_b.rebind(&retained), Err(Error::Mismatch { aspect: Aspect::Owner, .. })));
}

#[test]
fn authority_rejects_wrong_arity_and_constructs_variants() {
    let engine = Shared::new(Engine::new());
    let authority = engine
        .compile_authority(
            r#"
                pub fn identity(value: i64) -> i64 { value }
                pub fn payload(message: Message) -> i64 {
                    match message { Message::Add(value) => value }
                }
                enum Message { Add(i64) }
            "#,
        )
        .unwrap();
    let function = symbolic(&authority, "identity", item::Kind::Function);
    let variant = symbolic(&authority, "Add", item::Kind::VariantConstructor);
    let payload = symbolic(&authority, "payload", item::Kind::Function);

    assert!(matches!(authority.invoke(&function, &mut []), Err(Error::InvalidArity { expected: 1, actual: 0, .. })));
    assert!(matches!(authority.invoke(&variant, &mut []), Err(Error::InvalidArity { expected: 1, actual: 0, .. })));
    assert!(matches!(
        authority.invoke(&variant, &mut [Dynamic::from(true)]),
        Err(Error::Construction {
            error: rawr::program::construction::Error::Member { source, .. },
            ..
        }) if matches!(*source, rawr::program::construction::Error::ValueType { .. })
    ));
    let message = authority.invoke(&variant, &mut [Dynamic::from(42_i64)]).unwrap();
    assert_eq!(message.type_name(), "Rawr enum");
    assert_eq!(authority.invoke(&payload, &mut [message]).unwrap().as_int().unwrap(), 42);
}

#[test]
fn authority_variant_construction_rejects_stale_nested_values() {
    let engine = Shared::new(Engine::new());
    let stale = engine.eval::<Dynamic>("struct Inner { value: i64 } Inner { value: 42 }").unwrap();
    let authority = engine.compile_authority("struct Inner { value: bool } enum Envelope { Value(Inner) }").unwrap();
    let variant = symbolic(&authority, "Value", item::Kind::VariantConstructor);

    assert!(matches!(
        authority.invoke(&variant, &mut [stale]),
        Err(Error::Construction {
            error: rawr::program::construction::Error::Member { source, .. },
            ..
        }) if matches!(*source, rawr::program::construction::Error::SchemaMismatch { .. })
    ));
}

#[test]
fn bare_ast_remains_evaluable_but_is_not_an_authority() {
    let engine = Engine::new();
    let ast = engine.compile("40 + 2").unwrap();
    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);

    let executable = engine.compile_executable("40 + 2").unwrap();
    assert_eq!(executable.type_count(), 0);
    assert_eq!(executable.item_count(), 0);
    assert_eq!(format!("{executable:?}"), "Executable { types: 0, items: 0, lowering: \"<private>\" }");
}

#[test]
fn executable_issues_the_generation_adopted_by_authority() {
    let engine = Shared::new(Engine::new());
    let first = engine.compile_executable("40 + 2").unwrap();
    let first_generation = first.generation();
    let second = engine.compile_executable("40 + 2").unwrap();
    let second_generation = second.generation();

    assert_ne!(first_generation, second_generation);

    let first_authority = engine.authorize(first).unwrap();
    let second_authority = engine.authorize(second).unwrap();
    assert_eq!(first_authority.generation(), first_generation);
    assert_eq!(second_authority.generation(), second_generation);
}

#[test]
fn authority_requires_the_exact_engine_that_compiled_the_executable() {
    let compiler = Shared::new(Engine::new());
    let other = Shared::new(Engine::new());
    let executable = compiler.compile_executable("pub fn answer() -> i64 { 42 }").unwrap();

    assert!(matches!(other.authorize(executable), Err(Error::EngineOriginMismatch)));
}

#[test]
fn raw_engines_issue_distinct_origins_lazily() {
    let compiler = Shared::new(Engine::RAW);
    let other = Shared::new(Engine::new_raw());
    let executable = compiler.compile_executable("40 + 2").unwrap();

    assert!(matches!(other.authorize(executable), Err(Error::EngineOriginMismatch)));

    let executable = compiler.compile_executable("40 + 2").unwrap();
    assert!(compiler.authorize(executable).is_ok());
}
