#[test]
fn snapshot_transport_rejects_more_than_one_bounded_frame() {
    let oversized = vec![0_u8; rawr_import::protocol::FRAME_BYTES].into_boxed_slice();
    assert!(rawr_import::supervisor::snapshot::Bytes::decode(
        rawr_import::surface::Snapshot::from_rawr_digest([0; 32]),
        oversized,
        &rawr_import::cargo::Limits::standard(),
    )
    .is_err());
}
