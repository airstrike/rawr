//! Same-pass executable lowering and imported-use parity checks.

use super::{ImportedOutput, ImportedUse};
use crate::host::{facade, item, signature, type_ref};
use crate::{host, resolve, Dynamic, Engine, Shared};
use rawr_import::classify::representability::convert;
use rawr_import::surface::identity;

const CALLS: &str = r#"
use native::{apply, identity, increment};
pub fn run() -> i64 { apply(identity(41), increment) }
"#;

fn selector(id: u8) -> identity::Selector {
    identity::Selector::Function {
        item: identity::Definition::authenticated([id; 32]),
    }
}

fn call_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("native", facade::Abi::new(1).unwrap()).unwrap();
    let integer = type_ref::Type::builtin(type_ref::Builtin::I64);
    let mut increment = signature::Builder::new();
    increment.parameter(integer.clone()).output(integer.clone());
    builder
        .item(
            item::Definition::function("increment", increment.finish())
                .unwrap()
                .imported(selector(1)),
        )
        .unwrap();
    let mut identity = signature::Builder::new();
    identity.type_parameter("T", &[]).unwrap();
    identity
        .parameter(type_ref::Type::parameter("T").unwrap())
        .output(type_ref::Type::parameter("T").unwrap());
    builder
        .item(
            item::Definition::function("identity", identity.finish())
                .unwrap()
                .imported(selector(2)),
        )
        .unwrap();
    let mut apply = signature::Builder::new();
    apply
        .parameter(integer.clone())
        .parameter(type_ref::Type::imported_function_item(
            integer.clone(),
            integer.clone(),
        ))
        .output(integer);
    builder
        .item(
            item::Definition::function("apply", apply.finish())
                .unwrap()
                .imported(selector(3)),
        )
        .unwrap();
    let mut make = signature::Builder::new();
    make.type_parameter("T", &[]).unwrap();
    make.output(type_ref::Type::parameter("T").unwrap());
    builder
        .item(
            item::Definition::function("make", make.finish())
                .unwrap()
                .imported(selector(4)),
        )
        .unwrap();
    let mut idle = signature::Builder::new();
    idle.output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder
        .item(
            item::Definition::function("idle", idle.finish())
                .unwrap()
                .imported(selector(5))
                .discardable()
                .elidable(),
        )
        .unwrap();
    builder.finish().unwrap()
}

fn program(engine: &Engine, source: &str) -> resolve::Program {
    resolve::resolve(engine.parse_unit(source).unwrap(), &engine.host_registry).unwrap()
}

fn lower(
    engine: &Engine,
    program: resolve::Program,
    allocation: &mut impl host::ImportAllocation,
) -> Result<(crate::program::Executable, ImportedOutput), resolve::ImportedFailure> {
    resolve::lower_package_with_uses(
        engine,
        program,
        allocation,
        None,
        crate::OptimizationLevel::None,
    )
}

fn parity(engine: &Engine, source: &str) -> (crate::program::Executable, ImportedOutput) {
    let program = program(engine, source);
    let mut symbolic_allocation = host::TestImportAllocation::default();
    let symbolic = super::imported(
        engine,
        &program,
        &engine.host_registry,
        &mut symbolic_allocation,
    )
    .unwrap_or_else(|failure| panic!("symbolic fixture: {}", failure.into_error()));
    let mut actual_allocation = host::TestImportAllocation::default();
    let (executable, actual) = lower(engine, program, &mut actual_allocation)
        .unwrap_or_else(|failure| panic!("executable fixture: {}", failure.into_error()));
    assert_eq!(actual.uses, symbolic.uses);
    assert_eq!(actual.dependencies, symbolic.dependencies);
    assert_eq!(actual_allocation.usage(), symbolic_allocation.usage());
    (executable, actual)
}

fn used<'a>(uses: &'a [ImportedUse], path: &str) -> &'a ImportedUse {
    uses.iter().find(|used| used.path == path).unwrap()
}

