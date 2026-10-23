use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rawr::reload::compatibility::{Action, Owner, Owners, Plan, Reason};
use rawr::{project, Engine, Shared};

static NEXT_PROJECT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn candidate_retains_one_exact_generation_proof_bundle() {
    let project = Project::new("proof-bundle");
    project.write("Cargo.toml", "[package]\nname = \"reload-app\"\n");
    project.write("src/main.rs", &format!("mod palette;\n{}", source("self.value += 1")));
    project.write("src/palette.rs", "");
    let engine = compiler();
    let first = compile(&engine, &project);

    assert!(first.has_application());
    assert_eq!(first.previews().len(), 1);
    assert_eq!(first.schemas().len(), 2);
    assert!(first.callbacks().application().is_some());
    assert!(!first.callbacks().items().is_empty());
    assert!(first.callbacks().widgets().is_empty());
    assert_eq!(first.source().packages().len(), 1);
    assert_eq!(first.source().sources().len(), 2);
    // The package is named as rustc names the crate (D471).
    assert_eq!(first.source().entry_package().name(), "reload_app");
    assert_eq!(
        first.source().entry_source().text(),
        format!("// rawr\nmod palette;\n{}", source("self.value += 1"))
    );

    project.write("src/main.rs", &format!("mod palette;\n{}", source("self.value += 2")));
    let second = compile(&engine, &project);

    assert_ne!(first.id(), second.id());
    assert_eq!(first.schemas(), second.schemas());
    assert_eq!(first.callbacks(), second.callbacks());
    assert_eq!(
        second.source().entry_source().text(),
        format!("// rawr\nmod palette;\n{}", source("self.value += 2"))
    );
}

#[test]
fn compatibility_plan_owns_the_candidate_without_mutating_current() {
    let project = Project::new("atomic-current");
    let first_source = source("self.value += 1").replace("    #[preview]\n", "");
    project.write("src/main.rs", &first_source);
    let engine = compiler();
    let first = compile(&engine, &project).generation().clone();
    assert!(first.previews().is_empty());

    let snapshot = first.clone();

    let second_source = source("self.value += 2").replace("    #[preview]\n", "");
    project.write("src/main.rs", &second_source);
    let second = compile(&engine, &project);
    let second_id = second.id();
    let second_text = second.source().entry_source().text().to_owned();
    let plan = compare(first.clone(), second, Owner::Application);

    assert_eq!(plan.evidence_set()[0].action(), Action::Retain);
    assert!(Arc::ptr_eq(&snapshot, &first));
    assert_eq!(snapshot.source().entry_source().text(), format!("// rawr\n{first_source}"));
    assert_eq!(plan.evidence_set()[0].current(), first.id());
    assert_eq!(plan.evidence_set()[0].candidate(), second_id);
    assert_eq!(second_text, format!("// rawr\n{second_source}"));
}

#[test]
fn body_and_formatting_only_changes_retain_application_state() {
    let project = Project::new("body-formatting");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let first = compile(&engine, &project).generation().clone();

    project.write("src/main.rs", SOURCE_TWO);
    let body = compare(
        first.clone(),
        compile(&engine, &project),
        Owner::Application,
    );
    assert_eq!(body.evidence_set()[0].action(), Action::Retain);
    assert!(body.evidence_set()[0].reasons().is_empty());

    let formatted = SOURCE_ONE
        .replace(
            "struct Counter { value: i64 }",
            "struct Counter {\n    value: i64,\n}",
        )
        .replace(
            "enum Message { Increment }",
            "enum Message {\n    Increment,\n}",
        );
    project.write("src/main.rs", &formatted);
    let formatted = compare(first, compile(&engine, &project), Owner::Application);
    assert_eq!(formatted.evidence_set()[0].action(), Action::Retain);
    assert!(formatted.evidence_set()[0].reasons().is_empty());
}

