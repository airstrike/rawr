use super::{Evidence, Fault, Range, Scalar};
use crate::metrics::{self, recorder, NodeKind, Sink as _};

#[test]
fn actual_scoped_observations_distinguish_unknown_zero_and_repeated_products() {
    let recorder = recorder::Recorder::transform_at(10, 1).unwrap();
    {
        let _scope = recorder.scope().unwrap();
        metrics::record_scalar(Scalar::ResolutionMetadataBytes, 0);
        metrics::record_scalar(Scalar::ApiCanonicalBytes, 13);
        metrics::record_scalar(Scalar::ApiCanonicalBytes, 5);
        metrics::record_scalar(Scalar::ApiCanonicalBytes, 8);
        metrics::Current.scalar(Scalar::NodePayloadBytes(NodeKind::Api), 7);
    }
    let evidence = recorder.snapshot().unwrap().accounting;
    assert_eq!(
        evidence
            .scalar(Scalar::ResolutionMetadataBytes)
            .unwrap()
            .total,
        0
    );
    assert_eq!(evidence.scalar(Scalar::ResolutionUnitBytes), None);
    assert_eq!(
        evidence.scalar(Scalar::ApiCanonicalBytes),
        Some(Range {
            count: 3,
            min: 5,
            max: 13,
            total: 26
        })
    );
    assert_eq!(
        evidence
            .scalar(Scalar::NodePayloadBytes(NodeKind::Api))
            .unwrap()
            .count,
        1
    );
}

#[test]
fn scalar_overflow_retains_reached_evidence_without_replacing_product_failure() {
    let recorder = recorder::Recorder::transform_at(10, 1).unwrap();
    let product: Result<(), &str>;
    {
        let _scope = recorder.scope().unwrap();
        product = metrics::executed(NodeKind::Api, || {
            metrics::record_scalar(Scalar::ApiTransactionBytes, u64::MAX);
            metrics::record_scalar(Scalar::ApiTransactionBytes, 1);
            Err("original producer refusal")
        });
    }
    assert_eq!(product, Err("original producer refusal"));
    let snapshot = recorder.snapshot().unwrap();
    assert_eq!(
        snapshot.accounting.fault,
        Some(Fault::ScalarOverflow(Scalar::ApiTransactionBytes))
    );
    assert_eq!(
        snapshot
            .accounting
            .scalar(Scalar::ApiTransactionBytes)
            .unwrap()
            .total,
        u64::MAX
    );
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: recorder::Snapshot = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, snapshot);
}

#[test]
fn wider_quantities_are_checked_and_conversion_overflow_is_sticky() {
    let recorder = recorder::Recorder::transform_at(10, 1).unwrap();
    {
        let _scope = recorder.scope().unwrap();
        metrics::record_scalar_wide(Scalar::ProcessElapsedNanoseconds, 17);
        metrics::record_scalar_wide(Scalar::ProcessElapsedNanoseconds, u128::from(u64::MAX) + 1);
        metrics::record_scalar(Scalar::ProcessElapsedNanoseconds, 23);
    }
    let evidence = recorder.snapshot().unwrap().accounting;
    assert_eq!(
        evidence.fault,
        Some(Fault::ScalarOverflow(Scalar::ProcessElapsedNanoseconds))
    );
    assert_eq!(
        evidence.scalar(Scalar::ProcessElapsedNanoseconds),
        Some(Range {
            count: 1,
            min: 17,
            max: 17,
            total: 17
        })
    );
}

#[test]
fn observation_population_limits_do_not_silently_replace_rows() {
    let recorder = recorder::Recorder::transform_at(1, 1).unwrap();
    recorder.scalar(Scalar::ApiSelectedBytes, 1);
    recorder.scalar(Scalar::ApiSelectedBytes, 1);
    let evidence = recorder.snapshot().unwrap().accounting;
    assert_eq!(
        evidence.fault,
        Some(Fault::ScalarOverflow(Scalar::ApiSelectedBytes))
    );
    assert_eq!(evidence.scalar(Scalar::ApiSelectedBytes).unwrap().count, 1);
}

