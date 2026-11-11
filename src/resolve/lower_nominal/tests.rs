use super::{
    default_value, inference_metrics, matches_runtime_model, model_clones, reset_inference_metrics,
    reset_model_clones, Inference, Models, Ty,
};
#[cfg(feature = "rust-import")]
use super::{host_ty_proof_allocations, reset_host_ty_proof_allocations, HostTyProof};
use crate::host::{facade, item, signature, type_ref};
use crate::{host, receiver, resolve, Dynamic, Engine, OptimizationLevel, ParseErrorType, Shared};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct RequiredValue;

#[derive(Clone)]
struct GenericValue;

#[derive(Clone)]
struct OrderedValue;

struct ExternalValue;

#[cfg(feature = "rust-import")]
fn symbolic_imported(
    engine: &Engine,
    program: &resolve::Program,
) -> Result<Box<[super::ImportedUse]>, resolve::ImportedFailure> {
    let mut allocation = host::TestImportAllocation::default();
    super::imported(
        engine,
        program,
        &engine.host_registry.share(),
        &mut allocation,
    )
    .map(|output| output.uses)
}

#[test]
fn compiler_ir_layout_keeps_parser_types_compact() {
    #[cfg(not(feature = "rust-import"))]
    assert_eq!(std::mem::size_of::<Ty>(), 40);
    #[cfg(feature = "rust-import")]
    assert_eq!(std::mem::size_of::<Ty>(), 48);
    #[cfg(not(feature = "rust-import"))]
    assert!(
        std::mem::size_of::<super::Op>() < 600,
        "pending operation size: {}",
        std::mem::size_of::<super::Op>()
    );
    #[cfg(feature = "rust-import")]
    assert_eq!(std::mem::size_of::<super::Op>(), 320);
    #[cfg(not(feature = "rust-import"))]
    assert!(
        std::mem::size_of::<super::ExecutableOp>() < 1_112,
        "executable operation size: {}",
        std::mem::size_of::<super::ExecutableOp>()
    );
    #[cfg(feature = "rust-import")]
    assert_eq!(std::mem::size_of::<super::ExecutableOp>(), 624);
}

#[cfg(feature = "rust-import")]
#[test]
fn host_type_clones_share_one_parser_proof_allocation() {
    let facade = imported_alias_facade(false);
    let definition = facade
        .types()
        .find(|definition| definition.path() == "aliases::First")
        .expect("the alias fixture retains First")
        .clone();
    let alias = facade
        .types()
        .find(|definition| definition.path() == "aliases::Second")
        .expect("the alias fixture retains Second")
        .clone();

    reset_host_ty_proof_allocations();
    let ty = Ty::Host {
        proof: HostTyProof::new(definition.clone(), Some(alias.clone()), None),
        arguments: Box::default(),
    };
    let clones = std::iter::repeat_with(|| ty.clone())
        .take(64)
        .collect::<Vec<_>>();
    assert_eq!(clones.len(), 64);
    assert_eq!(host_ty_proof_allocations(), 1);

    let _second_resolution = Ty::Host {
        proof: HostTyProof::new(definition, Some(alias), None),
        arguments: Box::default(),
    };
    assert_eq!(
            host_ty_proof_allocations(),
            2,
            "each independent resolution allocates exactly one proof, no more than the former canonical Definition Rc",
        );
}

#[cfg(feature = "sync")]
#[test]
fn executable_ops_remain_send_and_sync() {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<super::ExecutableOp>();
}

#[cfg(feature = "rust-import")]
#[test]
fn imported_struct_fields_variants_and_methods_remain_symbolic() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(imported_facade())
        .expect("symbolic imported facade installs");
    let program = imported_program(
        &engine,
        r#"
use imported::{Choice, Pair};
let pair = Pair { left: 1, flag: true };
pair.left;
pair.shared();
Choice::Count(2)
"#,
    );
    let uses = symbolic_imported(&engine, &program)
        .unwrap_or_else(|failure| panic!("symbolic imported source: {}", failure.into_error()));
    let paths = uses
        .iter()
        .map(|used| used.path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        [
            "imported::Choice",
            "imported::Pair",
            "imported::Pair",
            "imported::Pair::shared",
        ]
    );
    assert!(uses.iter().all(|used| !used.span.is_none()));
}

#[cfg(feature = "rust-import")]
#[test]
fn imported_struct_update_syntax_reads_the_remaining_fields_from_base() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(imported_facade())
        .expect("symbolic imported facade installs");
    let program = imported_program(
        &engine,
        r#"
use imported::Pair;
let base = Pair { left: 1, flag: true };
let pair = Pair { left: 2, ..base };
pair.left
"#,
    );
    symbolic_imported(&engine, &program)
        .unwrap_or_else(|failure| panic!("symbolic imported source: {}", failure.into_error()));
    let program = imported_program(
        &engine,
        "use imported::Pair;\nlet pair = Pair { left: 2 };\npair.left\n",
    );
    assert!(symbolic_imported(&engine, &program).is_err());
}

#[cfg(feature = "rust-import")]
#[test]
fn imported_structure_stays_non_executable_before_generated_adapter_admission() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(imported_facade())
        .expect("symbolic imported facade installs");
    let error = engine
        .compile(
            r#"
use imported::Pair;
pub fn run() {
    let pair = Pair { left: 1, flag: true };
    pair.left;
}
"#,
        )
        .expect_err("structure is rejected before generated artifact admission");
    assert!(
        matches!(
            error.err_type(),
            ParseErrorType::Resolution(crate::resolve::Error::InvalidHostOperation {
                reason,
                ..
            }) if reason.contains("constructing it is not supported in Rawr yet")
        ),
        "unexpected pre-admission rejection: {error:?}"
    );
}

#[cfg(feature = "rust-import")]
#[test]
fn fully_qualified_nested_imported_type_owner_remains_symbolic() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(imported_facade())
        .expect("symbolic imported facade installs");
    let program = imported_program(&engine, "imported::domain::State::new(20, true)");
    let uses = symbolic_imported(&engine, &program).unwrap_or_else(|failure| {
        panic!(
            "fully qualified nested imported type stays exact: {}",
            failure.into_error()
        )
    });
    assert_eq!(
        uses.iter()
            .map(|used| used.path.as_str())
            .collect::<Vec<_>>(),
        ["imported::domain::State", "imported::domain::State::new"]
    );
    let (executable, collected) = resolve::lower_package_with_uses(
        &engine,
        program,
        &mut host::TestImportAllocation::default(),
        None,
        OptimizationLevel::None,
    )
    .unwrap_or_else(|failure| panic!("qualified owner executable: {}", failure.into_error()));
    assert_eq!(collected.uses, uses);
    assert!(collected.uses.iter().any(|used| {
        used.path == "imported::domain::State"
            && used.selector == rawr_import::surface::identity::Selector::Declaration
    }));
    assert_eq!(executable.runtime_requirements().len(), 1);
}