#[test]
fn reachable_nested_schema_and_callback_drift_reset_state() {
    let project = Project::new("nested-schema");
    project.write("src/main.rs", &nested_source("i64"));
    let engine = compiler();
    let first = compile(&engine, &project).generation().clone();

    project.write("src/main.rs", &nested_source("bool"));
    let schema = compare(first, compile(&engine, &project), Owner::Application);
    assert_eq!(schema.evidence_set()[0].action(), Action::Reset);
    assert!(schema.evidence_set()[0].reasons().contains(&Reason::Schema));

    project.write("src/main.rs", &callback_source(false));
    let first = compile(&engine, &project).generation().clone();
    project.write("src/main.rs", &callback_source(true));
    let callback = compare(first, compile(&engine, &project), Owner::Application);
    assert_eq!(callback.evidence_set()[0].action(), Action::Reset);
    assert!(callback.evidence_set()[0]
        .reasons()
        .contains(&Reason::Callback));

    project.write("src/main.rs", &message_source(false));
    let first = compile(&engine, &project).generation().clone();
    project.write("src/main.rs", &message_source(true));
    let message = compare(first, compile(&engine, &project), Owner::Application);
    assert_eq!(message.evidence_set()[0].action(), Action::Reset);
    assert!(message.evidence_set()[0]
        .reasons()
        .contains(&Reason::Schema));
    assert!(message.evidence_set()[0]
        .reasons()
        .contains(&Reason::Callback));
}

#[test]
fn unrelated_package_and_preview_changes_keep_the_selected_owner() {
    let project = Project::new("owner-separation");
    project.write("src/main.rs", &format!("mod extra;\n{}", preview_source("i64")));
    project.write("src/extra.rs", "pub struct Extra { value: i64 }");
    let engine = compiler();
    let first = compile(&engine, &project).generation().clone();
    let selected = first
        .previews()
        .iter()
        .find(|preview| preview.name() == "First")
        .expect("fixture retains the selected preview")
        .key()
        .clone();
    let other = first
        .previews()
        .iter()
        .find(|preview| preview.name() == "Second")
        .expect("fixture retains the unrelated preview")
        .key()
        .clone();

    project.write("src/extra.rs", "pub struct Extra { value: bool }");
    project.write("src/main.rs", &format!("mod extra;\n{}", preview_source("bool")));
    let candidate = compile(&engine, &project);
    let application = compare(first.clone(), candidate, Owner::Application);
    assert_eq!(application.evidence_set()[0].action(), Action::Retain);

    let candidate = compile(&engine, &project);
    let selected = compare(first.clone(), candidate, Owner::Preview(selected));
    assert_eq!(selected.evidence_set()[0].action(), Action::Retain);

    let candidate = compile(&engine, &project);
    let other = compare(first, candidate, Owner::Preview(other));
    assert_eq!(other.evidence_set()[0].action(), Action::Reset);
    assert!(other.evidence_set()[0].reasons().contains(&Reason::Schema));
}

#[test]
fn widget_protocol_and_opaque_content_reset_conservatively() {
    let project = Project::new("widget-opaque");
    let engine = compiler();
    project.write("src/main.rs", &widget_source("i64", "0"));
    let first = compile(&engine, &project).generation().clone();
    let widget_body = widget_source("i64", "0").replace("throw \"size\"", "throw \"changed\"");
    project.write("src/main.rs", &widget_body);
    let compatible = compare(
        first.clone(),
        compile(&engine, &project),
        Owner::Application,
    );
    assert_eq!(compatible.evidence_set()[0].action(), Action::Retain);

    project.write("src/main.rs", &widget_source("bool", "false"));
    let widget = compare(first, compile(&engine, &project), Owner::Application);
    assert_eq!(widget.evidence_set()[0].action(), Action::Reset);
    assert!(widget.evidence_set()[0].reasons().contains(&Reason::Widget));

    project.write("src/main.rs", &opaque_source("i64"));
    let first = compile(&engine, &project).generation().clone();
    let task_body = opaque_source("i64").replace("button(\"Tick\")", "button(\"Changed\")");
    project.write("src/main.rs", &task_body);
    let compatible = compare(
        first.clone(),
        compile(&engine, &project),
        Owner::Application,
    );
    assert_eq!(compatible.evidence_set()[0].action(), Action::Retain);
    assert!(compatible.evidence_set()[0].is_opaque());

    project.write("src/main.rs", &opaque_source("bool"));
    let opaque = compare(first, compile(&engine, &project), Owner::Application);
    assert_eq!(opaque.evidence_set()[0].action(), Action::Reset);
    assert!(opaque.evidence_set()[0].is_opaque());
    assert!(opaque.evidence_set()[0].reasons().contains(&Reason::Schema));
}