#[test]
fn executable_use_collection_retains_concrete_types_and_static_operands() {
    let mut engine = Engine::new();
    engine.register_host_facade(call_facade()).unwrap();
    let engine = Shared::new(engine);
    let (executable, collected) = parity(&engine, CALLS);
    assert_eq!(collected.uses.len(), 2);
    assert_eq!(
        used(&collected.uses, "native::identity").types.as_ref(),
        [convert::Type::Builtin(convert::Builtin::I64)]
    );
    let apply = used(&collected.uses, "native::apply");
    assert_eq!(apply.selector, selector(3));
    assert_eq!(apply.functions.len(), 1);
    assert_eq!(apply.functions[0].position, 1);
    assert_eq!(apply.functions[0].path, "native::increment");
    assert_eq!(apply.functions[0].selector, selector(1));
    assert!(!apply.functions[0].span.is_none());
    assert!(collected.uses.iter().all(|used| !used.span.is_none()));

    let ordinary = engine.compile_executable(CALLS).unwrap();
    assert_eq!(
        executable.runtime_requirements(),
        ordinary.runtime_requirements()
    );
    assert_eq!(executable.host_dependencies(), ordinary.host_dependencies());
    let authority = crate::program::Authority::new(engine, executable).unwrap();
    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    let mut runtime = host::runtime::Builder::new(call_facade());
    runtime
        .bind_imported(selector(2).fingerprint(), |_, args| {
            assert_eq!(args.len(), 1);
            Ok(args[0].take())
        })
        .unwrap();
    // The generated runner binds an item reached by two paths only once.
    assert!(!runtime.accepts_imported(&selector(2).fingerprint()));
    assert!(runtime.accepts_imported(&selector(3).fingerprint()));
    runtime
        .bind_imported(selector(3).fingerprint(), |_, args| {
            assert_eq!(
                args.len(),
                1,
                "static function operands never enter runtime dispatch"
            );
            Ok(Dynamic::from(args[0].as_int().unwrap() + 1))
        })
        .unwrap();
    let mut registry = host::runtime::Registry::new();
    registry.install(runtime.finish()).unwrap();
    let result = authority
        .invoke_bound(&run, &mut [], Shared::new(registry))
        .unwrap();
    assert_eq!(result.as_int().unwrap(), 42);
}

#[test]
fn imported_function_called_by_name_and_full_path_runs() {
    let mut engine = Engine::new();
    engine.register_host_facade(call_facade()).unwrap();
    let engine = Shared::new(engine);
    let (executable, _) = parity(
        &engine,
        "use native::{apply, identity, increment};\npub fn run() -> i64 { apply(native::identity(identity(41)), increment) }\n",
    );
    let authority = crate::program::Authority::new(engine, executable).unwrap();
    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    let mut runtime = host::runtime::Builder::new(call_facade());
    for id in [2, 3] {
        assert!(runtime.accepts_imported(&selector(id).fingerprint()));
        runtime
            .bind_imported(selector(id).fingerprint(), move |_, args| {
                let value = args[0].take();
                Ok(if id == 3 {
                    Dynamic::from(value.as_int().unwrap() + 1)
                } else {
                    value
                })
            })
            .unwrap();
        assert!(!runtime.accepts_imported(&selector(id).fingerprint()));
    }
    let mut registry = host::runtime::Registry::new();
    registry.install(runtime.finish()).unwrap();
    let result = authority
        .invoke_bound(&run, &mut [], Shared::new(registry))
        .unwrap();
    assert_eq!(result.as_int().unwrap(), 42);
}

#[test]
fn executable_use_collection_covers_functions_methods_and_entry_script() {
    let mut engine = Engine::new();
    engine.register_host_facade(call_facade()).unwrap();
    let (executable, collected) = parity(
        &engine,
        r#"
use native::identity;
struct State {}
impl State { fn value(&self) -> i64 { identity(7) } }
pub fn value() -> bool { identity(true) }
identity(42)
"#,
    );
    assert_eq!(collected.uses.len(), 3);
    assert!(collected
        .uses
        .iter()
        .any(|used| used.types.as_ref() == [convert::Type::Builtin(convert::Builtin::Bool)]));
    assert!(executable.item_count() >= 2);
}

#[test]
fn executable_use_collection_preserves_original_cumulative_allocation() {
    let mut engine = Engine::new();
    engine.register_host_facade(call_facade()).unwrap();
    let mut measured = host::TestImportAllocation::default();
    lower(&engine, program(&engine, CALLS), &mut measured)
        .unwrap_or_else(|failure| panic!("measured fixture: {}", failure.into_error()));
    let (items, bytes) = measured.usage();
    let mut clone_only = host::TestImportAllocation::default();
    engine
        .host_registry
        .clone_for_import(&mut clone_only)
        .unwrap();
    assert!(items > clone_only.usage().0);
    assert!(bytes > clone_only.usage().1);
    for mut allocation in [
        host::TestImportAllocation::new(0, usize::MAX),
        host::TestImportAllocation::new(clone_only.usage().0, usize::MAX),
        host::TestImportAllocation::new(items - 1, usize::MAX),
        host::TestImportAllocation::new(usize::MAX, bytes - 1),
    ] {
        assert!(matches!(
            lower(&engine, program(&engine, CALLS), &mut allocation),
            Err(resolve::ImportedFailure::Surface(
                rawr_import::surface::Error::Allocation
            ))
        ));
    }
    let mut original = host::TestImportAllocation::new(items, bytes);
    host::ImportAllocation::retain_checked(&mut original, Some(1), Some(1)).unwrap();
    assert!(matches!(
        lower(&engine, program(&engine, CALLS), &mut original),
        Err(resolve::ImportedFailure::Surface(
            rawr_import::surface::Error::Allocation
        ))
    ));
    assert!(
        original.usage().0 > 1,
        "the supplied ledger retains the work already charged"
    );
}