#[cfg(feature = "rust-import")]
#[test]
fn imported_aliases_preserve_origin_through_recursive_methods_in_both_install_orders() {
    for alias_first in [false, true] {
        for alias in ["First", "Second"] {
            let mut engine = Engine::new();
            engine
                .register_host_facade(imported_alias_facade(alias_first))
                .expect("same-definition aliases install");
            let source = format!(
                "use aliases::{alias}; let value = {alias} {{ value: 1 }}; value.recursive().value"
            );
            let program = imported_program(&engine, &source);
            let uses = symbolic_imported(&engine, &program).unwrap_or_else(|failure| {
                panic!("alias origin stays exact: {}", failure.into_error())
            });
            let paths = uses
                .iter()
                .map(|used| used.path.as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                paths,
                [
                    format!("aliases::{alias}"),
                    format!("aliases::{alias}"),
                    format!("aliases::{alias}::recursive"),
                ]
            );
        }
    }
}

#[cfg(feature = "rust-import")]
#[test]
fn semantic_imported_returns_defer_alias_selection_in_both_install_orders() {
    for alias_first in [false, true] {
        let mut engine = Engine::new();
        engine
            .register_host_facade(imported_alias_facade(alias_first))
            .expect("same-definition aliases install");

        let program = imported_program(&engine, "use aliases::make; let value = make();");
        let uses = symbolic_imported(&engine, &program).unwrap_or_else(|failure| {
            panic!("pathless return stays valid: {}", failure.into_error())
        });
        assert_eq!(
            uses.iter()
                .map(|used| used.path.as_str())
                .collect::<Vec<_>>(),
            ["aliases::make"]
        );

        // A field reads the same through every alias of one type, as a
        // method does, so a pathless return's field resolves (D561).
        let program = imported_program(&engine, "use aliases::make; make().value");
        let uses = symbolic_imported(&engine, &program).unwrap_or_else(|failure| {
            panic!(
                "a pathless return's field resolves: {}",
                failure.into_error()
            )
        });
        assert_eq!(
            uses.iter()
                .map(|used| used.path.as_str())
                .collect::<Vec<_>>(),
            ["aliases::First", "aliases::make"]
        );

        // A member no alias has still names every alias it could be.
        let program = imported_program(&engine, "use aliases::make; make().missing");
        let error = symbolic_imported(&engine, &program)
            .expect_err("a member no alias has selects no exact alias")
            .into_error();
        let ParseErrorType::Resolution(crate::resolve::Error::InvalidHostOperation {
            item,
            reason,
            operation,
        }) = error.err_type()
        else {
            panic!("alias ambiguity is a positioned host operation error: {error}");
        };
        assert_eq!(item, "aliases::First");
        assert!(reason.contains("aliases::First, aliases::Second"));
        assert!(!operation.is_none());
    }
}

#[cfg(feature = "rust-import")]
#[test]
fn local_enum_payload_patterns_bind_imported_values() {
    for pattern in ["pair", "_"] {
        let mut engine = Engine::new();
        engine
            .register_host_facade(imported_facade())
            .expect("symbolic imported facade installs");
        let source = format!(
            r#"
use imported::Pair;
enum Payload {{ Pair(Pair), Empty }}
let payload = Payload::Pair(Pair {{ left: 1, flag: true }});
match payload {{ Payload::Pair({pattern}) => 1, Payload::Empty => 0 }}
"#
        );
        let program = imported_program(&engine, &source);
        symbolic_imported(&engine, &program)
            .unwrap_or_else(|failure| panic!("imported payload pattern: {}", failure.into_error()));
    }
}

/// A crate type exported under several paths, as iced's `Vector` is,
/// prints as the program imports it, else by its shortest path (#508).
#[cfg(feature = "rust-import")]
#[test]
fn a_type_of_several_paths_prints_by_one_of_them() {
    fn mismatch(source: &str) -> (String, String) {
        let mut engine = Engine::new();
        engine
            .register_host_facade(vector_alias_facade())
            .expect("a type under two paths installs");
        let program = imported_program(&engine, source);
        let error = symbolic_imported(&engine, &program)
            .expect_err("an i64 can't hold a Vector")
            .into_error();
        let ParseErrorType::Resolution(crate::resolve::Error::TypeMismatch {
            expected,
            actual,
            ..
        }) = error.err_type()
        else {
            panic!("a type mismatch: {error}");
        };
        (expected.to_string(), actual.to_string())
    }

    // No `use` names the type: its shortest path.
    assert_eq!(
        mismatch("use vectors::make; let value: i64 = make();"),
        ("i64".into(), "vectors::Vector<f32>".into())
    );
    // A `use` of the longer path: that path.
    assert_eq!(
        mismatch("use vectors::Deep::Vector; use vectors::make; let value: i64 = make();"),
        ("i64".into(), "vectors::Deep::Vector<f32>".into())
    );
}

/// A crate type a macro's `$crate` reaches through its crate's hidden
/// segment prints by a public path when the dependency has one, else
/// from its own crate's root, as rustc does: no message names the
/// segment (#563).
#[cfg(feature = "rust-import")]
#[test]
fn a_type_behind_a_hidden_crate_segment_prints_by_a_public_path() {
    fn mismatch(paths: &[&str]) -> (String, String) {
        let mut builder = facade::Builder::new("vectors", facade::Abi::new(1).unwrap()).unwrap();
        let mut allocation = host::TestImportAllocation::default();
        let identity = host::type_def::import::Identity::new([0x57; 32]);
        for module in [HIDDEN, "deep", "deep::er"] {
            builder.imported_module(module, &mut allocation).unwrap();
        }
        for path in paths {
            builder
                .imported_type(
                    path,
                    identity,
                    &[],
                    Box::from(["T".into()]),
                    Box::from([None]),
                    host::type_def::Shape::Opaque.into(),
                    &mut allocation,
                )
                .unwrap();
        }
        let mut make = signature::Builder::new();
        make.output(type_ref::Type::imported(
            identity,
            [type_ref::Type::builtin(type_ref::Builtin::F32)],
        ));
        builder
            .item(
                item::Definition::function("make", make.finish())
                    .unwrap()
                    .imported(rawr_import::surface::identity::Selector::Unavailable),
            )
            .unwrap();
        let mut engine = Engine::new();
        engine
            .register_host_facade(builder.finish().unwrap())
            .expect("the facade installs");
        let source = "use vectors::make; let value: i64 = make();";
        let program = imported_program(&engine, source);
        let error = symbolic_imported(&engine, &program)
            .expect_err("an i64 can't hold a Vector")
            .into_error();
        assert!(!error.to_string().contains("__rawr"), "{error}");
        let ParseErrorType::Resolution(crate::resolve::Error::TypeMismatch { actual, .. }) =
            error.err_type()
        else {
            panic!("a type mismatch: {error}");
        };
        let report = crate::diagnostic::Report::from_parse(
            crate::diagnostic::Source::new("main.rs", source).unwrap(),
            &error,
        )
        .expect("the error reports at its span");
        assert!(!report.message().contains("__rawr"), "{}", report.message());
        (error.to_string(), actual.to_string())
    }
    const HIDDEN: &str = "__rawr_crate_linear_00000000000000ab";

    // A public path, though the hidden one is shorter.
    let (_, actual) = mismatch(&[&format!("{HIDDEN}::Vector"), "deep::er::Vector"]);
    assert_eq!(actual, "vectors::deep::er::Vector<f32>");
    // Only the hidden path: the defining crate's own.
    let (message, actual) = mismatch(&[&format!("{HIDDEN}::Vector")]);
    assert_eq!(actual, "linear::Vector<f32>");
    assert!(message.contains("linear::Vector<f32>"), "{message}");
}

