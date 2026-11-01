use super::{account, Error, Outcome};
use crate::package::compile::captured::tests::{
    compiler, native_snapshot, preview_runtime, snapshot, source,
};
use crate::{Engine, Shared};

fn retained() -> Shared<Engine> {
    let engine = compiler();
    let native = account::Native::transform(account::tests::carrier());
    engine.retain_imported_native(native).unwrap()
}

/// How many scopes keep the engine's decoded native metadata alive.
fn owners(engine: &Engine) -> usize {
    engine.imported_native.as_ref().unwrap().owners()
}

/// Whether `ast`'s support module holds the program's own support
/// functions, not only the generic `Debug` op a debugger reads through.
fn has_own_support(ast: &crate::AST) -> bool {
    ast.shared_support()
        .iter_fn()
        .any(|(_, metadata)| metadata.name.as_str() != crate::resolve::lower_nominal::DEBUG_GENERIC)
}

#[test]
fn resident_compilation_requires_the_engines_retained_native_context() {
    let engine = compiler();
    assert!(matches!(
        engine.compile_package_snapshot_resident_plain(&snapshot(&source("Foo"))),
        Err(Error::Unowned)
    ));
}

#[test]
fn native_miss_and_source_error_release_attempts_without_retry_classification() {
    let engine = retained();
    let before = owners(&engine);
    assert!(matches!(
        engine.compile_package_snapshot_resident_plain(&snapshot(&source("Foo"))),
        Ok(Outcome::NeedsPreparation)
    ));
    assert_eq!(owners(&engine), before);
    let invalid = source("Foo").replace("native::echo(7)", "true");
    assert!(matches!(
        engine.compile_package_snapshot_resident_plain(&snapshot(&invalid)),
        Err(Error::Source(_))
    ));
    assert_eq!(owners(&engine), before);
}

#[test]
fn terminal_native_miss_releases_attempt_and_preserves_source_error_boundary() {
    let engine = retained();
    let before = owners(&engine);
    for source in [
        "use native::new_function; pub fn sample() -> i64 { new_function() }",
        "pub fn sample() -> i64 { native::new_function() }",
        "pub fn sample(value: native::NewType) {}",
    ] {
        assert!(matches!(
            engine.compile_package_snapshot_resident_plain(&native_snapshot(source)),
            Ok(Outcome::NeedsPreparation)
        ));
        assert_eq!(owners(&engine), before);
    }
    assert!(matches!(
        engine.compile_package_snapshot_resident_plain(&native_snapshot(
            "pub fn sample() -> i64 { misspelled() }"
        )),
        Err(Error::Source(_))
    ));
    assert_eq!(owners(&engine), before);
}

#[test]
fn resident_receipt_follows_executable_authority_after_generation_metadata_retires() {
    let engine = retained();
    let before = owners(&engine);
    let pure =
        snapshot("#[preview] pub fn sample() -> iced::Element<()> { panic!(\"not invoked\") }");
    let Outcome::Compiled(compiled) = engine
        .compile_package_snapshot_resident_plain(&pure)
        .unwrap()
    else {
        panic!("no native requirement needs preparation");
    };
    let retained = owners(&engine);
    assert!(retained > before);
    let (candidate, carrier, coverage) = compiled.into_parts();
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
    let bootstrap = std::rc::Rc::new(candidate.bind(&engine, preview_runtime()).unwrap());
    assert_eq!(bootstrap.generation().id(), id);
    let retained_bootstrap = bootstrap.clone();
    drop(bootstrap);
    assert_eq!(owners(&engine), retained);
    drop(retained_bootstrap);
    assert_eq!(owners(&engine), before);
}

#[test]
fn initial_and_ordinary_generations_share_the_resident_account() {
    let engine = retained();
    let baseline = owners(&engine);
    let pure =
        snapshot("#[preview] pub fn sample() -> iced::Element<()> { panic!(\"not invoked\") }");
    let (initial, ast) = engine
        .compile_package_snapshot_generation_with_top_level(&pure)
        .unwrap();
    let live = owners(&engine);
    assert!(live > baseline);
    let ordinary = engine
        .compile_package_snapshot_generation_with_top_level(&pure)
        .unwrap();
    let overlapping = owners(&engine);
    assert!(overlapping > live);
    drop(ordinary);
    assert_eq!(owners(&engine), live);
    drop(ast);
    drop(initial);
    assert_eq!(owners(&engine), baseline);
}

