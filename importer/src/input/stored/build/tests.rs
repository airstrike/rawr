use std::fs;

use super::require;
use crate::cargo::{unit, Limits};
use crate::input::proof::{Entry, Role};
use crate::input::stored::observe::slots::Slots;
use crate::input::stored::{Budget, Error, Recipe, Result};
use crate::input::{Allocation, Usage};
use crate::node;

mod fixture;

use fixture::Fixture;

const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

fn acquire(
    fixture: &Fixture,
    recipes: &[Recipe],
    overrides: bool,
    allocation: &mut Allocation,
    graph: &node::GraphBudget,
) -> Result<Slots<Entry>> {
    let mut entries = Slots::new();
    require(
        &mut recipes.iter(),
        &mut entries,
        &fixture.build,
        overrides,
        &fixture.metadata,
        Some(&fixture.home),
        &fixture.base,
        |_, _, _| Ok(()),
        &mut Budget::borrow(&Limits::standard(), allocation, graph),
    )?;
    Ok(entries)
}

fn observe(fixture: &Fixture, recipes: &[Recipe]) -> Result<Slots<Entry>> {
    acquire(
        fixture,
        recipes,
        false,
        &mut Allocation::for_proof(PRIOR, &Limits::standard()),
        &node::GraphBudget::new(),
    )
}

fn canonical(entries: &[Entry]) -> serde_json::Value {
    let mut entries = entries.iter().collect::<Vec<_>>();
    entries.sort_unstable();
    entries.dedup();
    serde_json::to_value(entries).expect("entry comparison")
}

#[test]
fn actual_build_and_cold_recipes_cover_every_artifact_and_declared_script_entry() {
    let fixture = Fixture::open(false, false);
    let (recipes, expected) = fixture.cold();
    assert_eq!(fixture.build.build().evidence.artifacts.len(), 2);
    assert_eq!(fixture.build.build().evidence.scripts.len(), 2);
    assert_eq!(
        recipes
            .iter()
            .filter(|recipe| matches!(recipe, Recipe::DepInfo { .. }))
            .count(),
        2
    );
    assert_eq!(
        recipes
            .iter()
            .filter(|recipe| matches!(recipe, Recipe::SavedOutput { .. }))
            .count(),
        2
    );
    let observed = observe(&fixture, &recipes).expect("complete Build section");
    assert_eq!(observed.as_slice().len(), recipes.len());
    assert!(
        expected.len() < observed.as_slice().len(),
        "duplicate cold entries retain their recipe occurrences"
    );
    assert_eq!(canonical(observed.as_slice()), canonical(&expected));
    for key in [
        "DEP_SET",
        "DEP_EMPTY",
        "DEP_UNSET",
        "SET",
        "EMPTY",
        "MISSING",
    ] {
        assert!(observed.as_slice().iter().any(|entry| matches!(entry, Entry::Environment { key: actual, .. } if actual.as_ref() == key)), "covered environment {key}");
    }
    assert!(observed.as_slice().iter().any(|entry| matches!(
        entry,
        Entry::Missing {
            role: Role::DeclaredInput,
            ..
        }
    )));
}

#[test]
fn caller_prefix_cursor_and_buffer_are_preserved_without_searching_or_owning_copies() {
    let fixture = Fixture::open(false, false);
    let (mut recipes, expected) = fixture.cold();
    recipes.insert(
        0,
        Recipe::Fact {
            role: Role::Target,
            package_id: None,
            name: "target".into(),
            value: "fixture".into(),
        },
    );
    assert!(
        matches!(observe(&fixture, &recipes), Err(Error::Encoding(_))),
        "unconsumed prefix cannot be skipped"
    );
    let pointer = recipes.as_ptr();
    let mut cursor = recipes.iter();
    assert!(matches!(
        cursor.next(),
        Some(Recipe::Fact {
            role: Role::Target,
            ..
        })
    ));
    let limits = Limits::standard();
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let mut entries = Slots::new();
    let prefix = Entry::Fact {
        role: Role::Target,
        package_id: None,
        name: "target".into(),
        value: "fixture".into(),
    };
    entries
        .push(prefix.clone(), &mut budget)
        .expect("caller prefix entry");
    require(
        &mut cursor,
        &mut entries,
        &fixture.build,
        false,
        &fixture.metadata,
        Some(&fixture.home),
        &fixture.base,
        |_, _, _| Ok(()),
        &mut budget,
    )
    .expect("same cursor continues");
    assert!(cursor.as_slice().is_empty());
    assert_eq!(pointer, recipes.as_ptr());
    assert!(entries.as_slice()[0] == prefix);
    assert_eq!(canonical(&entries.as_slice()[1..]), canonical(&expected));
}