/// `Vector<T>` at `vectors::Deep::Vector` and `vectors::Vector`, and
/// `make() -> Vector<f32>`.
#[cfg(feature = "rust-import")]
fn vector_alias_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("vectors", facade::Abi::new(1).unwrap()).unwrap();
    let mut allocation = host::TestImportAllocation::default();
    let identity = host::type_def::import::Identity::new([0x56; 32]);
    builder.imported_module("Deep", &mut allocation).unwrap();
    for path in ["Deep::Vector", "Vector"] {
        builder
            .imported_type(
                path,
                identity,
                &[],
                Box::from(["T".into()]),
                Box::from([None]),
                host::type_def::Shape::Opaque.into(),
                &mut allocation,
            )
            .unwrap();
    }
    let mut make = signature::Builder::new();
    make.output(type_ref::Type::imported(
        identity,
        [type_ref::Type::builtin(type_ref::Builtin::F32)],
    ));
    builder
        .item(
            item::Definition::function("make", make.finish())
                .unwrap()
                .imported(rawr_import::surface::identity::Selector::Unavailable),
        )
        .unwrap();
    builder.finish().unwrap()
}

#[cfg(feature = "rust-import")]
#[test]
fn imported_structure_and_receiver_failures_keep_rawr_positions() {
    for (source, expected) in [
            (
                "use imported::Pair; let pair = Pair { left: 1, flag: true }; pair.missing",
                "Pair::missing",
            ),
            (
                "use imported::Pair; let pair = Pair { left: 1, flag: true }; pair.mutable()",
                "as mutable",
            ),
            (
                "use imported::Pair; let pair = Pair { left: 1, flag: true }; pair.owned(); pair.shared()",
                "moved value",
            ),
        ] {
            let mut engine = Engine::new();
            engine
                .register_host_facade(imported_facade())
                .expect("symbolic imported facade installs");
            let program = imported_program(&engine, source);
            let failure = symbolic_imported(&engine, &program)
                .expect_err("invalid imported use is rejected");
            let error = failure.into_error();

            assert!(!error.position().is_none(), "{source}");
            assert!(error.to_string().contains(expected), "{error}");
        }
}

#[cfg(feature = "rust-import")]
#[test]
fn imported_ownership_consumes_exactly_non_copy_values() {
    for source in [
            "use imported::{Pair, take_pair}; let pair = Pair { left: 1, flag: true }; take_pair(pair); take_pair(pair)",
            "use imported::Pair; let first = Pair { left: 1, flag: true }; let second = Pair { left: 2, flag: false }; first.combine(second); first.combine(second)",
            "use imported::{Holder, Pair}; let pair = Pair { left: 1, flag: true }; let holder = Holder { pair }; pair.shared()",
            "use imported::{Holder, Pair}; let pair = Pair { left: 1, flag: true }; let holder = Holder { pair }; holder.pair; holder.pair",
            "use imported::Pair; let first = Pair { left: 1, flag: true }; let second = first; first.shared()",
            "use imported::Pair; let mut destination = Pair { left: 1, flag: true }; let source = Pair { left: 2, flag: false }; destination = source; source.shared()",
            "use imported::Pair; let pair = Pair { left: 1, flag: true }; let moved = if true { pair } else { Pair { left: 2, flag: false } }; pair.shared()",
        ] {
            let mut engine = Engine::new();
            engine
                .register_host_facade(imported_facade())
                .expect("symbolic imported facade installs");
            let program = imported_program(&engine, source);
            let failure = symbolic_imported(&engine, &program)
                .expect_err("a non-Copy imported value cannot be consumed twice");
            let error = failure.into_error();
            assert!(!error.position().is_none(), "{source}");
            assert!(error.to_string().contains("moved value"), "{error}");
        }

    for source in [
            "use imported::Pair; let pair = Pair { left: 1, flag: true }; pair.left; pair.shared(); pair.shared()",
            "use imported::Pair; let mut pair = Pair { left: 1, flag: true }; pair.mutable(); pair.mutable()",
            "use imported::{CopyPair, take_copy}; let copy = CopyPair { value: 1 }; take_copy(copy); take_copy(copy); copy.owned_copy(); copy.owned_copy()",
            "use imported::Pair; let mut destination = Pair { left: 1, flag: true }; let source = Pair { left: 2, flag: false }; destination = source; destination.shared()",
            "use imported::CopyPair; let first = CopyPair { value: 1 }; let second = first; first.value + second.value",
        ] {
            let mut engine = Engine::new();
            engine
                .register_host_facade(imported_facade())
                .expect("symbolic imported facade installs");
            let program = imported_program(&engine, source);
            symbolic_imported(&engine, &program)
                .unwrap_or_else(|failure| panic!("exact imported ownership: {}", failure.into_error()));
        }
}

#[cfg(feature = "rust-import")]
#[test]
fn derived_debug_takes_an_imported_field_whose_type_implements_debug() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(imported_facade())
        .expect("symbolic imported facade installs");
    let program = imported_program(
            &engine,
            "use imported::CopyPair; #[derive(Debug)] struct Panel { pair: CopyPair } let panel = Panel { pair: CopyPair { value: 1 } }; format!(\"{panel:?} {:?}\", panel.pair)",
        );
    symbolic_imported(&engine, &program)
        .unwrap_or_else(|failure| panic!("imported Debug field: {}", failure.into_error()));

    let Err(error) = resolve::resolve(
        engine
            .parse_unit("use imported::Pair; #[derive(Debug)] struct Panel { pair: Pair } 0")
            .expect("imported fixture parses"),
        &engine.host_registry,
    ) else {
        panic!("an imported type without Debug");
    };
    assert!(
        matches!(&error, resolve::Error::InvalidDerive { capability, .. } if capability.as_str() == "Debug"),
        "{error:?}"
    );
}

/// A module re-exports a crate enum's variant by `pub use`, by name and
/// by a glob, and the root names it through the module (#1345).
#[cfg(feature = "rust-import")]
#[test]
fn a_module_re_exports_an_imported_enum_variant() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(imported_facade())
        .expect("symbolic imported facade installs");
    let text = r#"mod picks {
    pub use imported::Choice::Count;
    pub mod all {
        pub use imported::Choice::*;
    }
}
use picks::all::Empty;
fn picked(empty: bool) -> imported::Choice {
    if empty { Empty } else { picks::Count(2) }
}
fn counted() -> imported::Choice {
    picks::all::Count(3)
}
"#;
    let path = std::path::PathBuf::from("/main.rs");
    let snapshot =
        crate::package::snapshot::Snapshot::new(path.clone(), vec![(path, text.into())]).unwrap();
    let graph = crate::package::Graph::load_snapshot(&engine, &snapshot).unwrap();
    let program = resolve::package::resolve(graph, &engine.host_registry)
        .unwrap_or_else(|failure| panic!("re-exported variants: {:?}", failure.error));
    let uses = symbolic_imported(&engine, &program)
        .unwrap_or_else(|failure| panic!("symbolic imported source: {}", failure.into_error()));
    assert!(
        uses.iter()
            .all(|used| used.path.as_str() == "imported::Choice"),
        "{:?}",
        uses.iter()
            .map(|used| used.path.as_str())
            .collect::<Vec<_>>()
    );
    // Two outputs and three variants.
    assert_eq!(uses.len(), 5);
}