mod top_level {
    use super::{
        account, compiler, has_own_support, owners, preview_runtime, retained, snapshot, Outcome,
    };
    use crate::{Dynamic, Engine, Shared, AST};

    // The generated helper only needs these two Program fields and `locate`. Include its
    // emitted source unchanged so the lifetime regression exercises shipping code.
    struct Program {
        engine: Shared<Engine>,
        ast: Option<AST>,
    }

    // No debugger reads these programs.
    const fn locate(_: &Program) {}

    include!("../../../../importer/src/bridge/generate/top_level.rs");

    #[test]
    fn neutral_snapshot_retains_support_until_its_last_ast_copy_retires() {
        let engine = retained();
        let baseline = owners(&engine);
        let ast = engine
            .compile_package_snapshot(&snapshot(
                "struct State { value: i64 }\n\
             #[preview] pub fn sample() -> iced::Element<()> {\n\
                 let state = State { value: 7 };\n\
                 panic!(\"not invoked\");\n\
             }",
            ))
            .unwrap();
        assert!(has_own_support(&ast));
        let support = Shared::downgrade(ast.shared_support());
        let copy = ast.clone_functions_only();
        let live = owners(&engine);
        assert!(live > baseline);
        let mut program = Program {
            engine: engine.clone(),
            ast: Some(ast),
        };
        evaluate_program(&mut program).unwrap();
        assert!(support.upgrade().is_some());
        assert_eq!(owners(&engine), live);
        drop(copy);
        assert!(support.upgrade().is_none());
        assert_eq!(owners(&engine), baseline);
    }

    #[test]
    fn neutral_reconstruction_keeps_prior_support_alive() {
        let engine = retained();
        let source = snapshot(
            "struct State { value: i64 }\n\
             #[preview] pub fn sample() -> iced::Element<()> {\n\
                 let state = State { value: 7 };\n\
                 panic!(\"not invoked\");\n\
             }\n\
             fn answer() -> i64 { 42 }",
        );
        let ast = engine.compile_package_snapshot(&source).unwrap();
        let first_support = Shared::downgrade(ast.shared_support());
        assert!(has_own_support(&ast));
        // A live AST copy keeps the first generation's support alive across
        // the reconstruction and the retirement of its engine.
        let first_copy = ast.clone_functions_only();
        let mut program = Program {
            engine: engine.clone(),
            ast: Some(ast),
        };
        evaluate_program(&mut program).unwrap();
        let native = engine
            .reconstruct_imported_native(&account::tests::carrier_bytes(&[]))
            .unwrap();
        let replacement = compiler();
        let replacement = replacement.retain_imported_native(native).unwrap();
        drop(program);
        drop(engine);
        assert!(first_support.upgrade().is_some());
        let reconstructed = owners(&replacement);

        let ast = replacement.compile_package_snapshot(&source).unwrap();
        let second_support = Shared::downgrade(ast.shared_support());
        let second_copy = ast.clone_functions_only();
        let mut program = Program {
            engine: replacement.clone(),
            ast: Some(ast),
        };
        evaluate_program(&mut program).unwrap();
        drop(program);
        assert!(owners(&replacement) > reconstructed);
        assert!(first_support.upgrade().is_some());
        assert!(second_support.upgrade().is_some());
        drop(first_copy);
        drop(second_copy);
        assert!(first_support.upgrade().is_none());
        assert!(second_support.upgrade().is_none());
    }

