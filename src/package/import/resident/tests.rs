use super::{account, Error, Outcome};
use crate::host::ImportAllocation as _;
use crate::package::compile::captured::tests::{
    compiler, native_snapshot, preview_runtime, snapshot, source,
};
use crate::{host, Engine, Shared};

fn retained() -> (Shared<Engine>, host::facade::ProfileCycle) {
    let (engine, cycle) = compiler();
    let native = account::Native::transform(account::tests::carrier()).unwrap();
    (engine.retain_imported_native(native).unwrap(), cycle)
}

fn usage(engine: &Engine) -> (usize, usize) {
    engine.imported_native.as_ref().unwrap().usage()
}

#[test]
fn resident_compilation_requires_the_engines_retained_native_context() {
    let (engine, cycle) = compiler();
    assert!(matches!(
        engine.compile_package_snapshot_resident(&snapshot(&source("Foo")), &cycle),
        Err(Error::Unowned)
    ));
}

#[test]
fn native_miss_and_source_error_release_attempts_without_retry_classification() {
    let (engine, cycle) = retained();
    let before = usage(&engine);
    assert!(matches!(
        engine.compile_package_snapshot_resident(&snapshot(&source("Foo")), &cycle),
        Ok(Outcome::NeedsPreparation)
    ));
    assert_eq!(usage(&engine), before);
    let invalid = source("Foo").replace("native::echo(7)", "true");
    assert!(matches!(
        engine.compile_package_snapshot_resident(&snapshot(&invalid), &cycle),
        Err(Error::Source(_))
    ));
    assert_eq!(usage(&engine), before);
}

#[test]
fn terminal_native_miss_releases_attempt_and_preserves_source_error_boundary() {
    let (engine, cycle) = retained();
    let before = usage(&engine);
    for source in [
        "use native::new_function; pub fn sample() -> i64 { new_function() }",
        "pub fn sample() -> i64 { native::new_function() }",
        "pub fn sample(value: native::NewType) {}",
    ] {
        assert!(matches!(
            engine.compile_package_snapshot_resident(&native_snapshot(source), &cycle),
            Ok(Outcome::NeedsPreparation)
        ));
        assert_eq!(usage(&engine), before);
    }
    assert!(matches!(
        engine.compile_package_snapshot_resident(
            &native_snapshot("pub fn sample() -> i64 { misspelled() }"),
            &cycle
        ),
        Err(Error::Source(_))
    ));
    assert_eq!(usage(&engine), before);
}

#[test]
fn resident_receipt_follows_executable_authority_after_generation_metadata_retires() {
    let (engine, cycle) = retained();
    let before = usage(&engine);
    let pure =
        snapshot("#[preview] pub fn sample() -> iced::Element<()> { throw \"not invoked\" }");
    let Outcome::Compiled(compiled) = engine
        .compile_package_snapshot_resident(&pure, &cycle)
        .unwrap()
    else {
        panic!("no native requirement needs preparation");
    };
    let retained = usage(&engine);
    assert!(retained.0 > before.0 && retained.1 > before.1);
    let (candidate, semantic, carrier, coverage) = compiled.into_parts();
    assert!(semantic.is_none());
    assert_eq!(
        carrier,
        *engine
            .imported_native
            .as_ref()
            .unwrap()
            .carrier()
            .identity()
    );
    assert_ne!(coverage, [0; 32]);
    let id = candidate.id();
    let bootstrap = candidate.bind(&engine, preview_runtime()).unwrap();
    assert_eq!(bootstrap.generation().id(), id);
    // This transformation drops Generation metadata while the executable lives.
    let catalog = bootstrap.into_snapshot_catalog();
    let retained_catalog = catalog.clone();
    drop(catalog);
    assert_eq!(usage(&engine), retained);
    drop(retained_catalog);
    assert_eq!(usage(&engine), before);
}

#[test]
fn initial_and_ordinary_generations_share_the_resident_account() {
    let (engine, cycle) = retained();
    let baseline = usage(&engine);
    let pure =
        snapshot("#[preview] pub fn sample() -> iced::Element<()> { throw \"not invoked\" }");
    let (initial, ast) = engine
        .compile_package_snapshot_generation_with_top_level_checked(&pure, &cycle, &[0; 32])
        .unwrap();
    let live = usage(&engine);
    assert!(live.0 > baseline.0 && live.1 > baseline.1);
    let ordinary = engine
        .compile_package_snapshot_generation_checked(&pure, &cycle, &[0; 32])
        .unwrap();
    let overlapping = usage(&engine);
    assert!(overlapping.0 > live.0 && overlapping.1 > live.1);
    drop(ordinary);
    assert_eq!(usage(&engine), live);
    drop(ast);
    drop(initial);
    assert_eq!(usage(&engine), baseline);
}