#[test]
fn owner_sets_reject_invalid_scope_and_order_multi_owner_evidence() {
    assert_eq!(
        Owners::new(std::iter::empty()).unwrap_err(),
        rawr::reload::compatibility::Error::Empty
    );
    assert_eq!(
        Owners::new([Owner::Application, Owner::Application]).unwrap_err(),
        rawr::reload::compatibility::Error::Duplicate
    );

    let project = Project::new("owner-set");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let current = compile(&engine, &project).generation().clone();
    let preview = current.previews()[0].key().clone();
    project.write("src/main.rs", SOURCE_TWO);
    let owners = Owners::new([Owner::Preview(preview.clone()), Owner::Application]).unwrap();
    assert_eq!(
        owners.iter().cloned().collect::<Vec<_>>(),
        vec![Owner::Application, Owner::Preview(preview.clone())]
    );

    let plan = Plan::compare(current, compile(&engine, &project), owners);
    assert_eq!(
        plan.evidence_set()
            .iter()
            .map(|evidence| (evidence.owner().clone(), evidence.action()))
            .collect::<Vec<_>>(),
        vec![
            (Owner::Application, Action::Retain),
            (Owner::Preview(preview), Action::Retain),
        ]
    );
}

#[test]
fn binding_rejects_wrong_engine_origin_and_missing_runtime_facades() {
    let project = Project::new("binding-rejection");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let wrong = compiler();
    let candidate = compile(&engine, &project);
    assert!(matches!(
        candidate.bind(&wrong, registry()),
        Err(rawr::reload::installation::Error::Origin(_))
    ));

    let candidate = compile(&engine, &project);
    assert!(matches!(
        candidate.bind(&engine, rawr::host::runtime::Registry::new()),
        Err(rawr::reload::installation::Error::Runtime(_))
    ));

    let bootstrap = compile(&engine, &project)
        .bind(&engine, registry())
        .unwrap();
    let debug = format!("{bootstrap:?}");
    assert!(debug.starts_with("Bootstrap"));
    assert!(!debug.contains("BoundCatalog"));
}

#[test]
fn staged_inputs_cover_candidate_additions_and_withdraw_on_drop() {
    let project = Project::new("staged-input-union");
    project.write("Cargo.toml", "[package]\nname = \"staged-inputs\"\n");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let first = compile(&engine, &project);
    let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
        rawr::reload::source::Inputs::from_graph(first.source()),
    ));

    project.write("src/main.rs", &format!("mod extra;\n{SOURCE_ONE}"));
    project.write("src/extra.rs", "pub struct Added;");
    let candidate = compile(&engine, &project);
    let added = fs::canonicalize(project.path().join("src/extra.rs")).unwrap();
    let (_gate, candidate) = scheduled((
        (),
        rawr::reload::source::Snapshot::from_graph(candidate.source()),
    ));
    let candidate = candidate.stage_inputs(&publication);
    let stage = candidate.value().1.inputs();

    assert!(!publication.read().current().contains(&added));
    assert!(publication.read().contains(&added));
    assert_eq!(publication.read().staged(), 1);
    assert!(stage.contains(&added));
    drop(candidate);
    assert!(!publication.read().contains(&added));
    assert_eq!(publication.read().staged(), 0);
}