    #[test]
    fn generated_top_level_retires_initial_support_with_its_last_executable() {
        let engine = retained();
        let baseline = owners(&engine);
        let source = "struct State { value: i64 }\n\
            #[preview] pub fn sample() -> iced::Element<()> {\n\
                let state = State { value: 7 };\n\
                panic!(\"not invoked\");\n\
            }";
        let (initial, ast) = engine
            .compile_package_snapshot_generation_with_top_level(&snapshot(source))
            .unwrap();
        assert!(has_own_support(&ast));
        let support = Shared::downgrade(ast.shared_support());
        let initial_owners = owners(&engine);
        assert!(initial_owners > baseline);
        let initial = std::rc::Rc::new(initial.bind(&engine, preview_runtime()).unwrap());
        let last_good = initial.clone();
        let mut program = Program {
            engine: engine.clone(),
            ast: Some(ast),
        };
        evaluate_program(&mut program).unwrap();
        assert!(program.ast.is_none());
        assert!(support.upgrade().is_some());
        assert_eq!(owners(&engine), initial_owners);

        let Outcome::Compiled(resident) = engine
            .compile_package_snapshot_resident_plain(&snapshot(&source.replace("7", "8")))
            .unwrap()
        else {
            panic!("the resident edit has no new native requirement");
        };
        let overlapping = owners(&engine);
        assert!(overlapping > initial_owners);
        drop(initial);
        assert!(support.upgrade().is_some());
        assert_eq!(owners(&engine), overlapping);
        drop(last_good);
        assert!(support.upgrade().is_none());
        assert_eq!(owners(&engine), overlapping - (initial_owners - baseline));
        drop(resident);
        assert_eq!(owners(&engine), baseline);
        // The original runner Program remains live after resident replacement.
        assert!(program.ast.is_none());
        assert!(evaluate_program(&mut program)
            .unwrap_err()
            .to_string()
            .contains("already evaluated"));
        assert_eq!(owners(&engine), baseline);
    }