mod top_level {
    use super::{account, compiler, preview_runtime, retained, snapshot, usage, Outcome};
    use crate::host::ImportAllocation as _;
    use crate::{Dynamic, Engine, Shared, AST};

    // The generated helper only needs these two Program fields. Include its
    // emitted source unchanged so the lifetime regression exercises shipping code.
    struct Program {
        engine: Shared<Engine>,
        ast: Option<AST>,
    }

    include!("../../../../importer/src/bridge/generate/top_level.rs");

    #[test]
    fn neutral_snapshot_retains_support_until_its_last_ast_copy_retires() {
        let (engine, _) = retained();
        let baseline = usage(&engine);
        let ast = engine
            .compile_package_snapshot(&snapshot(
                "struct State { value: i64 }\n\
             #[preview] pub fn sample() -> iced::Element<()> {\n\
                 let state = State { value: 7 };\n\
                 throw \"not invoked\";\n\
             }",
            ))
            .unwrap();
        assert!(!ast.shared_support().is_empty());
        let support = Shared::downgrade(ast.shared_support());
        let copy = ast.clone_functions_only();
        let live = usage(&engine);
        assert!(live.0 > baseline.0 && live.1 > baseline.1);
        let mut program = Program {
            engine: engine.clone(),
            ast: Some(ast),
        };
        evaluate_program(&mut program).unwrap();
        assert!(support.upgrade().is_some());
        assert_eq!(usage(&engine), live);
        drop(copy);
        assert!(support.upgrade().is_none());
        assert_eq!(usage(&engine), baseline);
    }

    #[test]
    fn neutral_reconstruction_keeps_prior_constant_cycles_in_the_original_account() {
        let (engine, _) = retained();
        let original = engine.imported_native_snapshot().unwrap();
        let source = snapshot(
            "struct State { value: i64 }\n\
             #[preview] pub fn sample() -> iced::Element<()> {\n\
                 let state = State { value: 7 };\n\
                 throw \"not invoked\";\n\
             }\n\
             const first = 42;\n\
             fn answer() { 42 }\n\
             const callback = answer;\n\
             callback",
        );
        let ast = engine.compile_package_snapshot(&source).unwrap();
        let first_support = Shared::downgrade(ast.shared_support());
        assert!(!ast.shared_support().is_empty());
        let mut program = Program {
            engine: engine.clone(),
            ast: Some(ast),
        };
        evaluate_program(&mut program).unwrap();
        let first_live = engine.imported_native_snapshot().unwrap();
        assert!(first_live.current[1] > original.current[1]);
        let (native, observation) =
            engine.reconstruct_imported_native(&account::tests::carrier_bytes(&[]));
        let native = native.unwrap();
        let (replacement, _) = compiler();
        let replacement = replacement.retain_imported_native(native).unwrap();
        drop(program);
        drop(engine);
        assert!(first_support.upgrade().is_some());
        let reconstructed = replacement.imported_native_snapshot().unwrap();
        assert_eq!(reconstructed.account, original.account);
        assert_eq!(reconstructed.limits, original.limits);
        assert!(reconstructed.current[1] > first_live.current[1]);

        let ast = replacement.compile_package_snapshot(&source).unwrap();
        let second_support = Shared::downgrade(ast.shared_support());
        let mut program = Program {
            engine: replacement.clone(),
            ast: Some(ast),
        };
        evaluate_program(&mut program).unwrap();
        drop(program);
        let retained = replacement.imported_native_snapshot().unwrap();
        assert!(retained.current[1] > reconstructed.current[1]);
        assert!(first_support.upgrade().is_some());
        assert!(second_support.upgrade().is_some());
        assert_eq!(observation.snapshot(), Some(retained));

        let native = replacement.imported_native.as_ref().unwrap();
        let mut pressure = native.attempt().unwrap();
        let available = retained.limits[1] - native.usage().1;
        pressure.retain_checked(Some(0), Some(available)).unwrap();
        // The byte target is accounting. Reconstruction past it is
        // accepted and recorded on the original runner account with a
        // truthful high water; nothing is refused or latched.
        let (accepted, exceeded_observation) =
            replacement.reconstruct_imported_native(&account::tests::carrier_bytes(&[]));
        let accepted = accepted.unwrap();
        let exceeded = native.snapshot();
        assert_eq!(exceeded.account, original.account);
        assert!(exceeded.first_refusal.is_none());
        assert!(exceeded.exceeded.axes().any(|axis| {
            axis.target as usize == retained.limits[1]
                && axis.high_water as usize > retained.limits[1]
        }));
        assert_eq!(exceeded_observation.snapshot(), Some(exceeded));
        // The accepted observation retains its own scope until it retires.
        drop(exceeded_observation);
        drop(accepted);
        drop(pressure);
        assert_eq!(native.snapshot().current, retained.current);
        assert!(native.snapshot().first_refusal.is_none());
        assert_eq!(native.snapshot().exceeded, exceeded.exceeded);
        assert!(first_support.upgrade().is_some());
        assert!(second_support.upgrade().is_some());
    }