#[test]
fn staged_snapshot_revalidation_rejects_source_and_manifest_drift() {
    let project = Project::new("snapshot-revalidation");
    project.write("Cargo.toml", "[package]\nname = \"snapshot\"\n");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let candidate = compile(&engine, &project);
    let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
        rawr::reload::source::Inputs::from_graph(candidate.source()),
    ));
    let source = fs::canonicalize(project.path().join("src/main.rs")).unwrap();
    let manifest = fs::canonicalize(project.path().join("Cargo.toml")).unwrap();

    let (_gate, source_stage) = scheduled((
        (),
        rawr::reload::source::Snapshot::from_graph(candidate.source()),
    ));
    let source_stage = source_stage.stage_inputs(&publication);
    project.write("src/main.rs", SOURCE_TWO);
    assert!(matches!(
        source_stage.try_map(|(_, stage)| stage.revalidate()),
        Err(rawr::reload::source::Error::Changed { path }) if path == source
    ));
    assert_eq!(publication.read().staged(), 0);

    project.write("src/main.rs", SOURCE_ONE);
    let (_gate, manifest_stage) = scheduled((
        (),
        rawr::reload::source::Snapshot::from_graph(candidate.source()),
    ));
    let manifest_stage = manifest_stage.stage_inputs(&publication);
    project.write("Cargo.toml", "[package]\nname = \"snapshot-renamed\"\n");
    assert!(matches!(
        manifest_stage.try_map(|(_, stage)| stage.revalidate()),
        Err(rawr::reload::source::Error::Changed { path }) if path == manifest
    ));
    assert_eq!(publication.read().staged(), 0);
}

#[test]
fn gate_admits_host_and_exact_inputs_as_one_structural_replacement() {
    let project = Project::new("dual-admission");
    project.write("Cargo.toml", "[package]\nname = \"dual\"\n");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let first = compile(&engine, &project);
    let initial_inputs = rawr::reload::source::Inputs::from_graph(first.source());
    let bootstrap = first.bind(&engine, registry()).unwrap();
    let mut slot = bootstrap.install(1_u8);
    let retained = slot.generation();
    let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
        initial_inputs,
    ));

    project.write("src/extra.rs", "pub struct Added;");
    project.write("src/main.rs", &format!("mod extra;\n{SOURCE_TWO}"));
    let candidate = compile(&engine, &project);
    let added = fs::canonicalize(project.path().join("src/extra.rs")).unwrap();
    let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
    let prepared = Plan::compare(retained.clone(), candidate, Owners::one(Owner::Application))
        .bind(&engine, registry())
        .unwrap()
        .prepare(&slot)
        .unwrap()
        .seal(2_u8);
    let (gate, candidate) = scheduled((prepared, snapshot));
    let candidate = candidate.stage_inputs(&publication);
    let candidate = candidate
        .try_map(|(prepared, staged)| staged.revalidate().map(|validated| (prepared, validated)))
        .unwrap();

    let rawr::reload::scheduler::Admission::Current(Ok((_evidence, retired_host, retired_inputs))) =
        gate.admit_slot_and_publish(candidate, &mut slot, &publication)
    else {
        panic!("fresh host and input proofs must admit together");
    };

    assert_eq!(*slot.state(), 2);
    assert!(!Arc::ptr_eq(&slot.generation(), &retained));
    assert!(publication.read().current().contains(&added));
    assert!(!retired_inputs.previous().contains(&added));
    assert_eq!(retired_host.release(), 1);
    retired_inputs.release();
}