    #[test]
    fn generated_top_level_discards_acyclic_function_environments_on_success_and_error() {
        for (source, succeeds) in [
            (
                "fn answer() -> i64 { 42 } let callback = answer; callback",
                true,
            ),
            (
                "fn answer() -> i64 { 42 } let callback = answer; panic!(\"{}\", callback());",
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
    fn escaped_callback_keeps_support_charged_and_observes_later_constants() {
        let engine = retained();
        let baseline = owners(&engine);
        let source = "struct State { value: i64 }\n\
            #[preview] pub fn sample() -> iced::Element<()> {\n\
                let state = State { value: 7 };\n\
                panic!(\"not invoked\");\n\
            }\n\
            const FIRST: i64 = 1;\n\
            fn answer() -> i64 { LATER }\n\
            let callback = answer;\n\
            const LATER: i64 = 42;\n\
            callback";
        let (candidate, ast) = engine
            .compile_package_snapshot_generation_with_top_level(&snapshot(source))
            .unwrap();
        assert!(has_own_support(&ast));
        let support = Shared::downgrade(ast.shared_support());
        let callback = engine.eval_ast::<crate::FnPtr>(&ast).unwrap();
        let live = owners(&engine);
        drop(ast);
        drop(candidate);
        assert!(support.upgrade().is_some());
        assert_eq!(owners(&engine), live);
        assert_eq!(
            callback.call::<i64>(&engine, &AST::empty(), ()).unwrap(),
            42
        );
        drop(callback);
        assert!(support.upgrade().is_none());
        assert_eq!(owners(&engine), baseline);
    }

    #[test]
    fn copied_support_functions_keep_one_receipt_after_ast_and_module_merges() {
        let engine = retained();
        let baseline = owners(&engine);
        let source = "struct State { value: i64 }\n\
            #[preview] pub fn sample() -> iced::Element<()> {\n\
                let state = State { value: 7 };\n\
                panic!(\"not invoked\");\n\
            }";
        let (candidate, ast) = engine
            .compile_package_snapshot_generation_with_top_level(&snapshot(source))
            .unwrap();
        assert!(has_own_support(&ast));
        let original = Shared::downgrade(ast.shared_support());
        let live = owners(&engine);
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
        assert_eq!(owners(&engine), live);
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
            owners(&engine),
            live,
            "copied closures still own their models"
        );
        drop(copied);
        assert_eq!(owners(&engine), baseline);
    }

    #[test]
    fn resolver_cached_module_retains_support_until_its_last_resolved_copy_retires() {
        use crate::ModuleResolver as _;

        let engine = retained();
        let baseline = owners(&engine);
        let source = "struct State { value: i64 }\n\
            #[preview] pub fn sample() -> iced::Element<()> {\n\
                let state = State { value: 7 };\n\
                panic!(\"not invoked\");\n\
            }\n\
            fn answer() -> i64 { 42 }";
        let (candidate, ast) = engine
            .compile_package_snapshot_generation_with_top_level(&snapshot(source))
            .unwrap();
        assert!(has_own_support(&ast));
        let support = Shared::downgrade(ast.shared_support());
        let module = crate::Module::eval_ast_as_new(crate::Scope::new(), &ast, &engine).unwrap();
        let mut resolver = crate::module_resolvers::StaticModuleResolver::new();
        resolver.insert("saved", module);
        let resolved = resolver
            .resolve(&engine, None, "saved", crate::Position::NONE)
            .unwrap();
        let live = owners(&engine);
        drop(candidate);
        drop(ast);
        assert!(support.upgrade().is_some());
        assert_eq!(owners(&engine), live);
        resolver.clear();
        assert!(support.upgrade().is_some());
        assert_eq!(owners(&engine), live);
        drop(resolved);
        assert!(support.upgrade().is_none());
        assert_eq!(owners(&engine), baseline);
    }

    #[test]
    fn empty_support_receipt_retires_with_the_executable() {
        let engine = retained();
        let baseline = owners(&engine);
        let (candidate, ast) = engine
            .compile_package_snapshot_generation_with_top_level(&snapshot(
                "#[preview] pub fn sample() -> iced::Element<()> { panic!(\"not invoked\") }",
            ))
            .unwrap();
        assert!(ast.shared_support().is_empty());
        assert!(owners(&engine) > baseline);
        drop(candidate);
        assert_eq!(owners(&engine), baseline);
        drop(ast);
        assert_eq!(owners(&engine), baseline);
    }
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
        #[preview] pub fn sample() -> iced::Element<()> { panic!(\"not invoked\") }";
    let path = "/resident-fixture/main.rs";
    let mut inputs = rawr_import::surface::Snapshot::builder(&limits);
    inputs.entry(path).unwrap();
    inputs.file(path, original.as_bytes()).unwrap();
    let mut demand = contract
        .compiler((), inputs.seal().unwrap(), &limits)
        .unwrap();
    // An unsuffixed literal is an `i32`, as in Rust.
    let arguments = instance::Arguments::new(
        Box::from([convert::Type::Builtin(convert::Builtin::I32)]),
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
    let mut native = account::Native::transform(carrier);
    let plan = facade::Plan::encode(&contract).unwrap();
    let mut engine = compiler();
    let _definitions = runner::install_retained(&mut engine, &plan, &mut native).unwrap();
    let engine = engine.retain_imported_native(native).unwrap();
    let before = owners(&engine);
    let mut candidates = Vec::new();
    for text in [original.to_owned(), original.replace("(7)", "(8)")] {
        let Outcome::Compiled(compiled) = engine
            .compile_package_snapshot_resident_plain(&snapshot(&text))
            .unwrap()
        else {
            panic!("the checked I32 native instance covers both actual snapshots");
        };
        let (candidate, carrier, coverage) = compiled.into_parts();
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
    let retained = owners(&engine);
    assert!(retained > before);
    let changed = original.replace("(7)", "(true)");
    assert!(matches!(
        engine.compile_package_snapshot_resident_plain(&snapshot(&changed)),
        Ok(Outcome::NeedsPreparation)
    ));
    assert_eq!(owners(&engine), retained);
    drop(candidates);
    assert_eq!(owners(&engine), before);
}

#[test]
fn plain_resident_compilation_takes_a_main_with_no_application_semantics() {
    let engine = retained();
    let before = owners(&engine);
    let Outcome::Compiled(compiled) = engine
        .compile_package_snapshot_resident_plain(&snapshot("fn main() { let value = 1 + 2; }"))
        .unwrap()
    else {
        panic!("a program with no native requirement needs no preparation");
    };
    let (compiled, top_level) = compiled.with_top_level();
    let (candidate, ..) = compiled.into_parts();
    drop((candidate, top_level));
    assert_eq!(owners(&engine), before);
    // The same boundaries as a profile's: a native miss needs preparation,
    // and a source error is no miss.
    assert!(matches!(
        engine.compile_package_snapshot_resident_plain(&native_snapshot(
            "pub fn sample() -> i64 { native::new_function() }"
        )),
        Ok(Outcome::NeedsPreparation)
    ));
    assert!(matches!(
        engine.compile_package_snapshot_resident_plain(&native_snapshot(
            "pub fn sample() -> i64 { misspelled() }"
        )),
        Err(Error::Source(_))
    ));
    assert_eq!(owners(&engine), before);
}