    #[test]
    fn generated_top_level_retires_initial_support_with_its_last_executable() {
        let (engine, cycle) = retained();
        let baseline = usage(&engine);
        let source = "struct State { value: i64 }\n\
            #[preview] pub fn sample() -> iced::Element<()> {\n\
                let state = State { value: 7 };\n\
                throw \"not invoked\";\n\
            }";
        let (initial, ast) = engine
            .compile_package_snapshot_generation_with_top_level_checked(
                &snapshot(source),
                &cycle,
                &[0; 32],
            )
            .unwrap();
        assert!(!ast.shared_support().is_empty());
        let support = Shared::downgrade(ast.shared_support());
        let initial_usage = usage(&engine);
        assert!(initial_usage.0 > baseline.0 && initial_usage.1 > baseline.1);
        let initial = initial
            .bind(&engine, preview_runtime())
            .unwrap()
            .into_snapshot_catalog();
        let last_good = initial.clone();
        let mut program = Program {
            engine: engine.clone(),
            ast: Some(ast),
        };
        evaluate_program(&mut program).unwrap();
        assert!(program.ast.is_none());
        assert!(support.upgrade().is_some());
        assert_eq!(usage(&engine), initial_usage);

        let Outcome::Compiled(resident) = engine
            .compile_package_snapshot_resident(&snapshot(&source.replace("7", "8")), &cycle)
            .unwrap()
        else {
            panic!("the resident edit has no new native requirement");
        };
        let overlapping = usage(&engine);
        assert!(overlapping.0 > initial_usage.0 && overlapping.1 > initial_usage.1);
        drop(initial);
        assert!(support.upgrade().is_some());
        assert_eq!(usage(&engine), overlapping);
        drop(last_good);
        assert!(support.upgrade().is_none());
        assert_eq!(
            usage(&engine),
            (
                overlapping.0 - (initial_usage.0 - baseline.0),
                overlapping.1 - (initial_usage.1 - baseline.1),
            )
        );
        drop(resident);
        assert_eq!(usage(&engine), baseline);
        // The original runner Program remains live after resident replacement.
        assert!(program.ast.is_none());
        assert!(evaluate_program(&mut program)
            .unwrap_err()
            .to_string()
            .contains("already evaluated"));
        assert_eq!(usage(&engine), baseline);
    }

    #[test]
    fn generated_top_level_discards_acyclic_function_environments_on_success_and_error() {
        for (source, succeeds) in [
            ("fn answer() { 42 } let callback = answer; callback", true),
            (
                "fn answer() { 42 } let callback = answer; throw callback;",
                false,
            ),
        ] {
            let engine = Shared::new(Engine::new());
            let ast = engine.compile(source).unwrap();
            let functions = Shared::downgrade(ast.shared_lib());
            let support = Shared::downgrade(ast.shared_support());
            let mut program = Program {
                engine,
                ast: Some(ast),
            };
            let result = evaluate_program(&mut program);
            assert_eq!(result.is_ok(), succeeds, "{result:?}");
            assert!(program.ast.is_none());
            assert!(
                functions.upgrade().is_none(),
                "function library retained: {source}"
            );
            assert!(
                support.upgrade().is_none(),
                "compiler support retained: {source}"
            );
        }
    }