#[test]
fn executable_use_collection_preserves_positioned_lowering_diagnostics() {
    let mut engine = Engine::new();
    engine.register_host_facade(call_facade()).unwrap();
    let source = "use native::identity;\npub fn run() -> i64 { identity(true) }";
    let resolved = program(&engine, source);
    let original_source = resolved.source().entry();
    let failure = match lower(
        &engine,
        resolved,
        &mut host::TestImportAllocation::default(),
    ) {
        Err(resolve::ImportedFailure::Lowering(failure)) => failure,
        _ => panic!("a type error must remain a lowering diagnostic"),
    };
    let (evidence, error) = failure.into_parts();
    assert!(
        matches!(evidence, resolve::LoweringEvidence::Original(source) if source == original_source)
    );
    assert!(!error.position().is_none());
    let ordinary = engine.compile_executable(source).unwrap_err();
    assert_eq!(error.to_string(), ordinary.to_string());
    assert_eq!(error.position(), ordinary.position());
}

#[test]
fn executable_use_collection_retains_profile_event_mapper_seals() {
    let mut builder = facade::Builder::new("events", facade::Abi::new(1).unwrap()).unwrap();
    let identity = host::type_def::import::Identity::new([9; 32]);
    builder
        .imported_type(
            "Signal",
            identity,
            &[],
            Box::from(["Message".into()]),
            Box::default(),
            Shared::new(host::type_def::Shape::Opaque),
            &mut host::TestImportAllocation::default(),
        )
        .unwrap();
    let mut on = signature::Builder::new();
    on.type_parameter("Message", &[]).unwrap();
    on.parameter(type_ref::Type::imported_function_item(
        type_ref::Type::builtin(type_ref::Builtin::Bool),
        type_ref::Type::parameter("Message").unwrap(),
    ))
    .output(type_ref::Type::imported(
        identity,
        [type_ref::Type::parameter("Message").unwrap()],
    ));
    builder
        .item(
            item::Definition::associated_function("Signal::on", on.finish())
                .unwrap()
                .imported(identity::Selector::AssociatedFunction {
                    item: identity::Definition::authenticated([10; 32]),
                }),
        )
        .unwrap();
    let mut engine = Engine::new();
    engine
        .register_host_facade(builder.finish().unwrap())
        .unwrap();
    let (executable, collected) = parity(
        &engine,
        r#"
use events::Signal;
enum Message { Changed(bool) }
pub fn mapper(value: bool) -> Message { Message::Changed(value) }
pub fn run() { let signal = Signal::on(mapper); }
"#,
    );
    let on = used(&collected.uses, "events::Signal::on");
    let [mapper] = on.mappers.as_ref() else {
        panic!("one event mapper is retained");
    };
    assert_eq!(mapper.position, 0);
    assert_ne!(mapper.seal, [0; 16]);
    assert!(!mapper.span.is_none());
    assert!(
        matches!(on.types.as_ref(), [convert::Type::ProfileMessage { mapper: seal, payload }] if seal == &mapper.seal && payload.as_ref() == &convert::Type::Builtin(convert::Builtin::Bool))
    );
    assert_eq!(executable.runtime_requirements().len(), 1);
}

