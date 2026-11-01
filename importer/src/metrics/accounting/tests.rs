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
    evidence.fault = Some(Fault::ScalarOverflow(Scalar::SemanticRefusedRequired(
        super::SemanticStage::ApiTransaction,
    )));

    assert_eq!(evidence.scalars.len(), 83);
    // The full inline payload owns no secondary allocation. These fixed schema
    // terms are regression expectations, not production resource limits. Keep
    // their component bounds explicit so boxing cannot disguise retained bytes.
    assert!(!std::mem::needs_drop::<Evidence>());
    for (name, bytes, bound) in [
        ("scalar-row", std::mem::size_of::<super::ScalarRow>(), 48),
        ("fault", std::mem::size_of::<Option<Fault>>(), 16),
    ] {
        assert!(bytes <= bound, "{name}={bytes}, bound={bound}");
    }
    let retained_bound = 83 * 48 + 16;

    // Compact JSON: 20 digits per u64, 10 per u32, 3 per u8, quoted closed
    // tags/field names, braces and separators. The largest scalar tag is
    // semantic-refused-required(api-transaction). All 83 concrete tags are
    // checked below. 256 covers top-level names/framing and the row commas.
    let encoded_bound = 256 + 83 * 183 + 88;
    for row in &evidence.scalars {
        assert!(
            json_size(row) <= 183,
            "scalar {:?}: {} bytes",
            row.scalar,
            json_size(row)
        );
    }
    assert!(json_size(&evidence.fault) <= 88);

    let retained_bytes = std::mem::size_of::<Evidence>();
    let encoded = serde_json::to_vec(&evidence).unwrap();
    let encoded_bytes = encoded.len();
    assert!(
        retained_bytes <= retained_bound,
        "retained={retained_bytes}, bound={retained_bound}, encoded={encoded_bytes}, scalar-row={}",
        std::mem::size_of::<super::ScalarRow>(),
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