    #[test]
    fn generated_top_level_const_cycle_keeps_candidate_support_charged() {
        let (engine, cycle) = retained();
        let baseline = usage(&engine);
        let source = "struct State { value: i64 }\n\
            #[preview] pub fn sample() -> iced::Element<()> {\n\
                let state = State { value: 7 };\n\
                throw \"not invoked\";\n\
            }\n\
            const first = 42;\n\
            fn answer() { 42 }\n\
            const callback = answer;\n\
            callback";
        let (candidate, ast) = engine
            .compile_package_snapshot_generation_with_top_level_checked(
                &snapshot(source),
                &cycle,
                &[0; 32],
            )
            .unwrap();
        assert!(!ast.shared_support().is_empty());
        let support = Shared::downgrade(ast.shared_support());
        let live = usage(&engine);
        assert!(live.0 > baseline.0 && live.1 > baseline.1);
        let candidate = candidate
            .bind(&engine, preview_runtime())
            .unwrap()
            .into_snapshot_catalog();
        let mut program = Program {
            engine: engine.clone(),
            ast: Some(ast),
        };
        evaluate_program(&mut program).unwrap();
        assert!(program.ast.is_none());
        assert!(support.upgrade().is_some());
        assert_eq!(usage(&engine), live);
        drop(candidate);
        // This preserves the existing constants/environment cycle. It is not
        // collected by dropping the AST or executable, and must remain charged.
        assert!(support.upgrade().is_some());
        assert_eq!(usage(&engine), live);

        let native = engine.imported_native.as_ref().unwrap();
        let identity = native.snapshot().account;
        let mut pressure = native.attempt().unwrap();
        let remaining = rawr_import::cargo::Limits::standard().byte_limit() - native.usage().1;
        crate::host::ImportAllocation::retain_checked(&mut pressure, Some(0), Some(remaining))
            .unwrap();
        // Compiling past the byte target is accepted and recorded on
        // the same runner account; nothing is refused or latched.
        let outcome = engine
            .compile_package_snapshot_resident(&snapshot(source), &cycle)
            .unwrap();
        assert_eq!(native.snapshot().account, identity);
        assert!(native.snapshot().first_refusal.is_none());
        assert!(native.snapshot().exceeded.is_exceeded());
        drop(outcome);
        drop(pressure);
        assert!(support.upgrade().is_some());
        assert_eq!(usage(&engine), live);
    }

    #[test]
    fn escaped_callback_keeps_support_charged_and_observes_later_constants() {
        let (engine, cycle) = retained();
        let baseline = usage(&engine);
        let source = "struct State { value: i64 }\n\
            #[preview] pub fn sample() -> iced::Element<()> {\n\
                let state = State { value: 7 };\n\
                throw \"not invoked\";\n\
            }\n\
            const first = 1;\n\
            fn answer() { global::later }\n\
            let callback = answer;\n\
            const later = 42;\n\
            callback";
        let (candidate, ast) = engine
            .compile_package_snapshot_generation_with_top_level_checked(
                &snapshot(source),
                &cycle,
                &[0; 32],
            )
            .unwrap();
        assert!(!ast.shared_support().is_empty());
        let support = Shared::downgrade(ast.shared_support());
        let callback = engine.eval_ast::<crate::FnPtr>(&ast).unwrap();
        let live = usage(&engine);
        drop(ast);
        drop(candidate);
        assert!(support.upgrade().is_some());
        assert_eq!(usage(&engine), live);
        assert_eq!(
            callback.call::<i64>(&engine, &AST::empty(), ()).unwrap(),
            42
        );
        drop(callback);
        assert!(support.upgrade().is_none());
        assert_eq!(usage(&engine), baseline);
    }