#[cfg(feature = "rust-import")]
#[test]
fn an_imported_type_is_send_as_an_iced_message_needs() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(imported_facade())
        .expect("symbolic imported facade installs");
    for path in ["imported::Choice", "imported::Pair"] {
        let Some(host::Entry::Type(definition)) = engine.host_registry.entry(path) else {
            panic!("{path} is an imported type");
        };
        for capability in [
            crate::capability::Kind::Send,
            crate::capability::Kind::Static,
        ] {
            assert!(
                definition.capabilities().contains(capability),
                "{path} {capability:?}"
            );
        }
    }
}

#[cfg(feature = "rust-import")]
#[test]
fn derived_clone_takes_an_imported_field_whose_type_implements_clone() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(imported_facade())
        .expect("symbolic imported facade installs");
    resolve::resolve(
        engine
            .parse_unit(
                "use imported::Choice; #[derive(Clone)] enum Message { Picked(Choice) } \
                     fn again(message: &Message) -> Message { message.clone() } 0",
            )
            .expect("imported fixture parses"),
        &engine.host_registry,
    )
    .unwrap_or_else(|error| panic!("imported Clone field: {error:?}"));

    let Err(error) = resolve::resolve(
        engine
            .parse_unit("use imported::Pair; #[derive(Clone)] struct Panel { pair: Pair } 0")
            .expect("imported fixture parses"),
        &engine.host_registry,
    ) else {
        panic!("an imported type without Clone");
    };
    assert!(
        matches!(&error, resolve::Error::InvalidDerive { capability, .. } if capability.as_str() == "Clone"),
        "{error:?}"
    );
}

mod stress {
    pub(super) struct Element;
    pub(super) struct Container;
    pub(super) struct Text;
    pub(super) struct Style;
}

#[test]
fn inference_transactions_replay_winners_without_leaking_nested_changes() {
    let mut inference = Inference::default();
    let Ty::Infer(stable) = inference.fresh() else {
        panic!("fresh inference type must carry its binding id");
    };
    inference.bind(stable, Ty::Int(super::Int::I64));

    let candidate = inference.checkpoint();
    let fresh = inference.fresh();
    let Ty::Infer(fresh_id) = fresh else {
        panic!("fresh inference type must carry its binding id");
    };
    inference.bind(fresh_id, Ty::Bool);
    let patch = inference.patch(candidate);
    inference.rollback(candidate);

    assert_eq!(inference.next, 1);
    assert_eq!(
        inference.resolve(&Ty::Infer(stable)),
        Ty::Int(super::Int::I64)
    );
    assert_eq!(inference.resolve(&fresh), fresh);

    let statement = inference.checkpoint();
    inference.apply(patch);
    let nested = inference.checkpoint();
    inference.bind(stable, Ty::String);
    inference.commit(nested);
    assert_eq!(inference.resolve(&Ty::Infer(stable)), Ty::String);
    assert_eq!(inference.resolve(&fresh), Ty::Bool);
    inference.rollback(statement);

    assert_eq!(inference.next, 1);
    assert_eq!(
        inference.resolve(&Ty::Infer(stable)),
        Ty::Int(super::Int::I64)
    );
    assert_eq!(inference.resolve(&fresh), fresh);
    assert!(inference.journal.is_empty());
}

#[test]
fn gallery_shaped_compiler_work_scales_linearly() {
    let mut builder = facade::Builder::new("stress", facade::Abi::new(1).unwrap()).unwrap();
    builder
        .type_::<stress::Element>("Element", &["Message"], &[])
        .unwrap();
    builder
        .type_::<stress::Container>("Container", &["Message"], &[])
        .unwrap();
    builder.type_::<stress::Text>("Text", &[], &[]).unwrap();
    builder.type_::<stress::Style>("Style", &[], &[]).unwrap();
    builder
        .project_with("Text", "Element", &[host::type_def::Argument::fresh()])
        .unwrap();
    builder
        .project("Container", "Element", &["Message"])
        .unwrap();

    for input in [
        type_ref::Type::reference(false, type_ref::Type::builtin(type_ref::Builtin::String)),
        type_ref::Type::builtin(type_ref::Builtin::I64),
        type_ref::Type::builtin(type_ref::Builtin::Bool),
    ] {
        let mut text = signature::Builder::new();
        text.parameter(input)
            .output(type_ref::Type::registered("Text").unwrap());
        builder
            .item(item::Definition::function("text", text.finish()).unwrap())
            .unwrap();
    }
    for input in [
        type_ref::Type::applied("Element", [type_ref::Type::parameter("Message").unwrap()])
            .unwrap(),
        type_ref::Type::builtin(type_ref::Builtin::I64),
        type_ref::Type::builtin(type_ref::Builtin::Bool),
    ] {
        let mut container = signature::Builder::new();
        container.type_parameter("Message", &[]).unwrap();
        container
            .parameter(input)
            .parameter(type_ref::Type::parameter("Message").unwrap())
            .output(
                type_ref::Type::applied(
                    "Container",
                    [type_ref::Type::parameter("Message").unwrap()],
                )
                .unwrap(),
            );
        builder
            .item(item::Definition::function("container", container.finish()).unwrap())
            .unwrap();
    }
    builder
        .item(
            item::Definition::value("rounded", type_ref::Type::registered("Style").unwrap())
                .unwrap(),
        )
        .unwrap();
    let mut apply_style = signature::Builder::new();
    apply_style.type_parameter("Message", &[]).unwrap();
    apply_style
        .parameter(type_ref::Type::registered("Style").unwrap())
        .output(
            type_ref::Type::applied("Container", [type_ref::Type::parameter("Message").unwrap()])
                .unwrap(),
        );
    builder
        .item(
            item::Definition::method(
                "Container::style",
                receiver::Mode::Owned,
                apply_style.finish(),
            )
            .unwrap(),
        )
        .unwrap();

    let mut engine = Engine::new();
    engine
        .register_host_facade(builder.finish().unwrap())
        .unwrap();
    let mut observed = Vec::new();
    for rows in [64, 128] {
        let mut source = String::from(
                "use stress::{container, rounded, text};\nenum Message { Tick }\npub fn view() -> stress::Element<Message> {\n",
            );
        for index in 0..rows {
            writeln!(
                source,
                "let row_{index} = container(text(\"row {index}\"), Message::Tick).style(rounded);"
            )
            .unwrap();
        }
        writeln!(source, "row_{}\n}}", rows - 1).unwrap();

        reset_inference_metrics();
        reset_model_clones();
        engine.compile_executable(&source).unwrap();
        let (writes, peak) = inference_metrics();
        observed.push((writes, peak, model_clones()));
    }
    let [(small_writes, small_peak, small_model_clones), (large_writes, large_peak, large_model_clones)] =
        observed.as_slice()
    else {
        panic!("both structural workload sizes must be measured");
    };
    eprintln!(
            "journal writes/peak/model clones: 64 rows={small_writes}/{small_peak}/{small_model_clones}, 128 rows={large_writes}/{large_peak}/{large_model_clones}"
        );
    assert!(*small_writes > 64);
    assert!(*large_writes <= small_writes * 2 + 32);
    assert!(
        *small_peak <= 16,
        "small workload journal peaked at {small_peak}"
    );
    assert!(
        *large_peak <= 16,
        "large workload journal peaked at {large_peak}"
    );
    assert_eq!(small_model_clones, large_model_clones);
    assert!(
        *large_model_clones <= 2,
        "support closures retained {large_model_clones} owned model clones"
    );
}

