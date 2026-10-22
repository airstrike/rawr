use super::*;
use std::io::Cursor;

#[test]
fn current_version_round_trips_consumed_rejection_and_rejects_old_peers() {
    assert_eq!(VERSION, 11);
    let mut payload = vec![7; 48];
    payload.extend_from_slice(b"rejected");
    let frame = Frame::new(Tag::HotConsumedRejected, payload.clone().into_boxed_slice())
        .expect("bounded fixture");
    let mut bytes = Vec::new();
    send(&mut bytes, &frame).expect("encode current version");
    let received = receive(&mut Cursor::new(&bytes)).expect("decode current version");
    assert_eq!(received.tag(), Tag::HotConsumedRejected);
    assert_eq!(received.payload(), payload);
    for old in [2_u32, 3, 4, 5, 6, 7, 8, 9, 10] {
        let mut old_wire = bytes.clone();
        old_wire[4..8].copy_from_slice(&old.to_be_bytes());
        assert!(
            matches!(receive(&mut Cursor::new(&old_wire)), Err(Error::Version { expected: 11, actual }) if actual == old)
        );
        assert!(
            matches!(Decoder::new().receive(&mut Cursor::new(old_wire)), Err(Error::Version { expected: 11, actual }) if actual == old)
        );
    }
}

#[test]
fn early_hot_message_tags_round_trip_without_expanding_frame_budget() {
    for tag in [
        Tag::HotBegin,
        Tag::HotBegan,
        Tag::HotCancel,
        Tag::HotCancelled,
        Tag::HotObserved,
    ] {
        let payload = [17_u8; 56];
        let frame = Frame::new(tag, Box::from(payload)).expect("bounded early identity");
        let mut wire = Vec::new();
        send(&mut wire, &frame).expect("encode early identity");
        assert_eq!(wire.len(), 4 + 5 + payload.len());
        let decoded = receive(&mut Cursor::new(wire)).expect("decode early identity");
        assert_eq!(decoded.tag(), tag);
        assert_eq!(decoded.payload(), payload);
    }
}