    #[test]
    fn copied_support_functions_keep_one_receipt_after_ast_and_module_merges() {
        let (engine, cycle) = retained();
        let baseline = usage(&engine);
        let source = "struct State { value: i64 }\n\
            #[preview] pub fn sample() -> iced::Element<()> {\n\
                let state = State { value: 7 };\n\
                throw \"not invoked\";\n\
            }";
        let (candidate, ast) = engine
            .compile_package_snapshot_generation_with_top_level_checked(
                &snapshot(source),
                &cycle,
                &[0; 32],
            )
            .unwrap();
        assert!(!ast.shared_support().is_empty());
        let original = Shared::downgrade(ast.shared_support());
        let live = usage(&engine);
        let functions = ast.clone_functions_only();
        let merged = AST::empty().merge(&functions);
        let mut module = crate::Module::new();
        module.merge(merged.shared_support());
        let combined = module.clone();
        module.combine(combined);
        let copied: Vec<_> = module
            .iter_fn()
            .map(|(function, _)| function.clone())
            .collect();
        assert!(!copied.is_empty());
        assert_eq!(usage(&engine), live);
        drop(candidate);
        drop(ast);
        drop(functions);
        drop(merged);
        drop(module);
        assert!(
            original.upgrade().is_none(),
            "the outer support module retired"
        );
        assert_eq!(
            usage(&engine),
            live,
            "copied closures still own their models"
        );
        drop(copied);
        assert_eq!(usage(&engine), baseline);
    }

    #[test]
    fn resolver_cached_module_retains_support_until_its_last_resolved_copy_retires() {
        use crate::ModuleResolver as _;

        let (engine, cycle) = retained();
        let baseline = usage(&engine);
        let source = "struct State { value: i64 }\n\
            #[preview] pub fn sample() -> iced::Element<()> {\n\
                let state = State { value: 7 };\n\
                throw \"not invoked\";\n\
            }\n\
            fn answer() { 42 }";
        let (candidate, ast) = engine
            .compile_package_snapshot_generation_with_top_level_checked(
                &snapshot(source),
                &cycle,
                &[0; 32],
            )
            .unwrap();
        assert!(!ast.shared_support().is_empty());
        let support = Shared::downgrade(ast.shared_support());
        let module = crate::Module::eval_ast_as_new(crate::Scope::new(), &ast, &engine).unwrap();
        let mut resolver = crate::module_resolvers::StaticModuleResolver::new();
        resolver.insert("saved", module);
        let resolved = resolver
            .resolve(&engine, None, "saved", crate::Position::NONE)
            .unwrap();
        let live = usage(&engine);
        drop(candidate);
        drop(ast);
        assert!(support.upgrade().is_some());
        assert_eq!(usage(&engine), live);
        resolver.clear();
        assert!(support.upgrade().is_some());
        assert_eq!(usage(&engine), live);
        drop(resolved);
        assert!(support.upgrade().is_none());
        assert_eq!(usage(&engine), baseline);
    }

    #[test]
    fn empty_support_receipt_retires_with_the_executable() {
        let (engine, cycle) = retained();
        let baseline = usage(&engine);
        let (candidate, ast) = engine
            .compile_package_snapshot_generation_with_top_level_checked(
                &snapshot(
                    "#[preview] pub fn sample() -> iced::Element<()> { throw \"not invoked\" }",
                ),
                &cycle,
                &[0; 32],
            )
            .unwrap();
        assert!(ast.shared_support().is_empty());
        assert!(usage(&engine).1 > baseline.1);
        drop(candidate);
        assert_eq!(usage(&engine), baseline);
        drop(ast);
        assert_eq!(usage(&engine), baseline);
    }
}

/// The runner byte target is accounting. A resident compile past it is
/// accepted and recorded on the existing account, and every owner releases
/// exactly its own contribution.
#[test]
fn resident_target_exceedance_keeps_existing_owners_and_their_account() {
    let (engine, cycle) = retained();
    let native = engine.imported_native.as_ref().unwrap();
    let before = native.usage();
    let identity = native.snapshot().account;
    let mut pressure = native.attempt().unwrap();
    let remaining = rawr_import::cargo::Limits::standard().byte_limit() - native.usage().1;
    pressure.retain_checked(Some(0), Some(remaining)).unwrap();
    let full = native.usage();
    let outcome = engine
        .compile_package_snapshot_resident(
            &snapshot("#[preview] pub fn sample() -> iced::Element<()> { throw \"not invoked\" }"),
            &cycle,
        )
        .unwrap();
    let snapshot = native.snapshot();
    assert_eq!(snapshot.account, identity);
    assert!(snapshot.first_refusal.is_none());
    assert!(snapshot.exceeded.axes().any(|axis| {
        axis.target as usize == snapshot.limits[1] && axis.high_water as usize > full.1
    }));
    drop(outcome);
    assert_eq!(native.usage(), full);
    drop(pressure);
    assert_eq!(native.usage(), before);
}