#[test]
fn cross_publication_proof_rejects_without_changing_host_or_inputs() {
    let project = Project::new("cross-publication");
    project.write("Cargo.toml", "[package]\nname = \"cross-publication\"\n");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let first = compile(&engine, &project);
    let inputs = rawr::reload::source::Inputs::from_graph(first.source());
    let bootstrap = first.bind(&engine, registry()).unwrap();
    let mut slot = bootstrap.install(1_u8);
    let retained = slot.generation();
    let source_publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
        inputs.clone(),
    ));
    let foreign_publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(inputs));

    project.write("src/main.rs", SOURCE_TWO);
    let candidate = compile(&engine, &project);
    let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
    let prepared = Plan::compare(retained.clone(), candidate, Owners::one(Owner::Application))
        .bind(&engine, registry())
        .unwrap()
        .prepare(&slot)
        .unwrap()
        .seal(2_u8);
    let (gate, candidate) = scheduled((prepared, snapshot));
    let candidate = candidate.stage_inputs(&source_publication);
    let candidate = candidate
        .try_map(|(prepared, staged)| staged.revalidate().map(|validated| (prepared, validated)))
        .unwrap();

    let rawr::reload::scheduler::Admission::Current(Err(rejected)) =
        gate.admit_slot_and_publish(candidate, &mut slot, &foreign_publication)
    else {
        panic!("a publication proof from another slot must reject");
    };

    assert_eq!(*slot.state(), 1);
    assert!(Arc::ptr_eq(&slot.generation(), &retained));
    assert_eq!(source_publication.read().staged(), 1);
    assert_eq!(foreign_publication.read().staged(), 0);
    assert_eq!(rejected.release(), 2);
    assert_eq!(source_publication.read().staged(), 0);
}

#[test]
fn obsolete_dual_candidate_withdraws_staged_inputs_after_gate_return() {
    let project = Project::new("obsolete-publication");
    project.write("Cargo.toml", "[package]\nname = \"obsolete-publication\"\n");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let first = compile(&engine, &project);
    let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
        rawr::reload::source::Inputs::from_graph(first.source()),
    ));
    let bootstrap = first.bind(&engine, registry()).unwrap();
    let mut slot = bootstrap.install(1_u8);
    let retained = slot.generation();

    project.write("src/main.rs", SOURCE_TWO);
    let candidate = compile(&engine, &project);
    let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
    let prepared = Plan::compare(retained.clone(), candidate, Owners::one(Owner::Application))
        .bind(&engine, registry())
        .unwrap()
        .prepare(&slot)
        .unwrap()
        .seal(2_u8);
    let (gate, candidate) = scheduled((prepared, snapshot));
    let candidate = candidate.stage_inputs(&publication);
    let candidate = candidate
        .try_map(|(prepared, staged)| staged.revalidate().map(|validated| (prepared, validated)))
        .unwrap();
    gate.observe(std::time::Instant::now()).unwrap();

    let admission = gate.admit_slot_and_publish(candidate, &mut slot, &publication);
    assert!(matches!(
        admission,
        rawr::reload::scheduler::Admission::Obsolete(_)
    ));
    assert_eq!(publication.read().staged(), 1);
    assert_eq!(*slot.state(), 1);
    assert!(Arc::ptr_eq(&slot.generation(), &retained));
    drop(admission);
    assert_eq!(publication.read().staged(), 0);
}

#[test]
fn slot_rejects_the_second_preparation_from_one_live_epoch() {
    let project = Project::new("slot-double-preparation");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let bootstrap = compile(&engine, &project)
        .bind(&engine, registry())
        .unwrap();
    let mut slot = bootstrap.install(0_u8);
    let current = slot.generation();

    project.write("src/main.rs", SOURCE_TWO);
    let first = Plan::compare(
        current.clone(),
        compile(&engine, &project),
        Owners::one(Owner::Application),
    )
    .bind(&engine, registry())
    .unwrap()
    .prepare(&slot)
    .unwrap()
    .seal(1_u8);
    let second = Plan::compare(
        current,
        compile(&engine, &project),
        Owners::one(Owner::Application),
    )
    .bind(&engine, registry())
    .unwrap()
    .prepare(&slot)
    .unwrap()
    .seal(2_u8);

    let (gate, candidate) = scheduled(first);
    let rawr::reload::scheduler::Admission::Current(Ok((_evidence, retired))) =
        gate.admit_slot(candidate, &mut slot)
    else {
        panic!("first preparation must remain current");
    };
    assert_eq!(retired.release(), 0);
    let (gate, candidate) = scheduled(second);
    let rawr::reload::scheduler::Admission::Current(Err(rejected)) =
        gate.admit_slot(candidate, &mut slot)
    else {
        panic!("second preparation must fail the rotated slot epoch");
    };
    assert_eq!(rejected.release(), 2);
    assert_eq!(*slot.state(), 1);
}