#[test]
fn external_signature_outputs_retain_both_exact_facade_dependencies() {
    let mut base = facade::Builder::new("base", facade::Abi::new(1).unwrap()).unwrap();
    base.type_::<ExternalValue>("Value", &["Message"], &[])
        .unwrap();
    let base = base.finish().unwrap();
    let exported = base.export_type("Value").unwrap();

    let mut make = signature::Builder::new();
    make.type_parameter("Message", &[]).unwrap();
    make.output(
        type_ref::Type::external(exported, [type_ref::Type::parameter("Message").unwrap()])
            .unwrap(),
    );
    let mut extension = facade::Builder::new("extension", facade::Abi::new(1).unwrap()).unwrap();
    extension
        .item(item::Definition::function("make", make.finish()).unwrap())
        .unwrap();

    let mut engine = Engine::new();
    engine.register_host_facade(base).unwrap();
    engine
        .register_host_facade(extension.finish().unwrap())
        .unwrap();
    let executable = engine
        .compile_executable(
            r#"
use base::Value;
use extension::make;
enum Message { Tick }
pub fn view() -> Value<Message> { make() }
"#,
        )
        .unwrap();
    let roots = executable
        .host_dependencies()
        .iter()
        .map(host::facade::Dependency::root)
        .collect::<Vec<_>>();

    assert_eq!(roots, ["base", "extension"]);
}

#[test]
fn user_cannot_spell_compiler_private_argument_temporaries() {
    let error = Engine::new()
        .compile("pub fn run() { let __rawr_nominal_op_0_arg_0 = 1; }")
        .unwrap_err();

    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::UnsupportedNominalSyntax {
            construct,
            ..
        }) if construct == "compiler-private nominal intrinsic"
    ));
}

#[test]
fn private_call_temporaries_and_inclusive_ranges_preserve_operand_order_once() {
    for level in [
        OptimizationLevel::None,
        OptimizationLevel::Simple,
        OptimizationLevel::Full,
    ] {
        let mut engine = Engine::new();
        engine.set_optimization_level(level);
        engine.register_host_facade(ordering_facade()).unwrap();
        let engine = Shared::new(engine);
        let authority = engine
            .compile_authority(
                r#"
use order::{make_receiver, mark, range_width};
enum Gate { Closed, Open }
pub fn skipped() -> i64 {
    match Gate::Closed {
        Gate::Open if mark(9) == 9 => 1,
        _ => 0,
    }
}
pub fn run() -> i64 {
    make_receiver().combine(mark(1), mark(2))
}
pub fn inclusive() -> i64 {
    range_width(mark(1) + mark(10) ..= mark(2) + mark(20))
}
"#,
            )
            .unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));
        let mut runtime = host::runtime::Builder::new(ordering_facade());
        let observed = events.clone();
        runtime
            .bind("order::make_receiver", move |_| {
                observed.lock().unwrap().push(0);
                Ok(Dynamic::from(OrderedValue))
            })
            .unwrap();
        let observed = events.clone();
        runtime
            .bind("order::mark", move |args| {
                observed.lock().unwrap().push(args[0].as_int().unwrap());
                Ok(args[0].take())
            })
            .unwrap();
        let observed = events.clone();
        runtime
            .bind("order::Value::combine", move |_| {
                observed.lock().unwrap().push(3);
                Ok(Dynamic::from(42_i64))
            })
            .unwrap();
        runtime
            .bind("order::range_width", |args| {
                let range = args[0]
                    .read_lock::<std::ops::RangeInclusive<crate::INT>>()
                    .unwrap();
                assert_eq!(*range, 11..=22);
                Ok(Dynamic::from(*range.end() - *range.start()))
            })
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(runtime.finish()).unwrap();
        let registry = Shared::new(registry);

        let skipped = authority
            .items()
            .find(|item| item.key().name() == "skipped")
            .unwrap()
            .clone();
        let run = authority
            .items()
            .find(|item| item.key().name() == "run")
            .unwrap()
            .clone();
        let inclusive = authority
            .items()
            .find(|item| item.key().name() == "inclusive")
            .unwrap()
            .clone();
        let skipped_output = authority
            .invoke_bound(&skipped, &mut [], registry.clone())
            .unwrap();
        assert_eq!(skipped_output.as_int().unwrap(), 0);
        assert!(events.lock().unwrap().is_empty(), "{level:?}");
        let output = authority
            .invoke_bound(&run, &mut [], registry.clone())
            .unwrap();

        assert_eq!(output.as_int().unwrap(), 42);
        assert_eq!(*events.lock().unwrap(), [0, 1, 2, 3], "{level:?}");

        events.lock().unwrap().clear();
        let output = authority
            .invoke_bound(&inclusive, &mut [], registry.clone())
            .unwrap();
        assert_eq!(output.as_int().unwrap(), 11);
        assert_eq!(*events.lock().unwrap(), [1, 10, 2, 20], "{level:?}");
    }
}

#[test]
fn usage_metadata_perturbs_exact_proofs_and_rejects_unsafe_elision() {
    let ordinary = usage_facade(false, false, false).unwrap();
    let required = usage_facade(true, false, false).unwrap();
    assert_ne!(ordinary.fingerprint(), required.fingerprint());

    let discardable = usage_facade(true, true, false).unwrap();
    assert_ne!(required.fingerprint(), discardable.fingerprint());

    let elidable = usage_facade(true, true, true).unwrap();
    assert_ne!(discardable.fingerprint(), elidable.fingerprint());

    assert!(matches!(
        usage_facade(true, false, true),
        Err(host::Error::InvalidElidableSignature { .. })
    ));

    let mut receiver = usage_builder(true);
    receiver
        .item(
            item::Definition::method(
                "Value::produce",
                receiver::Mode::Owned,
                value_signature().finish(),
            )
            .unwrap()
            .discardable()
            .elidable(),
        )
        .unwrap();
    assert!(matches!(
        receiver.finish(),
        Err(host::Error::InvalidElidableSignature { .. })
    ));

    let mut parameter = usage_builder(true);
    let mut definition = value_signature();
    definition.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    parameter
        .item(
            item::Definition::function("produce", definition.finish())
                .unwrap()
                .discardable()
                .elidable(),
        )
        .unwrap();
    assert!(matches!(
        parameter.finish(),
        Err(host::Error::InvalidElidableSignature { .. })
    ));
}

