use super::*;
use crate::process::{hot::Transition, RunnerError};

#[test]
fn consumed_hot_rejection_restores_pins_and_never_sends_duplicate_abort() {
    for wrapped in [false, true] {
        let mut fixture = Fixture::new();
        let (cold, _) = fixture.generation(b"before");
        let live = fixture.build(cold);
        live.pin_live_last_good().unwrap();
        fs::write(fixture.root.join("main.rawr"), b"after!").unwrap();
        let prepared = fixture.prepare(&live, b"after!");
        let store = fixture.context.open_store().unwrap();
        let before = pins(&store);
        let guards = store.fixture_pin_counts().unwrap();
        let mut transitions = Vec::new();
        let failure = prepared
            .commit(|transition| {
                transitions.push(transition);
                assert_eq!(
                    transition,
                    Transition::Commit,
                    "consumed preparation must not be aborted twice"
                );
                let failure =
                    RunnerError::HotConsumedRejected("widget failed before installation".into());
                Err(if wrapped {
                    RunnerError::PinCleanup {
                        failure: Box::new(failure),
                        cleanup: node::Error::Codec("fixture prior rollback failure"),
                    }
                } else {
                    failure
                })
            })
            .unwrap_err();
        assert_eq!(transitions, [Transition::Commit]);
        assert!(failure.is_hot_consumed_rejection());
        assert!(!failure.is_owner_lost());
        assert_eq!(pins(&store), before);
        assert_eq!(store.fixture_pin_counts().unwrap(), guards);
    }
}

#[test]
fn owner_loss_after_commit_does_not_send_abort_to_uncertain_owner() {
    let mut fixture = Fixture::new();
    let (cold, _) = fixture.generation(b"before");
    let live = fixture.build(cold);
    let prepared = fixture.prepare(&live, b"before");
    let mut transitions = Vec::new();
    let failure = prepared
        .commit(|transition| {
            transitions.push(transition);
            Err(RunnerError::OwnerLost {
                reason: "commit acknowledgement disconnected".into(),
            })
        })
        .unwrap_err();
    assert_eq!(transitions, [Transition::Commit]);
    assert!(failure.is_owner_lost());
    assert!(!failure.is_hot_consumed_rejection());
}