#[test]
fn retired_state_drop_panics_only_after_the_slot_is_current() {
    struct Bomb {
        explode: bool,
        drops: Arc<AtomicUsize>,
    }

    impl Drop for Bomb {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
            if self.explode {
                panic!("retired slot state drop bomb");
            }
        }
    }

    let project = Project::new("slot-retired-drop");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let drops = Arc::new(AtomicUsize::new(0));
    let bootstrap = compile(&engine, &project)
        .bind(&engine, registry())
        .unwrap();
    let mut slot = bootstrap.install(Bomb {
        explode: true,
        drops: drops.clone(),
    });
    let retained = slot.generation();

    project.write("src/main.rs", SOURCE_TWO);
    let prepared = Plan::compare(
        retained.clone(),
        compile(&engine, &project),
        Owners::one(Owner::Application),
    )
    .bind(&engine, registry())
    .unwrap()
    .prepare(&slot)
    .unwrap()
    .seal(Bomb {
        explode: false,
        drops: drops.clone(),
    });

    let (gate, candidate) = scheduled(prepared);
    let rawr::reload::scheduler::Admission::Current(Ok((_evidence, retired))) =
        gate.admit_slot(candidate, &mut slot)
    else {
        panic!("fresh prepared slot must remain current");
    };
    let committed = slot.generation();
    assert!(!Arc::ptr_eq(&retained, &committed));
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(retired.release());
    }));
    assert!(panic.is_err());
    assert!(Arc::ptr_eq(&slot.generation(), &committed));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn rejected_state_drop_cannot_change_the_live_slot() {
    struct Bomb(Arc<AtomicUsize>);

    impl Drop for Bomb {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("rejected slot state drop bomb");
        }
    }

    let project = Project::new("slot-rejected-drop");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let bootstrap = compile(&engine, &project)
        .bind(&engine, registry())
        .unwrap();
    let mut slot = bootstrap.install(None::<Bomb>);
    let retained = slot.generation();
    let drops = Arc::new(AtomicUsize::new(0));

    project.write("src/main.rs", SOURCE_TWO);
    let prepared = Plan::compare(
        retained.clone(),
        compile(&engine, &project),
        Owners::one(Owner::Application),
    )
    .bind(&engine, registry())
    .unwrap()
    .prepare(&slot)
    .unwrap()
    .seal(Some(Bomb(drops.clone())));
    {
        let _mutation = slot.mutation();
    }

    let (gate, candidate) = scheduled(prepared);
    let rawr::reload::scheduler::Admission::Current(Err(rejected)) =
        gate.admit_slot(candidate, &mut slot)
    else {
        panic!("current stale slot proof must return its rejected bundle");
    };
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(rejected.release());
    }));
    assert!(panic.is_err());
    assert!(Arc::ptr_eq(&slot.generation(), &retained));
    assert!(slot.state().is_none());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn obsolete_state_drop_runs_only_after_the_revision_gate_returns() {
    struct Bomb(Arc<AtomicUsize>);

    impl Drop for Bomb {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("obsolete slot state drop bomb");
        }
    }

    let project = Project::new("slot-obsolete-drop");
    project.write("src/main.rs", SOURCE_ONE);
    let engine = compiler();
    let bootstrap = compile(&engine, &project)
        .bind(&engine, registry())
        .unwrap();
    let mut slot = bootstrap.install(None::<Bomb>);
    let retained = slot.generation();
    let drops = Arc::new(AtomicUsize::new(0));

    project.write("src/main.rs", SOURCE_TWO);
    let prepared = Plan::compare(
        retained.clone(),
        compile(&engine, &project),
        Owners::one(Owner::Application),
    )
    .bind(&engine, registry())
    .unwrap()
    .prepare(&slot)
    .unwrap()
    .seal(Some(Bomb(drops.clone())));
    let (gate, candidate) = scheduled(prepared);
    gate.observe(std::time::Instant::now()).unwrap();

    let admission = gate.admit_slot(candidate, &mut slot);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert!(matches!(
        admission,
        rawr::reload::scheduler::Admission::Obsolete(_)
    ));
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(admission)));
    assert!(panic.is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(Arc::ptr_eq(&slot.generation(), &retained));
}