#[test]
fn omission_of_each_recipe_and_extra_or_reordered_records_reject() {
    let fixture = Fixture::open(false, false);
    let (mut recipes, _) = fixture.cold();
    for index in 0..recipes.len() {
        let removed = recipes.remove(index);
        assert!(
            observe(&fixture, &recipes).is_err(),
            "omitted recipe {index}"
        );
        recipes.insert(index, removed);
    }
    recipes.swap(0, 1);
    assert!(matches!(
        observe(&fixture, &recipes),
        Err(Error::Encoding(_))
    ));
    recipes.swap(0, 1);
    recipes.push(Recipe::SavedOutput {
        package_id: fixture.id.clone().into(),
        out_dir: fixture.build.build().evidence.scripts[0].out_dir.clone(),
    });
    assert!(matches!(
        observe(&fixture, &recipes),
        Err(Error::Encoding("stored Build recipes remain"))
    ));
}

#[test]
fn every_script_payload_and_optional_environment_value_is_compared() {
    let fixture = Fixture::open(false, false);
    for mode in 0..6 {
        let (mut recipes, _) = fixture.cold();
        let changed = recipes
            .iter_mut()
            .find(|recipe| match mode {
                0 | 1 => matches!(recipe, Recipe::Environment { .. }),
                2 => matches!(recipe, Recipe::Directive { .. }),
                3 => matches!(recipe, Recipe::BuildMessage { .. }),
                4 => matches!(recipe, Recipe::SavedOutput { .. }),
                5 => matches!(recipe, Recipe::Directory { .. }),
                _ => false,
            })
            .expect("covered recipe shape");
        match (mode, changed) {
            (0, Recipe::Environment { key, .. }) => *key = "DIFFERENT".into(),
            (1, Recipe::Environment { value, .. }) => {
                *value = match value {
                    Some(_) => None,
                    None => Some("".into()),
                }
            }
            (2, Recipe::Directive { index, .. }) => *index += 1,
            (3, Recipe::BuildMessage { name, .. }) => *name = "env:DIFFERENT".into(),
            (4, Recipe::SavedOutput { out_dir, .. }) => *out_dir = out_dir.join("."),
            (5, Recipe::Directory { role, .. }) => *role = Role::DeclaredInput,
            _ => unreachable!(),
        }
        assert!(
            matches!(observe(&fixture, &recipes), Err(Error::Encoding(_))),
            "changed payload {mode}"
        );
    }
}

#[test]
fn exact_header_owner_paths_and_ordered_out_directories_precede_native_work() {
    let fixture = Fixture::open(false, false);
    for mode in 0..5 {
        let (mut recipes, _) = fixture.cold();
        let Recipe::DepInfo {
            package_id,
            path,
            package_root,
            out_directories,
        } = &mut recipes[0]
        else {
            panic!("cold DepInfo header")
        };
        match mode {
            0 => *package_id = "other".into(),
            1 => *path = path.join("."),
            2 => *package_root = package_root.join("."),
            3 => out_directories.swap(0, 1),
            4 => *out_directories = Box::default(),
            _ => unreachable!(),
        }
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("zero remaining path work");
        let mut allocation = Allocation::for_proof(PRIOR, &Limits::standard());
        assert!(
            matches!(
                acquire(&fixture, &recipes, false, &mut allocation, &graph),
                Err(Error::Encoding(_))
            ),
            "header association {mode}"
        );
        assert_eq!(allocation.usage(), PRIOR);
    }
}

