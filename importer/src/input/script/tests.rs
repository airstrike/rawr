use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use super::{outputs, Value};
use crate::cargo;
use crate::input::stored::{self, Budget};
use crate::input::{build, proof, Allocation, Usage};
use crate::{node, Error};

mod declarations;

const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

fn script() -> build::Script {
    build::Script {
        compiler: None,
        package_id: "pkg".into(),
        out_dir: "/build/unit/out".into(),
        cfg: Box::from([Box::from("alpha"), Box::from("beta")]),
        environment: Box::from([
            (Box::from("K"), Box::from("é")),
            (Box::from("EMPTY"), Box::from("")),
        ]),
    }
}

fn account() -> Allocation {
    Allocation::for_proof(PRIOR, &cargo::Limits::standard())
}

#[test]
fn cold_retention_preserves_order_spelling_and_original_field_charges() {
    let script = script();
    let execution = "/build/unit/./out";
    let mut allocation = account();
    let mut retained = Vec::new();
    outputs(&script, execution, &mut allocation, |value, allocation| {
        retained.push(value.retain(allocation)?);
        Ok(())
    })
    .expect("the cold recipe constructors retain the borrowed fields");
    let [directory, alpha, beta, key, empty] = retained.as_slice() else {
        panic!("one directory and four messages");
    };
    assert!(
        matches!(directory, proof::Recipe::Directory { role, package_id, path }
        if *role == proof::Role::OutDirectory && package_id.as_deref() == Some("pkg")
        && path.as_os_str() == script.out_dir.as_os_str())
    );
    for (recipe, expected_name, expected_value) in [
        (alpha, "cfg", "alpha"),
        (beta, "cfg", "beta"),
        (key, "env:K", "é"),
        (empty, "env:EMPTY", ""),
    ] {
        assert!(matches!(recipe, proof::Recipe::BuildMessage {
            package_id, execution: actual_execution, name, value,
        } if package_id.as_ref() == "pkg" && actual_execution.as_ref() == execution
            && name.as_ref() == expected_name && value.as_ref() == expected_value));
    }
    // Directory: two owned fields. Four messages: four each. Two formatted
    // names: one temporary each, independently of their retained message copy.
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 20,
            bytes: PRIOR.bytes
                + 3
                + 15
                + 4 * (3 + execution.len())
                + 3
                + 5
                + 3
                + 4
                + 5
                + 2
                + 9
                + 5
                + 9,
        }
    );
}

#[test]
fn borrowed_budget_consumer_allocates_only_formatted_names() {
    let script = script();
    let graph = node::GraphBudget::new();
    let mut allocation = account();
    let mut index = 0;
    outputs(
        &script,
        "execution",
        &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
        |value, _budget| {
            match value {
                Value::Directory { package_id, path } => {
                    assert_eq!(index, 0);
                    assert_eq!(package_id.as_ptr(), script.package_id.as_ptr());
                    assert!(std::ptr::eq(path, script.out_dir.as_path()));
                }
                Value::BuildMessage {
                    package_id, value, ..
                } => {
                    assert_eq!(package_id.as_ptr(), script.package_id.as_ptr());
                    let expected = match index {
                        1 => &script.cfg[0],
                        2 => &script.cfg[1],
                        3 => &script.environment[0].1,
                        4 => &script.environment[1].1,
                        _ => panic!("exact ordered message count"),
                    };
                    assert_eq!(value.as_ptr(), expected.as_ptr());
                }
                Value::SavedOutput { .. }
                | Value::Directive { .. }
                | Value::DeclaredPath { .. }
                | Value::Environment { .. } => {
                    panic!("only script output values emit");
                }
            }
            index += 1;
            Ok(())
        },
    )
    .expect("the current consumer borrows payloads");
    assert_eq!(index, 5);
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 2,
            bytes: PRIOR.bytes + 14
        }
    );
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .expect("formatted storage is owned by the original account, not graph decode");
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .expect("no hashing");
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
        .expect("no native observation");
}

