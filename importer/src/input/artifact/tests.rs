use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use super::{derived, Value};
use crate::cargo;
use crate::input::stored::{self, Budget};
use crate::input::{
    dependency, proof, Allocation, Usage, INPUT_BYTES_RESOURCE, INPUT_ITEMS_RESOURCE,
};
use crate::{node, Error};

const PACKAGE: &str = "pkg";
const EXECUTION: &str = "/build/./artifact.d";
const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};
// Two File payloads, the execution path admission, and three Environment
// payloads. Present-empty and absent values have distinct item charges.
const RETAINED: Usage = Usage {
    items: 16,
    bytes: 124,
};

fn dependency() -> dependency::Dependency {
    dependency::Dependency {
        sources: Box::from([PathBuf::from("/src/a.rs"), PathBuf::from("/src/é.rs")]),
        environment: Box::from([
            (Box::from("A"), Some(Box::from("one"))),
            (Box::from("EMPTY"), Some(Box::from(""))),
            (Box::from("UNSET"), None),
        ]),
    }
}

fn account() -> Allocation {
    Allocation::for_proof(PRIOR, &cargo::Limits::standard())
}

fn retain(
    dependency: &dependency::Dependency,
    allocation: &mut Allocation,
) -> crate::Result<Vec<proof::Recipe>> {
    let mut recipes = Vec::new();
    derived(
        dependency,
        PACKAGE,
        Path::new(EXECUTION),
        allocation,
        |value, allocation| {
            recipes.push(value.retain(allocation)?);
            Ok(())
        },
    )?;
    Ok(recipes)
}

#[test]
fn cold_retention_preserves_source_order_execution_and_optional_environment() {
    let dependency = dependency();
    let mut allocation = account();
    let recipes = retain(&dependency, &mut allocation).expect("cold fields retain");
    let [first, second, set, empty, absent] = recipes.as_slice() else {
        panic!("two source files and three environment records");
    };
    for (recipe, source) in [(first, "/src/a.rs"), (second, "/src/é.rs")] {
        assert!(
            matches!(recipe, proof::Recipe::File { role, package_id, path }
            if *role == proof::Role::DependencySource
                && package_id.as_deref() == Some(PACKAGE) && path == Path::new(source))
        );
    }
    for (recipe, expected_key, expected_value) in [
        (set, "A", Some("one")),
        (empty, "EMPTY", Some("")),
        (absent, "UNSET", None),
    ] {
        assert!(
            matches!(recipe, proof::Recipe::Environment { package_id, execution, key, value }
            if package_id.as_ref() == PACKAGE && execution.as_ref() == EXECUTION
                && key.as_ref() == expected_key && value.as_deref() == expected_value)
        );
    }
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + RETAINED.items,
            bytes: PRIOR.bytes + RETAINED.bytes,
        }
    );
}

#[test]
fn current_consumer_borrows_all_fields_without_storage_hashing_or_native_work() {
    let dependency = dependency();
    let package = String::from(PACKAGE);
    let execution_path = PathBuf::from(EXECUTION);
    let graph = node::GraphBudget::new();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .expect("prior decoding");
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .expect("prior hashing");
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
        .expect("prior native work");
    let mut allocation = account();
    let mut calls = 0;
    derived(
        &dependency,
        &package,
        &execution_path,
        &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
        |value, _budget| {
            match value {
                Value::File { package_id, path } => {
                    assert_eq!(package_id.as_ptr(), package.as_ptr());
                    assert!(std::ptr::eq(path, dependency.sources[calls].as_path()));
                }
                Value::Environment {
                    package_id,
                    execution,
                    key,
                    value,
                } => {
                    let (expected_key, expected_value) = &dependency.environment[calls - 2];
                    assert_eq!(package_id.as_ptr(), package.as_ptr());
                    assert_eq!(
                        execution.as_ptr(),
                        execution_path.as_os_str().as_encoded_bytes().as_ptr()
                    );
                    assert_eq!(key.as_ptr(), expected_key.as_ptr());
                    assert_eq!(
                        value.map(str::as_ptr),
                        expected_value.as_deref().map(str::as_ptr)
                    );
                }
            }
            calls += 1;
            Ok(())
        },
    )
    .expect("borrowing adds no ownership or observation work");
    assert_eq!(calls, 5);
    assert_eq!(allocation.usage(), PRIOR);
}