#[test]
fn changed_inventory_or_structured_declarations_cannot_select_a_recipe_subset() {
    let mut fixture = Fixture::open(false, false);
    let (recipes, _) = fixture.cold();
    let artifacts = std::mem::take(&mut fixture.build.build.evidence.artifacts);
    assert!(matches!(
        observe(&fixture, &recipes),
        Err(Error::Encoding(_))
    ));
    fixture.build.build.evidence.artifacts = artifacts;
    fs::write(fixture.root.join("new.rs"), b"new source").expect("new source");
    let dep = &fixture.build.build().evidence.artifacts[0].dep_info;
    fs::write(dep, b"target: new.rs\n").expect("changed dependency membership");
    assert!(matches!(
        observe(&fixture, &recipes),
        Err(Error::Encoding(_))
    ));
    fs::write(dep, b"target: source\\ \xc3\xa9.rs second.rs\n# env-dep:DEP_SET=value=tail\n# env-dep:DEP_EMPTY=\n# env-dep:DEP_UNSET\n").expect("restore parsed dependency");
    let script = &fixture.build.build().evidence.scripts[0];
    fs::write(
        script.out_dir.parent().expect("execution").join("output"),
        b"cargo:rerun-if-changed=declared\ncargo:metadata=new\n",
    )
    .expect("changed saved output");
    assert!(matches!(
        observe(&fixture, &recipes),
        Err(Error::Encoding(_))
    ));
}

#[test]
fn content_drift_is_returned_for_the_callers_complete_entry_comparison() {
    let fixture = Fixture::open(false, false);
    let (recipes, expected) = fixture.cold();
    fs::write(fixture.root.join("second.rs"), b"pub fn altered() {}\n")
        .expect("changed source content");
    let observed =
        observe(&fixture, &recipes).expect("recipe coverage alone supplies no current proof");
    assert_ne!(canonical(observed.as_slice()), canonical(&expected));
}

#[test]
fn registry_fallback_is_required_once_per_package_before_each_scripts_outputs() {
    let fixture = Fixture::open(true, true);
    let (mut recipes, expected) = fixture.cold();
    assert_eq!(
        recipes
            .iter()
            .filter(|recipe| matches!(recipe, Recipe::Package(_)))
            .count(),
        1
    );
    let observed = observe(&fixture, &recipes).expect("Registry fallback section");
    assert_eq!(
        observed
            .as_slice()
            .iter()
            .filter(|entry| matches!(entry, Entry::Registry { .. }))
            .count(),
        1
    );
    assert_eq!(canonical(observed.as_slice()), canonical(&expected));
    let index = recipes
        .iter()
        .position(|recipe| matches!(recipe, Recipe::Package(_)))
        .expect("fallback recipe");
    recipes.remove(index);
    assert!(matches!(
        observe(&fixture, &recipes),
        Err(Error::Encoding(_))
    ));
}

#[test]
fn fallback_metadata_must_match_build_and_path_git_remain_incomplete() {
    let mut fixture = Fixture::open(true, true);
    let (recipes, _) = fixture.cold();
    let original = fixture
        .metadata
        .package(&fixture.id)
        .expect("metadata package")
        .clone();
    fixture.metadata = unit::Metadata::fixture_packages([]);
    assert!(matches!(
        observe(&fixture, &recipes),
        Err(Error::Encoding(_))
    ));
    fixture.metadata = unit::Metadata::fixture_packages([unit::Package::fixture(
        &fixture.id,
        "package",
        "1.0.0",
        fixture.root.join("."),
        original.source().clone(),
    )]);
    assert!(matches!(
        observe(&fixture, &recipes),
        Err(Error::Encoding(_))
    ));
    for source in [
        unit::PackageSource::Path {
            cargo_id: "path+file:///fixture".into(),
        },
        unit::PackageSource::Git {
            cargo_id: "git+https://fixture.invalid/repo".into(),
            oid: "0123456789012345678901234567890123456789".into(),
        },
    ] {
        fixture.metadata = unit::Metadata::fixture_packages([unit::Package::fixture(
            &fixture.id,
            "package",
            "1.0.0",
            fixture.root.clone(),
            source,
        )]);
        assert!(matches!(
            observe(&fixture, &recipes),
            Err(Error::IncompleteWitness)
        ));
    }
}