#[test]
fn malformed_or_missing_serialized_accounting_cannot_be_complete_evidence() {
    let mut evidence = Evidence::default();
    evidence.record_scalar(Scalar::ResolutionRecordBytes, 17, 1);
    let encoded = serde_json::to_vec(&evidence).unwrap();
    let decoded: Evidence = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, evidence);
    let mut value = serde_json::to_value(evidence).unwrap();
    value["scalars"].as_array_mut().unwrap().pop();
    assert!(serde_json::from_value::<Evidence>(value).is_err());
    let mut value = serde_json::to_value(evidence).unwrap();
    let row = value["scalars"][0].clone();
    value["scalars"].as_array_mut().unwrap().push(row);
    assert!(serde_json::from_value::<Evidence>(value).is_err());
    let mut value = serde_json::to_value(evidence).unwrap();
    value["unknown"] = true.into();
    assert!(serde_json::from_value::<Evidence>(value).is_err());
    let mut value = serde_json::to_value(evidence).unwrap();
    value["scalars"][0]["scalar"] = "unknown-producer".into();
    assert!(serde_json::from_value::<Evidence>(value).is_err());
    evidence.scalars.swap(0, 1);
    assert!(!evidence.valid());

    let recorder = recorder::Recorder::transform_at(1, 1).unwrap();
    let mut snapshot = serde_json::to_value(recorder.snapshot().unwrap()).unwrap();
    snapshot.as_object_mut().unwrap().remove("accounting");
    assert!(serde_json::from_value::<recorder::Snapshot>(snapshot).is_err());
}

#[test]
fn semantic_accounting_slots_are_closed_and_prior_shape_is_rejected() {
    use super::SemanticStage;

    let evidence = Evidence::default();
    assert_eq!(evidence.scalars.len(), 83);
    for stage in [
        SemanticStage::ApiTransaction,
        SemanticStage::ApiRetained,
        SemanticStage::Classification,
    ] {
        for scalar in [
            Scalar::SemanticRefusals(stage),
            Scalar::SemanticRefusedRequired(stage),
            Scalar::SemanticRefusedLimit(stage),
        ] {
            assert_eq!(
                evidence
                    .scalars
                    .iter()
                    .filter(|row| row.scalar == scalar)
                    .count(),
                1
            );
            assert_eq!(evidence.scalar(scalar), None);
        }
    }
    let mut prior = serde_json::to_value(evidence).unwrap();
    prior["scalars"].as_array_mut().unwrap().truncate(73);
    assert!(serde_json::from_value::<Evidence>(prior).is_err());
}

