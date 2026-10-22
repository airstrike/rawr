use std::io::Cursor;

use rawr_import::protocol::{self, Frame, Tag};
use sha2::{Digest as _, Sha256};

#[test]
fn bounded_frame_round_trips_without_stdio_semantics() {
    let frame =
        Frame::new(Tag::Prepare, Box::from(*b"complete-key")).expect("the small frame is bounded");
    let mut bytes = Vec::new();
    protocol::send(&mut bytes, &frame).expect("the complete frame writes");
    let decoded = protocol::receive(&mut Cursor::new(bytes)).expect("the frame decodes");
    assert_eq!(decoded.tag(), Tag::Prepare);
    assert_eq!(decoded.payload(), b"complete-key");
}

#[test]
fn oversized_and_partial_frames_fail_before_partial_authority() {
    let announced = u32::try_from(protocol::FRAME_BYTES + 1)
        .expect("the governed limit fits the wire prefix")
        .to_be_bytes();
    assert!(matches!(
        protocol::receive(&mut Cursor::new(announced)),
        Err(protocol::Error::Oversized { .. })
    ));

    let mut partial = Vec::from(9_u32.to_be_bytes());
    partial.extend_from_slice(&1_u32.to_be_bytes());
    assert!(matches!(
        protocol::receive(&mut Cursor::new(partial)),
        Err(protocol::Error::Truncated)
    ));

    let mut wrong_version = Vec::from(5_u32.to_be_bytes());
    wrong_version.extend_from_slice(&2_u32.to_be_bytes());
    wrong_version.push(Tag::Shutdown as u8);
    assert!(matches!(
        protocol::receive(&mut Cursor::new(wrong_version)),
        Err(protocol::Error::Version {
            expected: protocol::VERSION,
            actual: 2
        })
    ));

    let mut unknown_tag = Vec::from(5_u32.to_be_bytes());
    unknown_tag.extend_from_slice(&protocol::VERSION.to_be_bytes());
    unknown_tag.push(u8::MAX);
    assert!(matches!(
        protocol::receive(&mut Cursor::new(unknown_tag)),
        Err(protocol::Error::Tag(u8::MAX))
    ));

    assert!(matches!(
        protocol::receive(&mut Cursor::new(4_u32.to_be_bytes())),
        Err(protocol::Error::Truncated)
    ));
    assert!(matches!(
        Frame::new(
            Tag::Report,
            vec![0; protocol::FRAME_BYTES].into_boxed_slice()
        ),
        Err(protocol::Error::Oversized { .. })
    ));
}

#[test]
fn snapshot_transport_binds_exact_frames_and_rejects_drift_or_bad_shape() {
    let limits = rawr_import::cargo::Limits::standard();
    let mut expected = rawr_import::surface::Snapshot::builder(&limits);
    expected.entry("application.rawr").expect("entry");
    expected.file("application.rawr", b"exact").expect("file");
    let expected = expected.seal().expect("snapshot");

    let mut capture = rawr_import::supervisor::snapshot::Bytes::capture(&limits);
    capture.entry("application.rawr").expect("entry frame");
    capture
        .file("application.rawr", b"exact")
        .expect("file frame");
    capture.bind(expected, &limits).expect("exact digest binds");

    let mut drifted = rawr_import::supervisor::snapshot::Bytes::capture(&limits);
    drifted.entry("application.rawr").expect("entry frame");
    drifted
        .file("application.rawr", b"drifted")
        .expect("file frame");
    assert!(matches!(
        drifted.bind(expected, &limits),
        Err(rawr_import::supervisor::snapshot::Error::Mismatch)
    ));

    let mut entry_drift = rawr_import::supervisor::snapshot::Bytes::capture(&limits);
    entry_drift.entry("other.rawr").expect("other entry frame");
    entry_drift
        .file("application.rawr", b"exact")
        .expect("exact file frame");
    assert!(matches!(
        entry_drift.bind(expected, &limits),
        Err(rawr_import::supervisor::snapshot::Error::Mismatch)
    ));

    let mut path_drift = rawr_import::supervisor::snapshot::Bytes::capture(&limits);
    path_drift
        .entry("application.rawr")
        .expect("exact entry frame");
    path_drift
        .file("other.rawr", b"exact")
        .expect("other file frame");
    assert!(matches!(
        path_drift.bind(expected, &limits),
        Err(rawr_import::supervisor::snapshot::Error::Mismatch)
    ));

    let mut unordered = rawr_import::supervisor::snapshot::Bytes::capture(&limits);
    unordered.entry("application.rawr").expect("entry frame");
    unordered.file("b.rawr", b"b").expect("first file");
    assert!(matches!(
        unordered.file("a.rawr", b"a"),
        Err(rawr_import::supervisor::snapshot::Error::Order)
    ));

    let duplicate = snapshot_frames(&[("a.rawr", b"a"), ("a.rawr", b"b")]);
    let digest: [u8; 32] = Sha256::digest(&duplicate).into();
    assert!(matches!(
        rawr_import::supervisor::snapshot::Bytes::decode(
            rawr_import::surface::Snapshot::from_rawr_digest(digest),
            duplicate.into_boxed_slice(),
            &limits,
        ),
        Err(rawr_import::supervisor::snapshot::Error::Order)
    ));
    assert!(matches!(
        rawr_import::supervisor::snapshot::Bytes::decode(
            rawr_import::surface::Snapshot::from_rawr_digest([0; 32]),
            Box::from(*b"malformed"),
            &limits,
        ),
        Err(rawr_import::supervisor::snapshot::Error::Malformed)
    ));

    let mut oversized = rawr_import::supervisor::snapshot::Bytes::capture(&limits);
    oversized
        .entry("application.rawr")
        .expect("the bounded entry fits");
    assert!(matches!(
        oversized.file("application.rawr", &vec![0; protocol::FRAME_BYTES]),
        Err(rawr_import::supervisor::snapshot::Error::Oversized { .. })
    ));
}

fn snapshot_frames(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::from(b"rawr.snapshot.entry/v1".as_slice());
    push(&mut bytes, b"application.rawr");
    for (path, contents) in files {
        push(&mut bytes, path.as_bytes());
        push(&mut bytes, contents);
    }
    bytes
}

fn push(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_le_bytes());
    output.extend_from_slice(value);
}