#[test]
fn executable_use_collection_requires_and_retains_exact_structural_adapters() {
    let constructor = identity::Selector::StructConstructor(identity::Form::Tuple { arity: 1 });
    let field: identity::Selector =
        serde_json::from_str(r#"{"StructField":{"index":0,"name":"0"}}"#).unwrap();
    let mut builder = facade::Builder::new("structure", facade::Abi::new(1).unwrap()).unwrap();
    builder
        .imported_type(
            "Cell",
            host::type_def::import::Identity::new([11; 32]),
            &[],
            Box::default(),
            Box::default(),
            Shared::new(host::type_def::Shape::Struct {
                form: host::type_def::Form::Tuple,
                constructor: Some(constructor.clone()),
                fields: Box::from([host::type_def::Field::new(
                    "0".into(),
                    type_ref::Type::builtin(type_ref::Builtin::I64),
                    field.clone(),
                )]),
                constructible: true,
            }),
            &mut host::TestImportAllocation::default(),
        )
        .unwrap();
    let mut engine = Engine::new();
    engine
        .register_host_facade(builder.finish().unwrap())
        .unwrap();
    let source = "use structure::Cell; pub fn run() -> i64 { let cell = Cell(41); cell.0 }";
    let failure = match lower(
        &engine,
        program(&engine, source),
        &mut host::TestImportAllocation::default(),
    ) {
        Err(resolve::ImportedFailure::Lowering(failure)) => failure.into_error(),
        _ => panic!("collection cannot replace structural adapter admission"),
    };
    assert!(failure
        .to_string()
        .contains("no authenticated generated structural adapter"));
    for (selector, kind) in [
        (&constructor, host::structural::Kind::Constructor),
        (&field, host::structural::Kind::Field { copy: true }),
    ] {
        let definition = host::structural::Definition::new(
            engine
                .host_registry
                .type_owner("structure::Cell")
                .unwrap()
                .clone(),
            "structure::Cell".into(),
            selector.fingerprint(),
            [12; 32],
            [13; 32],
            1,
            [14; 32],
            kind,
        );
        engine
            .host_registry
            .admit_structural(definition, &mut host::TestImportAllocation::default())
            .unwrap();
    }
    let (executable, collected) = parity(&engine, source);
    assert!(collected
        .uses
        .iter()
        .any(|used| used.selector == constructor));
    assert!(collected.uses.iter().any(|used| used.selector == field));
    assert_eq!(executable.runtime_requirements().len(), 2);
}

#[test]
fn executable_endpoint_selection_collects_the_same_site_as_symbolic_selection() {
    let engine = Engine::new();
    let program = program(&engine, "");
    let mut builder = facade::Builder::new("native", facade::Abi::new(1).unwrap()).unwrap();
    builder
        .type_::<crate::ImmutableString>("Target", &[], &[])
        .unwrap();
    let facade = builder.finish().unwrap();
    let operation =
        item::Definition::operation("native::endpoint", signature::Builder::new().finish())
            .unwrap();
    let owner = facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
    let endpoint = host::endpoint::Contract::new(
        operation.path_ref().clone(),
        facade.types().next().unwrap().path_ref().clone(),
        [21; 32],
        [22; 32],
        [23; 32],
        [24; 32],
        [25; 32],
        std::any::TypeId::of::<crate::ImmutableString>(),
        Dynamic::from,
    )
    .unwrap();
    let mut outputs = Vec::new();
    for symbolic in [true, false] {
        let runtime =
            crate::program::model::Model::new(crate::program::GenerationId::issue(), &program);
        let models = super::Models::new(&program, runtime, engine.host_registry.clone());
        let mut allocation = host::TestImportAllocation::default();
        let mut failure = None;
        let mut uses = Vec::new();
        let mut dependencies = std::collections::BTreeSet::new();
        let mut parser = super::Parser::new_with_offset(
            &engine,
            "(\"hello\")",
            models,
            0,
            0,
            Some(super::ImportedContext {
                native: None,
                allocation: &mut allocation,
                failure: &mut failure,
                source: program.source().entry(),
                uses: &mut uses,
                dependencies: &mut dependencies,
            }),
        );
        parser.symbolic_only = symbolic;
        parser
            .parse_operation_endpoint(
                endpoint.clone(),
                owner.clone(),
                crate::Span::new(crate::Position::new(1, 1), crate::Position::new(1, 4)),
            )
            .unwrap();
        let operations = parser.finish_ops().unwrap();
        assert_eq!(operations.len(), usize::from(!symbolic));
        drop(parser);
        assert!(failure.is_none());
        outputs.push((uses, dependencies));
    }
    assert_eq!(outputs[0], outputs[1]);
    let [used] = outputs[1].0.as_slice() else {
        panic!("one endpoint use is retained");
    };
    assert_eq!(used.path, "native::endpoint");
    assert_eq!(used.endpoint, Some([21; 32]));
    assert_eq!(used.selector, identity::Selector::Unavailable);
}

#[test]
fn executable_use_collection_waits_for_return_and_outer_call_inference() {
    let mut engine = Engine::new();
    engine.register_host_facade(call_facade()).unwrap();
    for source in [
        "pub fn run() -> i64 { native::make() }",
        "pub fn run() -> i64 { native::increment(native::make()) }",
        "use native::{increment, make}; increment(make())",
    ] {
        let ordinary = engine.compile_executable(source).unwrap();
        let (executable, collected) = parity(&engine, source);
        assert_eq!(
            used(&collected.uses, "native::make").types.as_ref(),
            [convert::Type::Builtin(convert::Builtin::I64)]
        );
        assert_eq!(
            executable.runtime_requirements(),
            ordinary.runtime_requirements()
        );
    }
}

#[test]
fn executable_use_collection_rejects_imported_entry_errors_before_legacy_fallback() {
    let mut engine = Engine::new();
    engine.register_host_facade(call_facade()).unwrap();
    for (source, expected) in [
        (
            "use native::increment; increment(true)",
            resolve::Category::TypeMismatch,
        ),
        ("native::increment(true)", resolve::Category::TypeMismatch),
        (
            "use native::make; make()",
            resolve::Category::InvalidHostOperation,
        ),
    ] {
        let resolved = program(&engine, source);
        let original_source = resolved.source().entry();
        let failure = match lower(
            &engine,
            resolved,
            &mut host::TestImportAllocation::default(),
        ) {
            Err(resolve::ImportedFailure::Lowering(failure)) => failure,
            _ => {
                panic!("an invalid imported entry cannot become unchecked legacy output: {source}")
            }
        };
        let (evidence, error) = failure.into_parts();
        assert!(
            matches!(evidence, resolve::LoweringEvidence::Original(source) if source == original_source)
        );
        let crate::ParseErrorType::Resolution(resolution) = error.err_type() else {
            panic!("expected a typed imported diagnostic for {source}: {error:?}");
        };
        assert_eq!(resolution.category(), expected, "{source}: {error:?}");
        assert!(
            !error.position().is_none(),
            "imported diagnostic needs its source position: {source}: {error:?}"
        );
    }
}

#[test]
fn collector_capacity_failures_remain_surface_allocation_errors() {
    let engine = Engine::new();
    let program = program(&engine, "");
    for kind in 0..3 {
        let runtime =
            crate::program::model::Model::new(crate::program::GenerationId::issue(), &program);
        let models = super::Models::new(&program, runtime, engine.host_registry.clone());
        let mut allocation = host::TestImportAllocation::default();
        let mut failure = None;
        let mut uses = Vec::new();
        let mut dependencies = std::collections::BTreeSet::new();
        let mut parser = super::Parser::new_with_offset(
            &engine,
            "",
            models,
            0,
            0,
            Some(super::ImportedContext {
                native: None,
                allocation: &mut allocation,
                failure: &mut failure,
                source: program.source().entry(),
                uses: &mut uses,
                dependencies: &mut dependencies,
            }),
        );
        let result = match kind {
            0 => parser
                .imported_entries::<super::ImportedMapperUse>(usize::MAX)
                .map(|_| ()),
            1 => parser
                .imported_entries::<super::ImportedFunctionUse>(usize::MAX)
                .map(|_| ()),
            _ => parser
                .imported_entries::<convert::Type>(usize::MAX)
                .map(|_| ()),
        };
        drop(parser);
        assert!(matches!(
            super::imported_result(result, &mut failure, program.source().entry()),
            Err(resolve::ImportedFailure::Surface(
                rawr_import::surface::Error::Allocation
            ))
        ));
    }
}

fn with_import_context<T>(
    allocation: &mut impl host::ImportAllocation,
    action: impl FnOnce(&mut super::ImportedContext<'_>) -> T,
) -> (T, Option<rawr_import::surface::Error>) {
    let mut failure = None;
    let mut uses = Vec::new();
    let mut dependencies = std::collections::BTreeSet::new();
    let output = action(&mut super::ImportedContext {
        native: None,
        allocation,
        failure: &mut failure,
        source: crate::program::source::Id::from_index(0),
        uses: &mut uses,
        dependencies: &mut dependencies,
    });
    (output, failure)
}

#[test]
fn native_evidence_buffers_require_budget_before_reserving_selected_entries() {
    fn check<T>() {
        let bytes = std::mem::size_of::<T>();
        let mut empty = host::TestImportAllocation::new(0, 0);
        let (entries, failure) = with_import_context(&mut empty, |context| context.entries::<T>(0));
        assert_eq!(entries.unwrap().capacity(), 0);
        assert!(failure.is_none());
        assert_eq!(empty.usage(), (0, 0));

        for mut short in [
            host::TestImportAllocation::new(0, bytes),
            host::TestImportAllocation::new(1, bytes - 1),
        ] {
            let (entries, failure) =
                with_import_context(&mut short, |context| context.entries::<T>(1));
            assert!(
                entries.is_err(),
                "unaccepted evidence must not reserve a buffer"
            );
            assert!(matches!(
                failure,
                Some(rawr_import::surface::Error::Allocation)
            ));
            assert_eq!(short.usage(), (0, 0));
        }
        let mut exact = host::TestImportAllocation::new(1, bytes);
        let (entries, failure) = with_import_context(&mut exact, |context| context.entries::<T>(1));
        assert!(entries.is_ok());
        assert!(failure.is_none());
        assert_eq!(exact.usage(), (1, bytes));
    }
    check::<super::ImportedMapperUse>();
    check::<super::ImportedFunctionUse>();
    check::<convert::Type>();
}

/// The models of an empty program, which no script type of these tests
/// names.
fn empty_models() -> super::Models {
    let engine = Engine::new();
    let program = program(&engine, "");
    let runtime =
        crate::program::model::Model::new(crate::program::GenerationId::issue(), &program);
    super::Models::new(&program, runtime, engine.host_registry.clone())
}

#[test]
fn profile_message_evidence_charges_its_box_before_payload_conversion() {
    let mut inference = super::Inference::default();
    let payload = inference.fresh();
    let super::Ty::Infer(id) = payload else {
        unreachable!()
    };
    inference.bind(id, super::Ty::Int(super::Int::I64));
    let message = super::Ty::ProfileMessage {
        payload: Box::new(payload),
        script: Box::new(super::Ty::Unknown),
        seal: [41; 16],
    };
    let bytes = std::mem::size_of::<convert::Type>();
    let mut exact = host::TestImportAllocation::new(2, 2 * bytes);
    let (converted, failure) = with_import_context(&mut exact, |context| {
        let mut types = context.entries(1)?;
        types.push(super::imported_surface_type(
            &inference,
            &empty_models(),
            &message,
            crate::Span::NONE,
            context,
        )?);
        Ok::<_, super::Error>(types.into_boxed_slice())
    });
    assert!(failure.is_none());
    assert!(
        matches!(converted.unwrap().as_ref(), [convert::Type::ProfileMessage { mapper, payload }]
        if *mapper == [41; 16] && **payload == convert::Type::Builtin(convert::Builtin::I64))
    );
    assert_eq!(exact.usage(), (2, 2 * bytes));

    let mut short = host::TestImportAllocation::new(2, 2 * bytes - 1);
    let (converted, failure) = with_import_context(&mut short, |context| {
        let mut types = context.entries(1)?;
        types.push(super::imported_surface_type(
            &inference,
            &empty_models(),
            &message,
            crate::Span::NONE,
            context,
        )?);
        Ok::<_, super::Error>(types)
    });
    assert!(converted.is_err());
    assert!(matches!(
        failure,
        Some(rawr_import::surface::Error::Allocation)
    ));
    assert_eq!(short.usage(), (1, bytes));

    // A deliberately invalid nested type reveals evaluation order: no budget
    // for its enclosing box must refuse before conversion even inspects it.
    let invalid = super::Ty::ProfileMessage {
        payload: Box::new(super::Ty::Unknown),
        script: Box::new(super::Ty::Unknown),
        seal: [41; 16],
    };
    let mut refused = host::TestImportAllocation::new(0, 0);
    let (converted, failure) = with_import_context(&mut refused, |context| {
        super::imported_surface_type(
            &inference,
            &empty_models(),
            &invalid,
            crate::Span::NONE,
            context,
        )
    });
    assert!(matches!(
        converted,
        Err(super::Error::LoweringUnavailable { .. })
    ));
    assert!(matches!(
        failure,
        Some(rawr_import::surface::Error::Allocation)
    ));
    assert_eq!(refused.usage(), (0, 0));
    let mut allowed = host::TestImportAllocation::new(1, bytes);
    let (converted, failure) = with_import_context(&mut allowed, |context| {
        super::imported_surface_type(
            &inference,
            &empty_models(),
            &invalid,
            crate::Span::NONE,
            context,
        )
    });
    assert!(matches!(
        converted,
        Err(super::Error::InvalidHostOperation { .. })
    ));
    assert!(failure.is_none());
}

#[test]
fn executable_use_collection_retains_elided_imported_selections() {
    let mut engine = Engine::new();
    engine.register_host_facade(call_facade()).unwrap();
    let (executable, collected) = parity(&engine, "use native::idle; pub fn run() { idle(); }");
    assert_eq!(collected.uses.len(), 1);
    assert_eq!(collected.uses[0].selector, selector(5));
    assert!(executable.runtime_requirements().is_empty());
}

#[test]
fn symbolic_lowering_records_the_facade_of_every_called_item() {
    let mut extension = facade::Builder::new("extension", facade::Abi::new(1).unwrap()).unwrap();
    let mut make = signature::Builder::new();
    make.output(type_ref::Type::builtin(type_ref::Builtin::I64));
    extension
        .item(item::Definition::function("make", make.finish()).unwrap())
        .unwrap();
    let mut engine = Engine::new();
    engine.register_host_facade(call_facade()).unwrap();
    engine
        .register_host_facade(extension.finish().unwrap())
        .unwrap();
    let source = "use extension::make; pub fn run() -> i64 { make() }";
    let program = program(&engine, source);
    let mut allocation = host::TestImportAllocation::default();
    let symbolic = super::imported(&engine, &program, &engine.host_registry, &mut allocation)
        .unwrap_or_else(|failure| panic!("symbolic fixture: {}", failure.into_error()));
    let ordinary = engine.compile_executable(source).unwrap();
    assert_eq!(symbolic.dependencies.as_ref(), ordinary.host_dependencies());
}

#[test]
fn cloning_an_imported_value_leaves_it_available() {
    let identity = host::type_def::import::Identity::new([0x51; 32]);
    let mut builder = facade::Builder::new("native", facade::Abi::new(1).unwrap()).unwrap();
    builder
        .imported_type(
            "Connection",
            identity,
            &[crate::capability::Kind::Clone],
            Box::default(),
            Box::default(),
            Shared::new(host::type_def::Shape::Opaque),
            &mut host::TestImportAllocation::default(),
        )
        .unwrap();
    let mut connect = signature::Builder::new();
    connect.output(type_ref::Type::imported(identity, []));
    builder
        .item(
            item::Definition::function("connect", connect.finish())
                .unwrap()
                .imported(selector(1)),
        )
        .unwrap();
    let mut engine = Engine::new();
    engine
        .register_host_facade(builder.finish().unwrap())
        .unwrap();
    engine
        .compile_executable(
            "use native::{connect, Connection};
             pub fn run() -> Connection { let c = connect(); let probe = c.clone(); let kept = probe; c }",
        )
        .unwrap();
    let error = engine
        .compile_executable(
            "use native::{connect, Connection};
             pub fn run() -> Connection { let c = connect(); let moved = c; c }",
        )
        .unwrap_err();
    assert!(error.to_string().contains("already moved"), "{error}");
}

/// palette's `Hsl<S = Srgb, T = f32>`, which `make` returns as
/// `Hsl<Srgb, f32>`, and `Pair<A, B>` with no defaults.
fn defaulted_facade() -> facade::Definition {
    let srgb = host::type_def::import::Identity::new([0x61; 32]);
    let hsl = host::type_def::import::Identity::new([0x62; 32]);
    let pair = host::type_def::import::Identity::new([0x63; 32]);
    let mut builder = facade::Builder::new("palette", facade::Abi::new(1).unwrap()).unwrap();
    let mut allocation = host::TestImportAllocation::default();
    for (path, identity, parameters, defaults) in [
        ("Srgb", srgb, Box::default(), Box::default()),
        (
            "Hsl",
            hsl,
            Box::from(["S".into(), "T".into()]),
            Box::from([
                Some(type_ref::Type::imported(srgb, [])),
                Some(type_ref::Type::builtin(type_ref::Builtin::F32)),
            ]),
        ),
        (
            "Pair",
            pair,
            Box::from(["A".into(), "B".into()]),
            Box::default(),
        ),
    ] {
        builder
            .imported_type(
                path,
                identity,
                &[
                    crate::capability::Kind::Copy,
                    crate::capability::Kind::Clone,
                ],
                parameters,
                defaults,
                Shared::new(host::type_def::Shape::Opaque),
                &mut allocation,
            )
            .unwrap();
    }
    let mut make = signature::Builder::new();
    make.output(type_ref::Type::imported(
        hsl,
        [
            type_ref::Type::imported(srgb, []),
            type_ref::Type::builtin(type_ref::Builtin::F32),
        ],
    ));
    builder
        .item(
            item::Definition::function("make", make.finish())
                .unwrap()
                .imported(selector(2)),
        )
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn an_imported_type_written_without_its_defaulted_arguments_takes_them() {
    let mut engine = Engine::new();
    engine.register_host_facade(defaulted_facade()).unwrap();
    engine
        .compile_executable(
            "use palette::{make, Hsl, Srgb};
             struct Picker { color: Hsl }
             struct Partial { color: Hsl<Srgb> }
             pub fn run() -> Hsl {
                 let color: Hsl = make();
                 let partial = Partial { color };
                 let picker = Picker { color: partial.color };
                 picker.color
             }",
        )
        .unwrap();
    let error = engine
        .compile_executable(
            "use palette::{make, Hsl};
             pub fn run() -> Hsl<i64> { make() }",
        )
        .unwrap_err();
    assert!(error.to_string().contains("mismatch"), "{error}");
    let error = engine
        .compile_executable("use palette::Pair; struct Holder { pair: Pair }")
        .unwrap_err();
    assert!(error.to_string().contains("expected 2, found 0"), "{error}");
}

#[test]
fn an_imported_type_compares_and_keys_a_map_by_the_traits_its_crate_implements() {
    use crate::capability::Kind;
    let finger = host::type_def::import::Identity::new([0x71; 32]);
    let instant = host::type_def::import::Identity::new([0x72; 32]);
    let mut builder = facade::Builder::new("touch", facade::Abi::new(1).unwrap()).unwrap();
    let mut allocation = host::TestImportAllocation::default();
    for (path, identity, implemented) in [
        (
            "Finger",
            finger,
            &[Kind::Copy, Kind::PartialEq, Kind::Eq, Kind::Hash][..],
        ),
        ("Instant", instant, &[Kind::Clone][..]),
    ] {
        builder
            .imported_type(
                path,
                identity,
                implemented,
                Box::default(),
                Box::default(),
                Shared::new(host::type_def::Shape::Opaque),
                &mut allocation,
            )
            .unwrap();
    }
    for (name, identity, id) in [("finger", finger, 3), ("instant", instant, 4)] {
        let mut make = signature::Builder::new();
        make.output(type_ref::Type::imported(identity, []));
        builder
            .item(
                item::Definition::function(name, make.finish())
                    .unwrap()
                    .imported(selector(id)),
            )
            .unwrap();
    }
    let mut engine = Engine::new();
    engine
        .register_host_facade(builder.finish().unwrap())
        .unwrap();
    engine
        .compile_executable(
            "use std::collections::HashMap;
             use touch::{finger, Finger};
             pub fn run() -> bool {
                 let mut fingers: HashMap<Finger, i64> = HashMap::new();
                 fingers.insert(finger(), 1);
                 finger() != finger()
             }",
        )
        .unwrap();
    let error = engine
        .compile_executable(
            "use touch::instant;
             pub fn run() -> bool { instant() == instant() }",
        )
        .unwrap_err();
    assert!(error.to_string().contains("PartialEq"), "{error}");
    let error = engine
        .compile_executable(
            "use std::collections::HashMap;
             use touch::{instant, Instant};
             pub fn run() { let mut times: HashMap<Instant, i64> = HashMap::new(); times.insert(instant(), 1); }",
        )
        .unwrap_err();
    assert!(error.to_string().contains("Hash + Eq"), "{error}");
}

#[test]
fn an_imported_type_s_to_string_is_a_function_value_through_its_display() {
    let preset = host::type_def::import::Identity::new([0x73; 32]);
    let mut builder = facade::Builder::new("life", facade::Abi::new(1).unwrap()).unwrap();
    let mut allocation = host::TestImportAllocation::default();
    builder
        .imported_type(
            "Preset",
            preset,
            &[crate::capability::Kind::Copy],
            Box::default(),
            Box::default(),
            Shared::new(host::type_def::Shape::Opaque),
            &mut allocation,
        )
        .unwrap();
    let mut make = signature::Builder::new();
    make.output(type_ref::Type::imported(preset, []));
    builder
        .item(
            item::Definition::function("preset", make.finish())
                .unwrap()
                .imported(selector(5)),
        )
        .unwrap();
    let mut engine = Engine::new();
    engine
        .register_host_facade(builder.finish().unwrap())
        .unwrap();
    // As game_of_life hands it to `pick_list`, and by its full path.
    engine
        .compile_executable(
            "use life::{preset, Preset};
             fn names(f: impl Fn(&Preset) -> String) -> String { f(&preset()) }
             pub fn run() -> Vec<String> {
                 vec![names(Preset::to_string), names(life::Preset::to_string)]
             }",
        )
        .unwrap();
    // Any other member is still the type's own, which it lacks.
    let error = engine
        .compile_executable(
            "use life::Preset;
             pub fn run() { let f = Preset::to_strin; }",
        )
        .unwrap_err();
    assert!(error.to_string().contains("to_strin"), "{error}");
}

/// A trait the program names becomes a module of the imported crate's
/// facade, unless a re-export already names that path: iced's
/// `widget::Component` stays the facade trait it re-exports.
#[test]
fn an_imported_trait_module_leaves_a_re_export_its_path() {
    let mut allocation = host::TestImportAllocation::default();
    let mut builder =
        facade::Builder::new_imported("ui", facade::Abi::new(1).unwrap(), &mut allocation).unwrap();
    builder
        .alias("widget::Component", "widget::component::Component")
        .unwrap();
    let Err(error) = builder.imported_module("widget::Component", &mut allocation) else {
        panic!("a re-export's path is taken");
    };
    assert!(error.to_string().contains("re-export"), "{error}");
    builder
        .imported_module("widget::Other", &mut allocation)
        .unwrap();
}
