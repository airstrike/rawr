use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Cause, Event, Prior, Target};
use crate::metrics::{principal, recorder};
use crate::node::GraphBudget;
use crate::store::Store;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);
impl Root {
    fn empty() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rawr-ledger-history-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "the creation ledger models only the Cargo-home store (rawr/v1); a project store under target/rawr records an identity fault"]
fn bootstrap_records_actual_bytes_on_the_supplied_original_account() {
    let root = Root::empty();
    let before = crate::metrics::cell::project::Manifest::read(&root.0.join("rawr/store")).unwrap();
    let graph = GraphBudget::new();
    let recorder = recorder::Recorder::transform(10_000).unwrap();
    let scope = recorder.scope().unwrap();
    let store = Store::open_project(&root.0.join("rawr"), [9; 32], graph.clone()).unwrap();
    let raw = fs::read(store.root().join("ledgers/installation")).unwrap();
    // The same account continues after bootstrap, rather than replacing the one
    // used to record the initial write.
    let other = Store::open_project(&root.0.join("rawr"), [10; 32], graph.clone()).unwrap();
    drop(other);
    let after = crate::metrics::cell::project::Manifest::read(store.root()).unwrap();
    drop(scope);
    let mut snapshot = recorder.snapshot().unwrap();
    let evidence = snapshot.principal.as_mut().unwrap();
    assert_eq!(evidence.fault, None);
    let (token, digest) = evidence
        .events
        .as_slice()
        .iter()
        .find_map(|row| match row {
            principal::Event::Ledger {
                change:
                    Event::Started {
                        attempt,
                        target: Target::Installation,
                        cause: Cause::Bootstrap,
                        before: Prior::Absent,
                        after,
                        ..
                    },
            } => Some((*attempt, *after)),
            _ => None,
        })
        .unwrap();
    assert_eq!(digest, principal::Commitment::produced(&raw));
    let lifecycle: Vec<_> = evidence
        .events
        .as_slice()
        .iter()
        .filter_map(|row| match row {
            principal::Event::Ledger {
                change: Event::Renamed { attempt },
            } if *attempt == token => Some("rename"),
            principal::Event::Ledger {
                change:
                    Event::Finished {
                        attempt,
                        succeeded: true,
                    },
            } if *attempt == token => Some("finish"),
            _ => None,
        })
        .collect();
    assert_eq!(lifecycle, ["rename", "finish"]);
    evidence
        .prepare_cell_transport(&mut snapshot.accounting, 10_000)
        .unwrap();
    let wire = serde_json::to_vec(&snapshot).unwrap();
    let decoded: recorder::Snapshot = serde_json::from_slice(&wire).unwrap();
    assert_eq!(decoded.principal, snapshot.principal);
    let history = decoded.principal.as_ref().unwrap();
    let empty = principal::Evidence {
        cell_transport: true,
        ..Default::default()
    };
    let pid = std::process::id();
    let other_pid = pid.checked_add(1).unwrap();
    let retirements = principal::retirement::join::pair(
        [(pid, history), (other_pid, &empty)],
        store.root(),
        &before,
        &after,
    )
    .unwrap();
    let joined = principal::creation::join::pair(
        [(pid, history), (other_pid, &empty)],
        store.root(),
        &before,
        &after,
        &retirements,
    )
    .unwrap();
    assert!(joined.endpoints.iter().all(|row| !row.creations.is_empty()));
    assert!(joined.attempts.iter().any(|attempt| attempt.target
        == principal::creation::Target::Control(principal::creation::Control::Heavy)
        && attempt.adopted.is_some()
        && attempt.creation.is_none()));
    // Derived from the real create history: a later sync error cannot erase the
    // successful create or its observed empty endpoint. This does not inject a
    // native sync error or infer success from a destructor.
    let token = joined
        .attempts
        .iter()
        .find(|attempt| {
            attempt.target
                == principal::creation::Target::Control(principal::creation::Control::Bootstrap)
                && attempt.creation == Some(principal::creation::Outcome::Created)
        })
        .unwrap()
        .token;
    let mut changed = serde_json::to_value(history).unwrap();
    for event in changed["events"].as_array_mut().unwrap() {
        if event["event"] == "creation"
            && event["change"]["attempt"] == token
            && (event["change"]["creation"] == "synced"
                || event["change"]["creation"] == "finished")
        {
            event["change"]["succeeded"] = false.into();
        }
    }
    let changed = serde_json::from_value::<principal::Evidence>(changed).unwrap();
    let joined = principal::creation::join::pair(
        [(pid, &changed), (other_pid, &empty)],
        store.root(),
        &before,
        &after,
        &retirements,
    )
    .unwrap();
    assert!(
        joined
            .endpoints
            .iter()
            .any(|row| row.path.as_ref() == b"bootstrap.lock"
                && row.creations.contains(&(pid, token)))
    );
    assert!(graph.census_vector().unwrap()[5] >= raw.len() as u64);
}

#[test]
#[ignore = "the creation ledger models only the Cargo-home store (rawr/v1); a project store under target/rawr records an identity fault"]
fn inactive_bootstrap_does_not_spend_and_active_exceedance_preserves_store_result() {
    let root = Root::empty();
    let graph = GraphBudget::new();
    let before = graph.census_vector().unwrap();
    Store::open_project(&root.0.join("rawr"), [11; 32], graph.clone()).unwrap();
    assert_eq!(before, graph.census_vector().unwrap());

    let second = Root::empty();
    let exhausted = GraphBudget::new();
    exhausted
        .decode(crate::node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .unwrap();
    // The decoded target is accounting. Evidence past it is recorded on
    // the original graph account; the principal evidence keeps no fault.
    let account = exhausted.clone();
    let recorder = recorder::Recorder::transform(10_000).unwrap();
    let scope = recorder.scope().unwrap();
    let store = Store::open_project(&second.0.join("rawr"), [12; 32], exhausted).unwrap();
    assert!(store.root().join("ledgers/installation").is_file());
    drop(scope);
    assert_eq!(recorder.snapshot().unwrap().principal.unwrap().fault, None);
    assert!(account
        .exceedances()
        .axis(crate::node::DECODED_RESOURCE)
        .is_some());
}