#[test]
fn complete_fixed_schema_payload_has_bounded_storage_and_serialization() {
    use crate::protocol::account;

    // Width stress, not a qualifying observation: every optional field is present
    // and every integer uses its maximum decimal width. Some simultaneous maxima
    // cannot be reached by a valid history. This deliberately bounds serialization
    // without mistaking a successful fixture for the exact largest valid report.
    let mut evidence = Evidence::default();
    for row in &mut evidence.scalars {
        row.observed = Some(Range {
            count: u64::MAX,
            min: u64::MAX,
            max: u64::MAX,
            total: u64::MAX,
        });
    }
    evidence.native.fill(Some(super::NativeRow {
        runner_pid: u32::MAX,
        epoch: u64::MAX,
        observations: u64::MAX,
        commands: u64::MAX,
        simultaneous: u8::MAX,
        snapshot: Some(account::Snapshot {
            account: std::num::NonZeroU64::new(u64::MAX),
            current: [u64::MAX; 2],
            peak: [u64::MAX; 2],
            limits: [u64::MAX; 2],
            first_refusal: Some(account::Refusal {
                axis: account::Axis::Bytes,
                cause: account::Cause::AggregateOverflow,
                current: [u64::MAX; 2],
                requested: Some(u64::MAX),
                limit: u64::MAX,
            }),
        }),
    }));
    evidence.fault = Some(Fault::Native {
        runner_pid: u32::MAX,
        epoch: u64::MAX,
        cause: super::NativeFault::Inconsistent,
    });

    assert_eq!(evidence.scalars.len(), 83);
    assert_eq!(evidence.native.len(), 32);
    // The full inline payload owns no secondary allocation. These fixed schema
    // terms are regression expectations, not production resource limits. Keep
    // their component bounds explicit so boxing cannot disguise retained bytes.
    assert!(!std::mem::needs_drop::<Evidence>());
    for (name, bytes, bound) in [
        ("scalar-row", std::mem::size_of::<super::ScalarRow>(), 48),
        (
            "native-row",
            std::mem::size_of::<Option<super::NativeRow>>(),
            136,
        ),
        ("fault", std::mem::size_of::<Option<Fault>>(), 16),
    ] {
        assert!(bytes <= bound, "{name}={bytes}, bound={bound}");
    }
    let retained_bound = 83 * 48 + 32 * 136 + 16;

    // Compact JSON: 20 digits per u64, 10 per u32, 3 per u8, quoted closed
    // tags/field names, braces and separators. The largest scalar tag is
    // semantic-refused-required(api-transaction). All 83 concrete tags are
    // checked below. 256 covers top-level names/framing and the row commas.
    let encoded_bound = 256 + 83 * 183 + 32 * 521 + 88;
    for row in &evidence.scalars {
        assert!(
            json_size(row) <= 183,
            "scalar {:?}: {} bytes",
            row.scalar,
            json_size(row)
        );
    }
    for row in evidence.native.iter().flatten() {
        assert!(json_size(row) <= 521, "native: {} bytes", json_size(row));
    }
    assert!(json_size(&evidence.fault) <= 88);

    let retained_bytes = std::mem::size_of::<Evidence>();
    let encoded = serde_json::to_vec(&evidence).unwrap();
    let encoded_bytes = encoded.len();
    assert!(
        retained_bytes <= retained_bound,
        "retained={retained_bytes}, bound={retained_bound}, encoded={encoded_bytes}, scalar-row={}, native-row={}",
        std::mem::size_of::<super::ScalarRow>(),
        std::mem::size_of::<Option<super::NativeRow>>()
    );
    assert!(
        encoded_bytes <= encoded_bound,
        "retained={retained_bytes}, encoded={encoded_bytes}, bound={encoded_bound}"
    );
    assert_eq!(
        serde_json::from_slice::<Evidence>(&encoded).unwrap(),
        evidence
    );
}

fn json_size(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).unwrap().len()
}

#[test]
fn no_scope_keeps_accounting_evidence_empty() {
    metrics::record_scalar(Scalar::ResolutionRecordBytes, 99);
    let recorder = recorder::Recorder::transform_at(1, 1).unwrap();
    assert_eq!(
        *recorder.snapshot().unwrap().accounting,
        Evidence::default()
    );
}

fn native(
    identity: u64,
    tag: crate::protocol::Tag,
    current: [u64; 2],
) -> crate::protocol::account::Observation {
    use crate::protocol::account;
    account::Observation {
        request: account::Request::from_parts(tag, &[]).unwrap(),
        accounts: [
            Some(account::Snapshot {
                account: std::num::NonZeroU64::new(identity),
                current,
                peak: current,
                limits: [u64::MAX; 2],
                first_refusal: None,
            }),
            None,
        ],
    }
}