#[test]
fn usage_metadata_retains_required_work_and_erases_exact_inert_work() {
    let mut engine = Engine::new();
    engine.register_host_facade(execution_facade()).unwrap();
    let engine = Shared::new(engine);
    let authority = engine
        .compile_authority(
            r#"
use effect::{idle, identity, required};
pub fn run() -> i64 {
    required();
    idle();
    identity(42)
}
"#,
        )
        .unwrap();

    assert_eq!(authority.runtime_requirements().len(), 2);

    let required_calls = Arc::new(AtomicUsize::new(0));
    let idle_calls = Arc::new(AtomicUsize::new(0));
    let identity_calls = Arc::new(AtomicUsize::new(0));
    let mut runtime = host::runtime::Builder::new(execution_facade());
    let calls = required_calls.clone();
    runtime
        .bind("effect::required", move |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(Dynamic::from(RequiredValue))
        })
        .unwrap();
    let calls = idle_calls.clone();
    runtime
        .bind("effect::idle", move |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(Dynamic::from(GenericValue))
        })
        .unwrap();
    let calls = identity_calls.clone();
    runtime
        .bind("effect::identity", move |args| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(args[0].take())
        })
        .unwrap();
    let mut registry = host::runtime::Registry::new();
    registry.install(runtime.finish()).unwrap();

    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    let output = authority
        .invoke_bound(&run, &mut [], Shared::new(registry))
        .unwrap();

    assert_eq!(output.as_int().unwrap(), 42);
    assert_eq!(required_calls.load(Ordering::Relaxed), 1);
    assert_eq!(idle_calls.load(Ordering::Relaxed), 0);
    assert_eq!(identity_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn recursive_member_schema_changes_invalidate_retained_values() {
    let engine = Engine::new();
    let old = resolve::resolve(
            engine
                .parse_unit(
                    "#[derive(Default)] struct Inner { value: i64 } #[derive(Default)] struct Outer { inner: Inner }",
                )
                .unwrap(),
            &host::Registry::new(),
        )
        .unwrap();
    let new = resolve::resolve(
            engine
                .parse_unit(
                    "#[derive(Default)] struct Inner { value: bool } #[derive(Default)] struct Outer { inner: Inner }",
                )
                .unwrap(),
            &host::Registry::new(),
        )
        .unwrap();
    let old_generation = crate::program::GenerationId::issue();
    let new_generation = crate::program::GenerationId::issue();
    let old_runtime = crate::program::model::Model::new(old_generation, &old);
    let new_runtime = crate::program::model::Model::new(new_generation, &new);
    let old_models = Models::new(
        &old,
        old_runtime,
        Shared::new(host::Registry::new()),
        &Engine::new_raw(),
    );
    let new_models = Models::new(
        &new,
        new_runtime,
        Shared::new(host::Registry::new()),
        &Engine::new_raw(),
    );
    let old_outer = old_models.get("Outer").unwrap();
    let new_outer = new_models.get("Outer").unwrap();
    let retained = default_value(old_outer, &old_models, &mut Vec::new()).unwrap();

    assert_eq!(
        old_outer.definition().schema(),
        new_outer.definition().schema()
    );
    assert!(!matches_runtime_model(&retained, new_outer, &new_models));
}

fn usage_facade(
    required: bool,
    discardable: bool,
    elidable: bool,
) -> Result<facade::Definition, host::Error> {
    let mut builder = usage_builder(required);
    let mut definition = item::Definition::function("produce", value_signature().finish())?;
    if discardable {
        definition = definition.discardable();
    }
    if elidable {
        definition = definition.elidable();
    }
    builder.item(definition)?;
    builder.finish()
}

#[cfg(feature = "rust-import")]
fn imported_program(engine: &Engine, source: &str) -> resolve::Program {
    resolve::resolve(
        engine.parse_unit(source).expect("imported fixture parses"),
        &engine.host_registry,
    )
    .expect("imported fixture headers resolve")
}

#[cfg(feature = "rust-import")]
fn imported_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("imported", facade::Abi::new(1).unwrap()).unwrap();
    let mut allocation = host::TestImportAllocation::default();
    let pair_identity = host::type_def::import::Identity::new([0x31; 32]);
    let copy_identity = host::type_def::import::Identity::new([0x33; 32]);
    let state_identity = host::type_def::import::Identity::new([0x35; 32]);
    builder.module("domain").unwrap();
    let pair_shape: Shared<host::type_def::Shape> = host::type_def::Shape::Struct {
        form: host::type_def::Form::Record,
        constructor: Some(rawr_import::surface::identity::Selector::Unavailable),
        fields: Box::from([
            host::type_def::Field::new(
                "left".into(),
                type_ref::Type::builtin(type_ref::Builtin::I64),
                rawr_import::surface::identity::Selector::Unavailable,
            ),
            host::type_def::Field::new(
                "flag".into(),
                type_ref::Type::builtin(type_ref::Builtin::Bool),
                rawr_import::surface::identity::Selector::Unavailable,
            ),
        ]),
        constructible: true,
        unsupported: None,
    }
    .into();
    builder
        .imported_type(
            "Pair",
            pair_identity,
            &[],
            Box::default(),
            Box::default(),
            pair_shape,
            &mut allocation,
        )
        .unwrap();
    builder
        .imported_type(
            "domain::State",
            state_identity,
            &[],
            Box::default(),
            Box::default(),
            host::type_def::Shape::Struct {
                form: host::type_def::Form::Record,
                constructor: None,
                fields: Box::from([
                    host::type_def::Field::new(
                        "dial".into(),
                        type_ref::Type::builtin(type_ref::Builtin::I64),
                        rawr_import::surface::identity::Selector::Unavailable,
                    ),
                    host::type_def::Field::new(
                        "enabled".into(),
                        type_ref::Type::builtin(type_ref::Builtin::Bool),
                        rawr_import::surface::identity::Selector::Unavailable,
                    ),
                ]),
                constructible: false,
                unsupported: None,
            }
            .into(),
            &mut allocation,
        )
        .unwrap();
    let mut state_new = signature::Builder::new();
    state_new
        .parameter(type_ref::Type::builtin(type_ref::Builtin::I64))
        .parameter(type_ref::Type::builtin(type_ref::Builtin::Bool))
        .output(type_ref::Type::imported(state_identity, []));
    builder
        .item(
            item::Definition::associated_function("domain::State::new", state_new.finish())
                .unwrap()
                .imported(rawr_import::surface::identity::Selector::Unavailable),
        )
        .unwrap();
    builder
        .imported_type(
            "Holder",
            host::type_def::import::Identity::new([0x34; 32]),
            &[],
            Box::default(),
            Box::default(),
            host::type_def::Shape::Struct {
                form: host::type_def::Form::Record,
                constructor: Some(rawr_import::surface::identity::Selector::Unavailable),
                fields: Box::from([host::type_def::Field::new(
                    "pair".into(),
                    type_ref::Type::imported(pair_identity, []),
                    rawr_import::surface::identity::Selector::Unavailable,
                )]),
                constructible: true,
                unsupported: None,
            }
            .into(),
            &mut allocation,
        )
        .unwrap();
    builder
        .imported_type(
            "CopyPair",
            copy_identity,
            &[
                crate::capability::Kind::Copy,
                crate::capability::Kind::Debug,
                crate::capability::Kind::Clone,
            ],
            Box::default(),
            Box::default(),
            host::type_def::Shape::Struct {
                form: host::type_def::Form::Record,
                constructor: Some(rawr_import::surface::identity::Selector::Unavailable),
                fields: Box::from([host::type_def::Field::new(
                    "value".into(),
                    type_ref::Type::builtin(type_ref::Builtin::I64),
                    rawr_import::surface::identity::Selector::Unavailable,
                )]),
                constructible: true,
                unsupported: None,
            }
            .into(),
            &mut allocation,
        )
        .unwrap();
    builder
        .imported_type(
            "Choice",
            host::type_def::import::Identity::new([0x32; 32]),
            &[crate::capability::Kind::Clone],
            Box::default(),
            Box::default(),
            host::type_def::Shape::Enum {
                variants: Box::from([
                    host::type_def::Variant::new(
                        "Empty".into(),
                        Box::default(),
                        true,
                        rawr_import::surface::identity::Selector::EnumVariant {
                            index: 0,
                            name: "Empty".into(),
                            form: rawr_import::surface::identity::Form::Unit,
                        },
                    ),
                    host::type_def::Variant::new(
                        "Count".into(),
                        Box::from([host::type_def::Field::new(
                            "0".into(),
                            type_ref::Type::builtin(type_ref::Builtin::I64),
                            rawr_import::surface::identity::Selector::Unavailable,
                        )]),
                        true,
                        rawr_import::surface::identity::Selector::EnumVariant {
                            index: 1,
                            name: "Count".into(),
                            form: rawr_import::surface::identity::Form::Tuple { arity: 1 },
                        },
                    ),
                ]),
                non_exhaustive: false,
            }
            .into(),
            &mut allocation,
        )
        .unwrap();
    for (name, mode) in [
        ("shared", receiver::Mode::Shared),
        ("mutable", receiver::Mode::Mutable),
        ("owned", receiver::Mode::Owned),
    ] {
        let mut signature = signature::Builder::new();
        signature.output(type_ref::Type::imported(pair_identity, []));
        builder
            .item(
                item::Definition::method(format!("Pair::{name}"), mode, signature.finish())
                    .unwrap()
                    .imported(rawr_import::surface::identity::Selector::Unavailable),
            )
            .unwrap();
    }
    let mut combine = signature::Builder::new();
    combine
        .parameter(type_ref::Type::imported(pair_identity, []))
        .output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder
        .item(
            item::Definition::method("Pair::combine", receiver::Mode::Shared, combine.finish())
                .unwrap()
                .imported(rawr_import::surface::identity::Selector::Unavailable),
        )
        .unwrap();
    let mut take_pair = signature::Builder::new();
    take_pair
        .parameter(type_ref::Type::imported(pair_identity, []))
        .output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder
        .item(
            item::Definition::function("take_pair", take_pair.finish())
                .unwrap()
                .imported(rawr_import::surface::identity::Selector::Unavailable),
        )
        .unwrap();
    let mut take_copy = signature::Builder::new();
    take_copy
        .parameter(type_ref::Type::imported(copy_identity, []))
        .output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder
        .item(
            item::Definition::function("take_copy", take_copy.finish())
                .unwrap()
                .imported(rawr_import::surface::identity::Selector::Unavailable),
        )
        .unwrap();
    let mut owned_copy = signature::Builder::new();
    owned_copy.output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder
        .item(
            item::Definition::method(
                "CopyPair::owned_copy",
                receiver::Mode::Owned,
                owned_copy.finish(),
            )
            .unwrap()
            .imported(rawr_import::surface::identity::Selector::Unavailable),
        )
        .unwrap();
    builder.finish().unwrap()
}