#[test]
fn callback_failure_precedes_later_emissions_and_execution_path_admission() {
    let dependency = dependency();
    for stop in 0..5 {
        let mut allocation = account();
        let mut calls = 0;
        let result = derived(
            &dependency,
            PACKAGE,
            Path::new(EXECUTION),
            &mut allocation,
            |_value, _allocation| {
                let current = calls;
                calls += 1;
                if current == stop {
                    Err(Error::Input {
                        code: "fixture-stop",
                        reason: "sink refused".into(),
                    })
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(
            result,
            Err(Error::Input {
                code: "fixture-stop",
                ..
            })
        ));
        assert_eq!(calls, stop + 1);
        assert_eq!(
            allocation.usage(),
            Usage {
                items: PRIOR.items + usize::from(stop >= 2),
                bytes: PRIOR.bytes + if stop >= 2 { EXECUTION.len() } else { 0 },
            }
        );
    }
}

#[test]
fn execution_text_is_checked_after_sources_even_without_environment() {
    let mut dependency = dependency();
    dependency.environment = Box::from([]);
    let invalid = PathBuf::from(std::ffi::OsString::from_vec(b"/artifact\xff.d".to_vec()));
    let mut allocation = account();
    let mut sources = 0;
    let result = derived(
        &dependency,
        PACKAGE,
        &invalid,
        &mut allocation,
        |value, _allocation| {
            assert!(matches!(value, Value::File { .. }));
            sources += 1;
            Ok(())
        },
    );
    assert!(matches!(
        result,
        Err(Error::Path {
            field: "rustc dep-info execution",
            ..
        })
    ));
    assert_eq!(sources, 2);
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 1,
            bytes: PRIOR.bytes + invalid.as_os_str().len(),
        }
    );
}

#[test]
fn cold_retention_preserves_exact_remaining_limits_and_permanent_refusal() {
    let dependency = dependency();
    let exact = Usage {
        items: PRIOR.items + RETAINED.items,
        bytes: PRIOR.bytes + RETAINED.bytes,
    };
    for floor in 0..3 {
        let mut allocation = account();
        allocation.item_limit = exact.items - usize::from(floor == 1);
        allocation.byte_limit = exact.bytes - usize::from(floor == 2);
        // The exact remainder records nothing; one less on either axis is
        // an exceedance whose high water is the exact retained total, and the
        // retention still completes on the original account.
        retain(&dependency, &mut allocation).expect("accounting targets never refuse");
        assert_eq!(allocation.usage(), exact);
        let record = allocation.exceedances();
        match floor {
            0 => assert!(!record.is_exceeded()),
            1 => {
                let axis = record
                    .axis(INPUT_ITEMS_RESOURCE)
                    .expect("one less item records the item axis");
                assert_eq!(axis.target, (exact.items - 1) as u64);
                assert_eq!(axis.high_water, exact.items as u64);
                assert!(record.axis(INPUT_BYTES_RESOURCE).is_none());
            }
            _ => {
                let axis = record
                    .axis(INPUT_BYTES_RESOURCE)
                    .expect("one less byte records the byte axis");
                assert_eq!(axis.target, (exact.bytes - 1) as u64);
                assert_eq!(axis.high_water, exact.bytes as u64);
                assert!(record.axis(INPUT_ITEMS_RESOURCE).is_none());
            }
        }
        allocation
            .items(0)
            .expect("an exceedance never latches the account");
    }
    // Only a genuine failure latches, and the latch is permanent: the current
    // comparison does no further work and emits nothing.
    let mut allocation = account();
    allocation.refuse();
    let before = allocation.usage();
    assert!(matches!(
        derived(
            &dependency,
            PACKAGE,
            Path::new(EXECUTION),
            &mut Budget::borrow(
                &cargo::Limits::standard(),
                &mut allocation,
                &node::GraphBudget::new()
            ),
            |_value, _budget| panic!("refused account cannot emit")
        ),
        Err(stored::Error::Refused)
    ));
    assert_eq!(allocation.usage(), before);
}

#[test]
fn native_source_bytes_survive_and_borrowed_execution_has_only_its_path_limit() {
    let mut dependency = dependency();
    dependency.sources[0] = PathBuf::from(std::ffi::OsString::from_vec(b"/source\xff.rs".to_vec()));
    let recipes = retain(&dependency, &mut account()).expect("native source payload retains");
    assert!(matches!(&recipes[0], proof::Recipe::File { path, .. }
        if path.as_os_str() == dependency.sources[0].as_os_str()));
    let mut limits = cargo::Limits::standard();
    limits.string_bytes = 1;
    let mut calls = 0;
    derived(
        &dependency,
        PACKAGE,
        Path::new(EXECUTION),
        &mut Budget::borrow(&limits, &mut account(), &node::GraphBudget::new()),
        |_value, _budget| {
            calls += 1;
            Ok(())
        },
    )
    .expect("borrowed execution text is not scalar-limited");
    assert_eq!(calls, 5);
}