#[test]
fn callback_failure_stops_before_later_name_formatting() {
    let script = script();
    for stop in 0..5 {
        let mut allocation = account();
        let mut calls = 0;
        let result = outputs(
            &script,
            "execution",
            &mut allocation,
            |_value, _allocation| {
                let current = calls;
                calls += 1;
                if current == stop {
                    Err(Error::Input {
                        code: "fixture-stop",
                        reason: "callback refused".into(),
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
        let (items, bytes) = match stop {
            0..=2 => (0, 0),
            3 => (1, 5),
            _ => (2, 14),
        };
        assert_eq!(
            allocation.usage(),
            Usage {
                items: PRIOR.items + items,
                bytes: PRIOR.bytes + bytes
            }
        );
    }
}

#[test]
fn formatted_name_records_exceedance_before_callback_and_keeps_original_account_terminal() {
    let script = script();
    let exact = Usage {
        items: PRIOR.items + 2,
        bytes: PRIOR.bytes + 14,
    };
    for floor in 0..3 {
        let mut allocation = account();
        allocation.item_limit = exact.items - usize::from(floor == 1);
        allocation.byte_limit = exact.bytes - usize::from(floor == 2);
        let mut calls = 0;
        // The second formatted name crosses the one-less target before its
        // callback, which already sees the recorded exceedance; every message
        // still emits and the original account holds the exact total.
        outputs(
            &script,
            "execution",
            &mut Budget::borrow(
                &cargo::Limits::standard(),
                &mut allocation,
                &node::GraphBudget::new(),
            ),
            |_value, budget| {
                calls += 1;
                assert_eq!(
                    budget.exceedances().is_exceeded(),
                    floor != 0 && calls == 5,
                    "the formatted name is charged before its callback"
                );
                Ok(())
            },
        )
        .expect("accounting targets never refuse");
        assert_eq!(calls, 5);
        assert_eq!(allocation.usage(), exact);
        let record = allocation.exceedances();
        match floor {
            0 => assert!(!record.is_exceeded()),
            1 => {
                let axis = record
                    .axis(crate::input::INPUT_ITEMS_RESOURCE)
                    .expect("one less item records the item axis");
                assert_eq!(axis.target, (exact.items - 1) as u64);
                assert_eq!(axis.high_water, exact.items as u64);
                assert_eq!(record.axes().count(), 1);
            }
            _ => {
                let axis = record
                    .axis(crate::input::INPUT_BYTES_RESOURCE)
                    .expect("one less byte records the byte axis");
                assert_eq!(axis.target, (exact.bytes - 1) as u64);
                assert_eq!(axis.high_water, exact.bytes as u64);
                assert_eq!(record.axes().count(), 1);
            }
        }
        allocation
            .items(0)
            .expect("an exceedance never latches the account");
    }
    // Only a genuine failure latches, and the latch is terminal.
    let mut allocation = account();
    allocation.refuse();
    let before = allocation.usage();
    assert!(matches!(
        outputs(
            &script,
            "execution",
            &mut Budget::borrow(
                &cargo::Limits::standard(),
                &mut allocation,
                &node::GraphBudget::new(),
            ),
            |_value, _budget| panic!("refused account cannot emit")
        ),
        Err(stored::Error::Refused)
    ));
    assert_eq!(allocation.usage(), before);
}

#[test]
fn native_directory_bytes_are_not_rendered_and_empty_messages_emit_once() {
    let mut script = script();
    script.out_dir = PathBuf::from(std::ffi::OsString::from_vec(b"/out/\xff".to_vec()));
    script.cfg = Box::from([]);
    script.environment = Box::from([]);
    let mut allocation = account();
    let mut count = 0;
    outputs(&script, "unused", &mut allocation, |value, allocation| {
        let proof::Recipe::Directory { path, .. } = value.retain(allocation)? else {
            panic!("only the OUT_DIR recipe remains");
        };
        assert_eq!(path.as_os_str(), script.out_dir.as_os_str());
        count += 1;
        Ok(())
    })
    .expect("the emitter preserves native path data without observation");
    assert_eq!(count, 1);
}