#[cfg(feature = "rust-import")]
fn imported_alias_facade(alias_first: bool) -> facade::Definition {
    let mut builder = facade::Builder::new("aliases", facade::Abi::new(1).unwrap()).unwrap();
    let mut allocation = host::TestImportAllocation::default();
    let identity = host::type_def::import::Identity::new([0x41; 32]);
    let shape: Shared<host::type_def::Shape> = host::type_def::Shape::Struct {
        form: host::type_def::Form::Record,
        constructor: Some(rawr_import::surface::identity::Selector::Unavailable),
        fields: Box::from([host::type_def::Field::new(
            "value".into(),
            type_ref::Type::builtin(type_ref::Builtin::I64),
            rawr_import::surface::identity::Selector::Unavailable,
        )]),
        constructible: true,
        unsupported: None,
    }
    .into();
    let aliases = if alias_first {
        ["Second", "First"]
    } else {
        ["First", "Second"]
    };
    for alias in aliases {
        builder
            .imported_type(
                alias,
                identity,
                &[],
                Box::default(),
                Box::default(),
                shape.clone(),
                &mut allocation,
            )
            .unwrap();
        let mut signature = signature::Builder::new();
        signature.output(type_ref::Type::imported(identity, []));
        builder
            .item(
                item::Definition::method(
                    format!("{alias}::recursive"),
                    receiver::Mode::Shared,
                    signature.finish(),
                )
                .unwrap()
                .imported(rawr_import::surface::identity::Selector::Unavailable),
            )
            .unwrap();
    }
    let mut make = signature::Builder::new();
    make.output(type_ref::Type::imported(identity, []));
    builder
        .item(
            item::Definition::function("make", make.finish())
                .unwrap()
                .imported(rawr_import::surface::identity::Selector::Unavailable),
        )
        .unwrap();
    builder.finish().unwrap()
}

fn usage_builder(required: bool) -> facade::Builder {
    let mut builder = facade::Builder::new("usage", facade::Abi::new(1).unwrap()).unwrap();
    if required {
        builder
            .required_type::<RequiredValue>("Value", &[], &[])
            .unwrap();
    } else {
        builder.type_::<RequiredValue>("Value", &[], &[]).unwrap();
    }
    builder
}

fn value_signature() -> signature::Builder {
    let mut signature = signature::Builder::new();
    signature.output(type_ref::Type::registered("Value").unwrap());
    signature
}

fn execution_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("effect", facade::Abi::new(1).unwrap()).unwrap();
    builder
        .required_type::<RequiredValue>("Value", &[], &[])
        .unwrap();
    builder
        .required_type::<GenericValue>("Slot", &["T"], &[])
        .unwrap();

    let mut required = signature::Builder::new();
    required.output(type_ref::Type::registered("Value").unwrap());
    builder
        .item(
            item::Definition::function("required", required.finish())
                .unwrap()
                .discardable(),
        )
        .unwrap();

    let mut idle = signature::Builder::new();
    idle.type_parameter("T", &[]).unwrap();
    idle.output(
        type_ref::Type::applied("Slot", [type_ref::Type::parameter("T").unwrap()]).unwrap(),
    );
    builder
        .item(
            item::Definition::function("idle", idle.finish())
                .unwrap()
                .discardable()
                .elidable(),
        )
        .unwrap();

    let mut identity = signature::Builder::new();
    identity.type_parameter("T", &[]).unwrap();
    identity.parameter(type_ref::Type::parameter("T").unwrap());
    identity.output(type_ref::Type::parameter("T").unwrap());
    builder
        .item(item::Definition::function("identity", identity.finish()).unwrap())
        .unwrap();

    builder.finish().unwrap()
}

fn ordering_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("order", facade::Abi::new(1).unwrap()).unwrap();
    builder
        .required_type::<OrderedValue>("Value", &[], &[])
        .unwrap();

    let mut make_receiver = signature::Builder::new();
    make_receiver.output(type_ref::Type::registered("Value").unwrap());
    builder
        .item(item::Definition::function("make_receiver", make_receiver.finish()).unwrap())
        .unwrap();

    let mut mark = signature::Builder::new();
    mark.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    mark.output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder
        .item(item::Definition::function("mark", mark.finish()).unwrap())
        .unwrap();

    let mut range_width = signature::Builder::new();
    range_width.parameter(type_ref::Type::builtin(type_ref::Builtin::InclusiveRange));
    range_width.output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder
        .item(item::Definition::function("range_width", range_width.finish()).unwrap())
        .unwrap();

    let mut combine = signature::Builder::new();
    combine.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    combine.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    combine.output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder
        .item(
            item::Definition::method("Value::combine", receiver::Mode::Owned, combine.finish())
                .unwrap(),
        )
        .unwrap();

    builder.finish().unwrap()
}