#[test]
fn native_lifecycle_coverage_preserves_account_identity_peaks_and_overlap() {
    use crate::protocol::{account, Tag};
    let recorder = recorder::Recorder::transform_at(10, 1).unwrap();
    {
        let _scope = recorder.scope().unwrap();
        let mut prepared = native(1, Tag::Prepare, [5, 10]);
        prepared.accounts[1] = native(2, Tag::Prepare, [7, 20]).accounts[0];
        metrics::record_native(10, 1, prepared);
        metrics::record_native(10, 1, native(1, Tag::Admit, [3, 4]));
        metrics::record_native(10, 2, native(1, Tag::Prepare, [1, 2]));
        metrics::record_native(20, 1, native(1, Tag::Prepare, [1, 2]));
        metrics::record_native(
            10,
            1,
            account::Observation {
                request: account::Request::from_parts(Tag::Shutdown, &[]).unwrap(),
                accounts: [None; 2],
            },
        );
    }
    let evidence = recorder.snapshot().unwrap().accounting;
    assert_eq!(evidence.native.iter().flatten().count(), 5);
    let first = evidence.native[0].unwrap();
    assert_eq!(first.observations, 2);
    assert_eq!(first.simultaneous, 2);
    assert_eq!(first.snapshot.unwrap().current, [3, 4]);
    assert_eq!(first.snapshot.unwrap().peak, [5, 10]);
    assert!(evidence.native_covers(10, 1, &[Tag::Prepare, Tag::Admit, Tag::Shutdown]));
    assert!(!evidence.native_covers(10, 2, &[Tag::Admit]));
    assert!(!evidence.native_covers(10, 1, &[Tag::Start]));
}

#[test]
fn native_refusal_survives_scope_retirement_and_bad_telemetry_cannot_qualify() {
    use crate::protocol::{account, Tag};
    let mut evidence = Evidence::default();
    let mut observed = native(1, Tag::Prepare, [5, 10]);
    let exceedance = account::Refusal {
        axis: account::Axis::Bytes,
        cause: account::Cause::AggregateOverflow,
        current: [5, 10],
        requested: None,
        limit: u64::MAX,
    };
    observed.accounts[0].as_mut().unwrap().first_refusal = Some(exceedance);
    evidence.record_native(10, 1, observed, 10);
    evidence.record_native(10, 1, native(1, Tag::Shutdown, [0, 0]), 10);
    assert_eq!(
        evidence.native[0].unwrap().snapshot.unwrap().first_refusal,
        Some(exceedance)
    );
    assert_eq!(
        evidence.native[0].unwrap().snapshot.unwrap().current,
        [0, 0]
    );
    assert_eq!(evidence.native[0].unwrap().snapshot.unwrap().peak, [5, 10]);
    assert!(evidence.valid());
    evidence.record_native_fault(10, 1, super::NativeFault::Missing);
    evidence.record_native_fault(10, 1, super::NativeFault::Malformed);
    assert_eq!(
        evidence.fault,
        Some(Fault::Native {
            runner_pid: 10,
            epoch: 1,
            cause: super::NativeFault::Missing
        })
    );
    assert!(!evidence.native_covers(10, 1, &[Tag::Prepare]));
    assert_eq!(
        serde_json::from_slice::<Evidence>(&serde_json::to_vec(&evidence).unwrap()).unwrap(),
        evidence
    );
}

#[test]
fn native_population_limit_is_atomic_for_two_simultaneous_accounts() {
    use crate::protocol::Tag;
    let mut evidence = Evidence::default();
    for identity in 1..super::NATIVE_ACCOUNTS as u64 {
        evidence.record_native(10, 1, native(identity, Tag::Prepare, [0; 2]), 10);
    }
    let mut observation = native(super::NATIVE_ACCOUNTS as u64, Tag::Prepare, [0; 2]);
    observation.accounts[1] =
        native(super::NATIVE_ACCOUNTS as u64 + 1, Tag::Prepare, [0; 2]).accounts[0];
    evidence.record_native(10, 1, observation, 10);
    assert_eq!(
        evidence.native.iter().flatten().count(),
        super::NATIVE_ACCOUNTS - 1
    );
    assert_eq!(
        evidence.fault,
        Some(Fault::Native {
            runner_pid: 10,
            epoch: 1,
            cause: super::NativeFault::Capacity
        })
    );

    let mut evidence = Evidence::default();
    evidence.record_native(10, 1, native(0, Tag::Prepare, [3, 4]), 10);
    assert!(evidence.native[0].unwrap().snapshot.is_some());
    assert!(evidence.valid());
    assert_eq!(
        evidence.fault,
        Some(Fault::Native {
            runner_pid: 10,
            epoch: 1,
            cause: super::NativeFault::Identity
        })
    );
    evidence.fault = None;
    assert!(!evidence.valid());
}