#[test]
fn environment_override_rejects_before_saved_output_recipe_consumption() {
    let fixture = Fixture::open(false, false);
    let (recipes, _) = fixture.cold();
    let mut cursor = recipes.iter();
    let mut entries = Slots::new();
    let limits = Limits::standard();
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    assert!(matches!(
        require(
            &mut cursor,
            &mut entries,
            &fixture.build,
            true,
            &fixture.metadata,
            Some(&fixture.home),
            &fixture.base,
            |_, _, _| Ok(()),
            &mut Budget::borrow(&limits, &mut allocation, &graph)
        ),
        Err(Error::Encoding("cargo-env-config-unproved"))
    ));
    assert!(matches!(
        cursor.as_slice().first(),
        Some(Recipe::SavedOutput { .. })
    ));
    assert!(entries
        .as_slice()
        .iter()
        .all(|entry| !matches!(entry, Entry::BuildMessage { .. } | Entry::Directive { .. })));
}

#[test]
fn saved_output_read_failure_precedes_its_header_comparison() {
    let fixture = Fixture::open(false, false);
    let (mut recipes, _) = fixture.cold();
    let Recipe::SavedOutput { package_id, .. } = recipes
        .iter_mut()
        .find(|recipe| matches!(recipe, Recipe::SavedOutput { .. }))
        .expect("saved-output header")
    else {
        unreachable!()
    };
    *package_id = "wrong stored owner".into();
    let output = fixture.build.build().evidence.scripts[0]
        .out_dir
        .parent()
        .expect("execution")
        .join("output");
    fs::write(output, b"unterminated").expect("incomplete saved output");
    assert!(matches!(
        observe(&fixture, &recipes),
        Err(Error::Observation(
            crate::input::stored::observe::Error::Policy("truncated-saved-output")
        ))
    ));
}

#[test]
fn original_nonzero_accounts_have_exact_floors_and_persistent_refusal() {
    let fixture = Fixture::open(true, true);
    let (recipes, _) = fixture.cold();
    let standard = Limits::standard();
    let mut allocation = Allocation::for_proof(PRIOR, &standard);
    acquire(
        &fixture,
        &recipes,
        false,
        &mut allocation,
        &node::GraphBudget::new(),
    )
    .expect("measure actual storage");
    let spent = allocation.usage();
    assert!(spent.items > PRIOR.items && spent.bytes > PRIOR.bytes);
    for mode in 0..3 {
        let mut limits = standard;
        limits.items = spent.items - usize::from(mode == 1);
        limits.input_proof_bytes = spent.bytes - usize::from(mode == 2);
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let result = acquire(
            &fixture,
            &recipes,
            false,
            &mut allocation,
            &node::GraphBudget::new(),
        );
        // The exact storage is charged either way; a ceiling one under
        // records the axis with the exact total as its high water.
        result.expect("original storage is owned, not gated");
        assert_eq!(allocation.usage(), spent, "one-under account {mode}");
        let record = allocation.exceedances();
        if mode == 0 {
            assert!(!record.is_exceeded(), "exact original ceilings");
        } else {
            let (name, total) = if mode == 1 {
                (crate::input::INPUT_ITEMS_RESOURCE, spent.items)
            } else {
                (crate::input::INPUT_BYTES_RESOURCE, spent.bytes)
            };
            let axis = record.axis(name).expect("one-under axis recorded");
            assert_eq!(axis.target as usize, total - 1);
            assert_eq!(axis.high_water as usize, total);
            assert!(allocation.bytes(0).is_ok(), "an exceedance never latches");
            // A genuine failure latches the account persistently.
            allocation.refuse();
            let stopped = allocation.usage();
            assert!(matches!(
                acquire(
                    &fixture,
                    &recipes,
                    false,
                    &mut allocation,
                    &node::GraphBudget::new()
                ),
                Err(Error::Refused)
            ));
            assert_eq!(allocation.usage(), stopped);
        }
    }
}