/// A facade of three crate enums of `Pressed(i64)`: `Event` has that one
/// variant, `Sealed` too but is `#[non_exhaustive]`, and `Key` has a
/// `Released` beside it.
#[cfg(feature = "rust-import")]
fn variant_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("keys", facade::Abi::new(1).unwrap()).unwrap();
    let mut allocation = host::TestImportAllocation::default();
    let variant = |index: u32, name: &str, fields: Box<[host::type_def::Field]>| {
        let form = if fields.is_empty() {
            rawr_import::surface::identity::Form::Unit
        } else {
            rawr_import::surface::identity::Form::Tuple { arity: 1 }
        };
        host::type_def::Variant::new(
            name.into(),
            fields,
            true,
            rawr_import::surface::identity::Selector::EnumVariant {
                index,
                name: name.into(),
                form,
            },
        )
    };
    let pressed = || {
        variant(
            0,
            "Pressed",
            Box::from([host::type_def::Field::new(
                "0".into(),
                type_ref::Type::builtin(type_ref::Builtin::I64),
                rawr_import::surface::identity::Selector::Unavailable,
            )]),
        )
    };
    for (name, identity, variants, non_exhaustive) in [
        ("Event", 0x51, vec![pressed()], false),
        ("Sealed", 0x52, vec![pressed()], true),
        (
            "Key",
            0x53,
            vec![pressed(), variant(1, "Released", Box::default())],
            false,
        ),
    ] {
        builder
            .imported_type(
                name,
                host::type_def::import::Identity::new([identity; 32]),
                &[crate::capability::Kind::Clone],
                Box::default(),
                Box::default(),
                host::type_def::Shape::Enum {
                    variants: variants.into_boxed_slice(),
                    non_exhaustive,
                }
                .into(),
                &mut allocation,
            )
            .unwrap();
    }
    builder.finish().unwrap()
}

/// `let Event::Pressed(label) = event;` on a crate enum of that one
/// variant is irrefutable, as rustc has it (#1681). A pattern that misses
/// some of its field's values, a `#[non_exhaustive]` enum, and an enum
/// with another variant are E0005, with rustc's witnesses.
#[cfg(feature = "rust-import")]
#[test]
fn a_let_on_a_crate_enums_only_variant_is_irrefutable() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(variant_facade())
        .expect("variant facade installs");
    let lowered = |ty: &str, pattern: &str| {
        let source = format!(
            "fn take(value: keys::{ty}) -> i64 {{\n    \
             let keys::{ty}::{pattern} = value;\n    0\n}}\n0\n"
        );
        let program = imported_program(&engine, &source);
        symbolic_imported(&engine, &program).map_err(|failure| failure.into_error())
    };
    for pattern in ["Pressed(label)", "Pressed(_)", "Pressed(..)"] {
        lowered("Event", pattern).unwrap_or_else(|error| panic!("{pattern}: {error}"));
    }
    for (ty, pattern, expected) in [
        (
            "Event",
            "Pressed(0)",
            &[
                "Event::Pressed(i64::MIN..=-1_i64)",
                "Event::Pressed(1_i64..=i64::MAX)",
            ][..],
        ),
        ("Sealed", "Pressed(label)", &["_"][..]),
        ("Key", "Pressed(label)", &["Key::Released"][..]),
    ] {
        let error = lowered(ty, pattern).expect_err("the pattern misses some values");
        let ParseErrorType::Resolution(resolve::Error::Refutable { witnesses, .. }) =
            error.err_type()
        else {
            panic!("{ty}::{pattern}: {error}");
        };
        let witnesses = witnesses.iter().map(|w| w.as_str()).collect::<Vec<_>>();
        assert_eq!(witnesses, expected, "{ty}::{pattern}");
    }
}

/// A `match` on a `#[non_exhaustive]` crate enum covers it only with an
/// arm that matches anything, as rustc has it outside the enum's crate:
/// E0004 with `_`, or `&_` through a reference.
#[cfg(feature = "rust-import")]
#[test]
fn a_match_on_a_non_exhaustive_crate_enum_needs_a_wildcard() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(variant_facade())
        .expect("variant facade installs");
    let lowered = |parameter: &str, arms: &str| {
        let source = format!(
            "fn take(value: {parameter}) -> i64 {{\n    match value {{\n        \
             {arms}\n    }}\n}}\n0\n"
        );
        let program = imported_program(&engine, &source);
        symbolic_imported(&engine, &program).map_err(|failure| failure.into_error())
    };
    for (parameter, arms) in [
        (
            "keys::Sealed",
            "keys::Sealed::Pressed(label) => label,\n        _ => 0,",
        ),
        (
            "keys::Key",
            "keys::Key::Pressed(label) => label,\n        keys::Key::Released => 0,",
        ),
        ("keys::Event", "keys::Event::Pressed(label) => label,"),
    ] {
        lowered(parameter, arms).unwrap_or_else(|error| panic!("{parameter}: {error}"));
    }
    for (parameter, arms, expected) in [
        (
            "keys::Sealed",
            "keys::Sealed::Pressed(label) => label,",
            "_",
        ),
        (
            "&keys::Sealed",
            "keys::Sealed::Pressed(label) => *label,",
            "&_",
        ),
    ] {
        let error = lowered(parameter, arms).expect_err("no arm matches anything");
        let ParseErrorType::Resolution(resolve::Error::NonExhaustive { witnesses, .. }) =
            error.err_type()
        else {
            panic!("{parameter}: {error}");
        };
        let witnesses = witnesses.iter().map(|w| w.as_str()).collect::<Vec<_>>();
        assert_eq!(witnesses, [expected], "{parameter}");
    }
}

#[cfg(feature = "rust-import")]
#[test]
fn an_or_pattern_over_crate_variants_below_a_program_variant_binds_no_names() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(imported_facade())
        .expect("symbolic imported facade installs");
    let lowered = |alternatives: &str| {
        let source = format!(
            "use imported::Choice;\nenum Message {{ Picked(usize, Choice), Other }}\n\
             fn pick(message: Message) -> i64 {{\n    match message {{\n        \
             {alternatives} => 1,\n        _ => 0,\n    }}\n}}\n0\n"
        );
        let program = imported_program(&engine, &source);
        symbolic_imported(&engine, &program).map_err(|failure| failure.into_error())
    };
    // Neither alternative binds a name, as elevato's
    // `Message::Floor(_, floor::Message::UpButtonPressed) | ...` binds none.
    for alternatives in [
        "Message::Picked(_, imported::Choice::Empty) \
         | Message::Picked(_, imported::Choice::Count(_))",
        "Message::Picked(n, imported::Choice::Empty) \
         | Message::Picked(n, imported::Choice::Count(_)) if n > 0",
    ] {
        lowered(alternatives).unwrap_or_else(|error| panic!("{alternatives}: {error}"));
    }
    // A name the crate part of one alternative binds and another's does not.
    let error = lowered(
        "Message::Picked(_, imported::Choice::Count(n)) \
         | Message::Picked(_, imported::Choice::Empty)",
    )
    .expect_err("one alternative binds `n`");
    assert!(
        matches!(
            error.err_type(),
            ParseErrorType::Resolution(resolve::Error::UnboundAlternative { name, .. })
                if name.as_str() == "n"
        ),
        "{error}"
    );
}
