use super::*;

fn frame(tag: crate::protocol::Tag, bytes: &[u8]) -> crate::protocol::Frame {
    crate::protocol::Frame::new(tag, bytes.into()).expect("bounded fixture frame")
}

#[test]
fn hot_commit_requires_exact_consumed_identity_and_valid_diagnostic() {
    use crate::protocol::Tag;
    let expected = [7; 48];
    assert!(hot_commit_outcome(frame(Tag::Admitted, &expected), &expected).is_ok());
    let mut payload = expected.to_vec();
    payload.extend_from_slice(b"widget rejected");
    assert!(matches!(
        hot_commit_outcome(frame(Tag::HotConsumedRejected, &payload), &expected),
        Err(Error::HotConsumedRejected(reason)) if reason.as_ref() == "widget rejected"
    ));
    for index in [0, 31, 32, 47] {
        let mut stale = payload.clone();
        stale[index] ^= 1;
        assert!(matches!(
            hot_commit_outcome(frame(Tag::HotConsumedRejected, &stale), &expected),
            Err(Error::Stale)
        ));
    }
    for length in [0, 31, 32, 47] {
        assert!(matches!(
            hot_commit_outcome(
                frame(Tag::HotConsumedRejected, &payload[..length]),
                &expected
            ),
            Err(Error::Stale)
        ));
    }
    assert!(matches!(
        hot_commit_outcome(frame(Tag::HotConsumedRejected, &expected), &expected),
        Err(Error::HotSuperseded)
    ));
    let mut invalid = expected.to_vec();
    invalid.push(0xff);
    assert!(hot_commit_outcome(frame(Tag::HotConsumedRejected, &invalid), &expected).is_err());
    for tag in [Tag::ProfileRejected, Tag::HotAborted, Tag::Prepared] {
        assert!(matches!(
            hot_commit_outcome(frame(tag, &payload), &expected),
            Err(Error::Stale)
        ));
    }
    assert!(matches!(
        hot_commit_outcome(frame(Tag::Admitted, &payload), &expected),
        Err(Error::Stale)
    ));
}

#[test]
fn consumed_rejection_survives_pin_cleanup_without_claiming_owner_loss() {
    let consumed = Error::PinCleanup {
        failure: Box::new(Error::HotConsumedRejected("rejected".into())),
        cleanup: crate::node::Error::Codec("fixture rollback failure"),
    };
    assert!(consumed.is_hot_consumed_rejection());
    assert!(!consumed.is_owner_lost());
    let generic = Error::PinCleanup {
        failure: Box::new(Error::ProfileRejected("rejected".into())),
        cleanup: crate::node::Error::Codec("fixture rollback failure"),
    };
    assert!(!generic.is_hot_consumed_rejection());
    let owner_lost = Error::HotCleanup {
        failure: Box::new(consumed),
        cleanup: Box::new(Error::OwnerLost {
            reason: "socket lost".into(),
        }),
    };
    assert!(owner_lost.is_owner_lost());
}

#[test]
fn hot_observation_payload_accounts_for_the_entire_early_identity() {
    let identity = [19; 56];
    let maximum = crate::protocol::FRAME_BYTES - 5 - identity.len();
    let payload = observation_payload(&identity, &vec![1; maximum]).unwrap();
    assert_eq!(payload.len() + 5, crate::protocol::FRAME_BYTES);
    assert_eq!(&payload[..56], &identity);
    assert!(matches!(
        observation_payload(&identity, &vec![1; maximum + 1]),
        Err(Error::Protocol(crate::protocol::Error::Oversized { .. }))
    ));
    assert!(observation_payload(&identity, &[]).is_err());
}