#[test]
fn original_graph_exceedance_is_recorded_and_latched_accounts_stop_later_observation() {
    let fixture = Fixture::open(false, false);
    let (recipes, _) = fixture.cold();
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
        .expect("consume current path account");
    let mut allocation = Allocation::for_proof(PRIOR, &Limits::standard());
    // Path checks past the target are charged and recorded; the
    // observation completes on the exhausted graph.
    acquire(&fixture, &recipes, false, &mut allocation, &graph)
        .expect("path checks are measured, not gated");
    let record = graph.exceedances();
    let axis = record
        .axis(node::PATHS_RESOURCE)
        .expect("path axis recorded");
    assert_eq!(axis.target as usize, node::MAX_PATH_CHECKS_PER_GRAPH);
    assert_eq!(
        axis.first_requested as usize,
        node::MAX_PATH_CHECKS_PER_GRAPH + 1
    );
    assert_eq!(
        axis.high_water as usize,
        graph.census_usage().expect("graph census").1,
        "high water is the truthful cumulative path count"
    );
    assert!(axis.site.is_some());
    assert!(allocation.bytes(0).is_ok(), "an exceedance never latches");
    // A genuine failure latches; a latched account observes nothing more.
    allocation.refuse();
    assert!(matches!(
        acquire(
            &fixture,
            &recipes,
            false,
            &mut allocation,
            &node::GraphBudget::new()
        ),
        Err(Error::Refused)
    ));
    let mut limits = Limits::standard();
    limits.input_proof_bytes = PRIOR.bytes;
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    // The real entry slot is admitted and recorded before the observer runs.
    let mut called = false;
    let result = super::push(
        &mut Slots::new(),
        &mut Budget::borrow(&limits, &mut allocation, &node::GraphBudget::new()),
        |_| {
            called = true;
            Err(Error::Encoding("observer payload"))
        },
    );
    assert!(matches!(result, Err(Error::Encoding("observer payload"))));
    assert!(called, "the admitted Entry slot reaches the observer");
    let record = allocation.exceedances();
    let axis = record
        .axis(crate::input::INPUT_BYTES_RESOURCE)
        .expect("slot byte axis recorded");
    assert_eq!(axis.target as usize, PRIOR.bytes);
    assert_eq!(
        axis.first_requested as usize,
        PRIOR.bytes + std::mem::size_of::<Entry>()
    );
    // A latched account refuses the slot before any observer payload.
    allocation.refuse();
    let mut called = false;
    let result = super::push(
        &mut Slots::new(),
        &mut Budget::borrow(&limits, &mut allocation, &node::GraphBudget::new()),
        |_| {
            called = true;
            Err(Error::Encoding("must not observe"))
        },
    );
    assert!(matches!(result, Err(Error::Refused)));
    assert!(
        !called,
        "latched Entry slot refuses before observer payload"
    );
}

#[test]
fn cargo_home_is_required_only_for_registry_fallback() {
    for registry_fallback in [false, true] {
        let fixture = Fixture::open(registry_fallback, registry_fallback);
        let (recipes, expected) = fixture.cold();
        let limits = Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let mut entries = Slots::new();
        let result = require(
            &mut recipes.iter(),
            &mut entries,
            &fixture.build,
            false,
            &fixture.metadata,
            None,
            &fixture.base,
            |_, _, _| Ok(()),
            &mut Budget::borrow(&limits, &mut allocation, &graph),
        );
        if registry_fallback {
            assert!(matches!(
                result,
                Err(Error::Encoding("missing-cargo-home-authority"))
            ));
        } else {
            result.expect("declared script inputs do not require Cargo home");
            assert_eq!(canonical(entries.as_slice()), canonical(&expected));
        }
    }
}
