//! Terminal lookup classification through the actual package compiler.

use super::tests::{compiler, native_snapshot};
use super::{Error, Output};
use crate::{host, package, resolve, Engine, Shared};
use rawr_import::surface::identity;

fn failure(engine: &Engine, cycle: &host::facade::ProfileCycle, source: &str) -> Error {
    match Output::transform(
        engine,
        &native_snapshot(source),
        cycle,
        &mut host::TestImportAllocation::default(),
    ) {
        Ok(_) => panic!("fixture must fail at its selected lookup: {source}"),
        Err(error) => error,
    }
}

#[test]
fn terminal_declared_native_function_and_type_absence_request_preparation() {
    let (engine, cycle) = compiler();
    for source in [
        "use native::new_function; pub fn sample() -> i64 { new_function() }",
        "pub fn sample() -> i64 { native::new_function() }",
        "pub fn sample(value: native::NewType) {}",
        "struct Holder { value: native::NewType }",
    ] {
        let error = failure(&engine, &cycle, source);
        let Error::NativeMiss(error) = error else {
            panic!("native lookup must request one preparation: {source}: {error}")
        };
        assert!(error
            .report()
            .unwrap()
            .sources()
            .iter()
            .any(|evidence| evidence.text() == source));
    }
}

#[test]
fn terminal_native_miss_preserves_the_original_resolution_diagnostic() {
    let (engine, cycle) = compiler();
    let source = "use native::new_function; pub fn sample() {}";
    let snapshot = native_snapshot(source);
    let graph = package::Graph::load_snapshot(&engine, &snapshot).unwrap();
    let sources = graph.diagnostic_sources();
    let ordinary = match resolve::package::resolve(graph, &engine.host_registry) {
        Ok(_) => panic!("ordinary resolution must still reject the absent import"),
        Err(error) => package::Error::resolution(sources, error),
    };
    let Error::NativeMiss(captured) = failure(&engine, &cycle, source) else {
        panic!("actual absent native import must have provenance")
    };
    assert_eq!(captured.to_string(), ordinary.to_string());
    assert_eq!(
        captured.report().unwrap().message(),
        ordinary.report().unwrap().message()
    );
}

#[test]
fn rawr_typos_shadowed_aliases_wrong_kinds_and_type_errors_remain_source_errors() {
    let (engine, cycle) = compiler();
    for source in [
        "pub fn sample() -> i64 { misspelled() }",
        "pub fn sample(value: MissingType) {}",
        "struct native {} pub fn sample() { native::missing(); }",
        "pub fn native() {} pub fn sample(value: native::MissingType) {}",
        "pub fn native() {} pub fn sample() { native::missing(); }",
        "pub fn sample() { let native = 1; native::missing(); }",
        "pub fn sample(value: native::echo) {}",
        "pub fn sample(value: native::echo::MissingType) {}",
        "use native::echo::missing; pub fn sample() {}",
        "pub fn sample() { native::echo::missing(); }",
        "pub fn sample() -> i64 { native::echo(true) }",
        "pub fn first() -> i64 { native::echo(1) } pub fn second() -> i64 { misspelled() }",
    ] {
        let error = failure(&engine, &cycle, source);
        let Error::Source(error) = error else {
            panic!("ordinary source failure changed classification: {source}: {error}");
        };
        assert_ne!(
            error.category(),
            package::error::Category::Parse,
            "fixture must reach resolution/lowering: {source}: {error}"
        );
    }
    assert!(matches!(
        failure(&engine, &cycle, "pub fn sample( {"),
        Error::Source(_)
    ));
}

fn structure(engine: &mut Engine) -> identity::Selector {
    let constructor = identity::Selector::StructConstructor(identity::Form::Tuple { arity: 1 });
    let field = serde_json::from_str(r#"{"StructField":{"index":0,"name":"0"}}"#).unwrap();
    let mut builder =
        host::facade::Builder::new("structure", host::facade::Abi::new(1).unwrap()).unwrap();
    builder
        .imported_type(
            "Cell",
            host::type_def::import::Identity::new([61; 32]),
            false,
            false,
            Box::default(),
            Shared::new(host::type_def::Shape::Struct {
                form: host::type_def::Form::Tuple,
                constructor: Some(constructor.clone()),
                fields: Box::from([host::type_def::Field::new(
                    "0".into(),
                    host::type_ref::Type::builtin(host::type_ref::Builtin::I64),
                    field,
                )]),
                constructible: true,
            }),
            &mut host::TestImportAllocation::default(),
        )
        .unwrap();
    engine
        .register_host_facade(builder.finish().unwrap())
        .unwrap();
    constructor
}

#[test]
fn terminal_member_absence_requires_a_resolved_imported_type() {
    let (mut engine, cycle) = compiler();
    structure(&mut engine);
    for source in [
        "use structure::Cell; pub fn sample(value: Cell) { value.new_method(); }",
        "use structure::Cell; pub fn sample(value: Cell) -> i64 { value.new_field }",
        "use structure::Cell; pub fn sample() { Cell::new_associated(); }",
        "pub fn sample() { structure::Cell::new_associated(); }",
    ] {
        let error = failure(&engine, &cycle, source);
        assert!(
            matches!(error, Error::NativeMiss(_)),
            "imported member lost provenance: {source}: {error}"
        );
    }
    let source = "struct Cell { value: i64 } pub fn sample(value: Cell) { value.new_method(); }";
    assert!(matches!(failure(&engine, &cycle, source), Error::Source(_)));
}

#[test]
fn absent_structural_adapter_is_a_miss_but_wrong_adapter_kind_is_not() {
    let (mut engine, cycle) = compiler();
    let constructor = structure(&mut engine);
    let source = "use structure::Cell; pub fn sample() -> i64 { let value = Cell(1); value.0 }";
    let error = failure(&engine, &cycle, source);
    assert!(matches!(error, Error::NativeMiss(_)), "{error}");
    let definition = host::structural::Definition::new(
        engine
            .host_registry
            .type_owner("structure::Cell")
            .unwrap()
            .clone(),
        "structure::Cell".into(),
        constructor.fingerprint(),
        [62; 32],
        [63; 32],
        1,
        [64; 32],
        host::structural::Kind::Field { copy: true },
    );
    engine
        .host_registry
        .admit_structural(definition, &mut host::TestImportAllocation::default())
        .unwrap();
    let error = failure(&engine, &cycle, source);
    assert!(matches!(error, Error::Source(_)), "{error}");
    assert!(error.to_string().contains("adapter kind differs"));
}

#[test]
fn capture_and_lowering_allocation_refusals_precede_native_miss_classification() {
    let (engine, cycle) = compiler();
    let snapshot = native_snapshot("pub fn sample() -> i64 { native::new_function() }");
    let graph = package::Graph::load_snapshot(&engine, &snapshot).unwrap();
    let mut probe = host::TestImportAllocation::default();
    let capture = resolve::native::Capture::from_graph(&graph, &mut probe).unwrap();
    drop(capture);
    let (items, bytes) = probe.usage();
    for mut allocation in [
        host::TestImportAllocation::new(0, 0),
        host::TestImportAllocation::new(items, bytes),
    ] {
        let error = match Output::transform(&engine, &snapshot, &cycle, &mut allocation) {
            Ok(_) => panic!("bounded metadata account must refuse this compilation"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Resource(_)), "{error}");
    }
}