struct GenerationRoot(std::path::PathBuf);

impl GenerationRoot {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "rawr-resident-coverage-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        let owned = Self(path);
        let harness = owned.0.join("runner-harness");
        std::fs::create_dir(&harness).unwrap();
        // The source-only harness transform uses this existing minimal fixture
        // shape; this test neither resolves dependencies nor builds native code.
        std::fs::write(
            harness.join("Cargo.lock"),
            b"version = 4\n\n[[package]]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        owned
    }
}

impl Drop for GenerationRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn resident_actual_snapshot_matches_checked_generated_inventory_and_misses_changed_type() {
    use crate::package::import::runner;
    use rawr_import::classify::representability::convert;
    use rawr_import::surface::{facade, instance};

    let limits = rawr_import::cargo::Limits::standard();
    let contract = rawr_import::test_fixture::representability_generic()
        .classify(&rawr_import::classify::Policy::standard(), &limits)
        .unwrap();
    let selected = contract
        .outcomes()
        .find(|outcome| outcome.export().path().eq(["family_2"]))
        .unwrap();
    let original = "pub fn main() -> bool { fixture::family_2(7) }\n\
        #[preview] pub fn sample() -> iced::Element<()> { throw \"not invoked\" }";
    let path = "/resident-fixture/main.rs";
    let mut inputs = rawr_import::surface::Snapshot::builder(&limits);
    inputs.entry(path).unwrap();
    inputs.file(path, original.as_bytes()).unwrap();
    let mut demand = contract
        .compiler((), inputs.seal().unwrap(), &limits)
        .unwrap();
    let arguments = instance::Arguments::new(
        Box::from([convert::Type::Builtin(convert::Builtin::I64)]),
        Box::default(),
        Box::default(),
        None,
        &limits,
    )
    .unwrap();
    let start = original.find("fixture::family_2").unwrap() as u32;
    demand
        .require_instance(
            selected.identity(),
            selected.identity().selector().clone(),
            arguments,
            path,
            start,
            start + "fixture::family_2(7)".len() as u32,
        )
        .unwrap();
    let root = GenerationRoot::new();
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let generated = rawr_import::bridge::fixture_generation_from_demand(
        demand.seal().unwrap(),
        rawr_import::profile::Proof::neutral(),
        &root.0,
        supervisor.revision().unwrap(),
    )
    .unwrap()
    .generate(&limits)
    .unwrap();
    let carrier = rawr_import::runner::manifest::Carrier::transform(
        &generated.generated_manifest().carrier().unwrap(),
    )
    .unwrap();
    let mut native = account::Native::transform(carrier).unwrap();
    let plan = facade::Plan::encode(&contract, &limits).unwrap();
    let (mut engine, unused_cycle) = compiler();
    let _definitions = runner::install_retained(&mut engine, &plan, &mut native).unwrap();
    let engine = engine.retain_imported_native(native).unwrap();
    let before = usage(&engine);
    let mut candidates = Vec::new();
    for text in [original.to_owned(), original.replace("(7)", "(8)")] {
        let Outcome::Compiled(compiled) = engine
            .compile_package_snapshot_resident(&snapshot(&text), &unused_cycle)
            .unwrap()
        else {
            panic!("the checked I64 native instance covers both actual snapshots");
        };
        let (candidate, semantic, carrier, coverage) = compiled.into_parts();
        assert!(semantic.is_none());
        assert_eq!(
            carrier,
            *engine
                .imported_native
                .as_ref()
                .unwrap()
                .carrier()
                .identity()
        );
        assert_ne!(coverage, [0; 32]);
        assert_eq!(candidate.source().entry_source().text(), text);
        candidates.push(candidate);
    }
    assert_ne!(candidates[0].id(), candidates[1].id());
    let retained = usage(&engine);
    assert!(retained.0 > before.0 && retained.1 > before.1);
    let changed = original.replace("(7)", "(true)");
    assert!(matches!(
        engine.compile_package_snapshot_resident(&snapshot(&changed), &unused_cycle),
        Ok(Outcome::NeedsPreparation)
    ));
    assert_eq!(usage(&engine), retained);
    drop(candidates);
    assert_eq!(usage(&engine), before);
}