fn compiler() -> Shared<Engine> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    Shared::new(engine)
}

fn registry() -> rawr::host::runtime::Registry {
    let mut registry = rawr::host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry
}

fn compile(engine: &Shared<Engine>, project: &Project) -> rawr::reload::Candidate {
    let target = project::Target::discover(project.path()).unwrap();
    engine.compile_package_generation(&target).unwrap()
}

fn compare(
    current: Arc<rawr::reload::Generation>,
    candidate: rawr::reload::Candidate,
    owner: Owner,
) -> Plan {
    Plan::compare(current, candidate, Owners::one(owner))
}

fn scheduled<T>(
    value: T,
) -> (
    rawr::reload::scheduler::Gate,
    rawr::reload::scheduler::Candidate<T>,
) {
    let now = std::time::Instant::now();
    let gate = rawr::reload::scheduler::Gate::new(std::time::Duration::ZERO);
    gate.observe(now).unwrap();
    let candidate = gate.start(now).unwrap().complete(value);
    (gate, candidate)
}

fn source(increment: &str) -> &'static str {
    match increment {
        "self.value += 1" => SOURCE_ONE,
        "self.value += 2" => SOURCE_TWO,
        _ => panic!("unsupported source body"),
    }
}

const SOURCE_ONE: &str = r#"
use iced::Element;
use iced::widget::{button, column, text};

#[derive(Default)]
struct Counter { value: i64 }

#[derive(Debug, Clone, Copy)]
enum Message { Increment }

impl Counter {
    fn update(&mut self, message: Message) {
        match message { Message::Increment => self.value += 1, }
    }

    #[preview]
    fn view(&self) -> Element<'_, Message> {
        let increment = button("Increment").on_press(Message::Increment);
        let value = text(self.value);
        column![value, increment]
    }
}

pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#;

const SOURCE_TWO: &str = r#"
use iced::Element;
use iced::widget::{button, column, text};

#[derive(Default)]
struct Counter { value: i64 }

#[derive(Debug, Clone, Copy)]
enum Message { Increment }

impl Counter {
    fn update(&mut self, message: Message) {
        match message { Message::Increment => self.value += 2, }
    }

    #[preview]
    fn view(&self) -> Element<'_, Message> {
        let increment = button("Increment").on_press(Message::Increment);
        let value = text(self.value);
        column![value, increment]
    }
}

pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#;

fn nested_source(nested: &str) -> String {
    format!(
        r#"
use iced::Element;
use iced::widget::button;

#[derive(Default)]
struct Nested {{ value: {nested} }}

#[derive(Default)]
struct State {{ nested: Nested }}

#[derive(Debug, Clone, Copy)]
enum Message {{ Tick }}

impl State {{
    fn update(&mut self, message: Message) {{
        match message {{ Message::Tick => (), }}
    }}

    fn view(&self) -> Element<Message> {{ button("Tick").on_press(Message::Tick) }}
}}

pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#,
    )
}

fn callback_source(task: bool) -> String {
    let output = if task { "-> Task<Message>" } else { "" };
    let body = if task { "Task::none()" } else { "()" };
    format!(
        r#"
use iced::Task;
use iced::Element;
use iced::widget::button;

#[derive(Default)]
struct State;

#[derive(Debug, Clone, Copy)]
enum Message {{ Tick }}

impl State {{
    fn update(&mut self, message: Message) {output} {{
        match message {{ Message::Tick => {body}, }}
    }}

    fn view(&self) -> Element<Message> {{ button("Tick").on_press(Message::Tick) }}
}}

pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#,
    )
}

fn preview_source(other: &str) -> String {
    format!(
        r#"
use iced::Element;
use iced::widget::button;

#[derive(Default)]
struct State {{ value: i64 }}

#[derive(Default)]
struct Other {{ value: {other} }}

#[derive(Debug, Clone, Copy)]
enum Message {{ Tick }}

impl State {{
    fn update(&mut self, message: Message) {{
        match message {{ Message::Tick => (), }}
    }}

    fn view(&self) -> Element<Message> {{ button("Tick").on_press(Message::Tick) }}

    #[preview(name = "First")]
    fn first(&self) -> Element<Message> {{ button("First").on_press(Message::Tick) }}
}}

impl Other {{
    fn update(&mut self, message: Message) {{
        match message {{ Message::Tick => (), }}
    }}

    #[preview(name = "Second", update = Other::update)]
    fn second(&self) -> Element<Message> {{ button("Second").on_press(Message::Tick) }}
}}

pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#,
    )
}

fn message_source(expanded: bool) -> String {
    let variant = if expanded { ", Tock" } else { "" };
    let arm = if expanded {
        "Message::Tick => (), Message::Tock => (),"
    } else {
        "Message::Tick => (),"
    };
    format!(
        r#"
use iced::Element;
use iced::widget::button;

#[derive(Default)]
struct State;

#[derive(Debug, Clone, Copy)]
enum Message {{ Tick{variant} }}

impl State {{
    fn update(&mut self, message: Message) {{
        match message {{ {arm} }}
    }}

    fn view(&self) -> Element<Message> {{ button("Tick").on_press(Message::Tick) }}
}}

pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#,
    )
}

fn widget_source(field: &str, value: &str) -> String {
    format!(
        r#"
use rawr_iced::widget::layout::Size;
use iced::Element;

#[derive(Default)]
struct State;

struct Dial {{ radius: {field} }}

#[derive(Debug, Clone, Copy)]
enum Message {{ Tick }}

#[widget]
impl Dial {{
    fn size(&self) -> Size {{ throw "size" }}
}}

impl State {{
    fn update(&mut self, message: Message) {{
        match message {{ Message::Tick => (), }}
    }}

    fn view(&self) -> Element<Message> {{ Dial {{ radius: {value} }} }}
}}

pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#,
    )
}

fn opaque_source(unused: &str) -> String {
    format!(
        r#"
use iced::Task;
use iced::Element;
use iced::widget::button;

struct Unused {{ value: {unused} }}

struct State {{ pending: Task<Message> }}

#[derive(Debug, Clone, Copy)]
enum Message {{ Tick }}

impl State {{
    fn boot() -> State {{ State {{ pending: Task::none() }} }}

    fn update(&mut self, message: Message) {{
        match message {{ Message::Tick => (), }}
    }}

    fn view(&self) -> Element<Message> {{ button("Tick").on_press(Message::Tick) }}
}}

pub fn main() -> iced::Result {{
    iced::application(State::boot, State::update, State::view).run()
}}
"#,
    )
}

struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let id = NEXT_PROJECT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rawr-reload-generation-{}-{id}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").unwrap();
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.root.join(path);
        // A `.rs` file is a Rawr file: it starts with `// rawr`.
        let marked;
        let source = if path.extension().is_some_and(|extension| extension == "rs") {
            marked = format!("// rawr\n{}", source);
            marked.as_str()
        } else {
            source
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, source).unwrap();
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}
