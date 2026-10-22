use super::{Axis, Evidence, Exceedance, Fault, Graph, Range, Scalar, Stage, GRAPH_ACCOUNTS};
use crate::metrics::{self, recorder, NodeKind, Sink as _};

fn graph(account: u64, admitted: [u64; 6]) -> Graph {
    Graph {
        account,
        stage: Stage::Node(NodeKind::Resolution),
        admitted,
        exceedance: None,
    }
}

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
        metrics::record_graph(graph(1, [0; 6]));
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
    assert!(evidence.qualifies(&[Scalar::ResolutionMetadataBytes, Scalar::ApiCanonicalBytes]));
    assert!(!evidence.qualifies(&[Scalar::ResolutionUnitBytes]));
    assert!(!Evidence::default().qualifies(&[]));
}

#[test]
fn graph_clones_share_a_row_and_late_observations_do_not_reset_high_water() {
    let recorder = recorder::Recorder::transform_at(10, 1).unwrap();
    recorder.graph(graph(7, [1, 2, 3, 4, 5, 6]));
    let exceedance = Exceedance {
        axis: Axis::Paths,
        requested: Some(9),
        limit: 8,
    };
    recorder.graph(Graph {
        account: 7,
        stage: Stage::Charge(Axis::Paths),
        admitted: [2, 3, 4, 5, 8, 9],
        exceedance: Some(exceedance),
    });
    recorder.graph(graph(7, [0, 1, 2, 3, 4, 5]));
    recorder.graph(Graph {
        account: 7,
        stage: Stage::Admission,
        admitted: [2, 3, 4, 5, 8, 9],
        exceedance: Some(Exceedance {
            axis: Axis::Paths,
            requested: Some(10),
            limit: 8,
        }),
    });
    recorder.graph(graph(9, [0; 6]));
    let evidence = recorder.snapshot().unwrap().accounting;
    let row = evidence.graphs[0].unwrap();
    assert_eq!(row.account, 7);
    assert_eq!(row.observations, 4);
    assert_eq!(row.high_water, [2, 3, 4, 5, 8, 9]);
    assert_eq!(row.last_stage, Stage::Admission);
    assert_eq!(row.exceedances, 2);
    assert_eq!(row.first_exceedance.unwrap().exceedance, exceedance);
    assert_eq!(row.first_exceedance.unwrap().admitted, [2, 3, 4, 5, 8, 9]);
    assert_eq!(evidence.graphs[1].unwrap().account, 9);
    assert!(evidence.valid());
    // A governed miss may handle a refusal. Metrics do not infer a product result.
    assert!(evidence.qualifies(&[]));
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
    assert!(!snapshot
        .accounting
        .qualifies(&[Scalar::ApiTransactionBytes]));
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
    assert!(!evidence.qualifies(&[Scalar::ProcessElapsedNanoseconds]));
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

    let recorder = recorder::Recorder::transform_at(1, 1).unwrap();
    recorder.graph(graph(1, [0; 6]));
    recorder.graph(graph(1, [1; 6]));
    let evidence = recorder.snapshot().unwrap().accounting;
    assert_eq!(evidence.fault, Some(Fault::GraphOverflow(1)));
    assert_eq!(evidence.graphs[0].unwrap().high_water, [0; 6]);
}

#[test]
fn graph_capacity_is_fixed_and_arithmetic_refusal_keeps_unknown_required_amount() {
    let mut evidence = Evidence::default();
    for account in 1..=GRAPH_ACCOUNTS as u64 {
        evidence.record_graph(graph(account, [0; 6]), u64::MAX);
    }
    evidence.record_graph(graph(GRAPH_ACCOUNTS as u64 + 1, [0; 6]), u64::MAX);
    assert_eq!(evidence.fault, Some(Fault::GraphCapacity));
    assert_eq!(evidence.graphs.iter().flatten().count(), GRAPH_ACCOUNTS);
    assert!(evidence.valid());
    assert!(!evidence.qualifies(&[]));

    let mut evidence = Evidence::default();
    evidence.record_graph(
        Graph {
            account: 1,
            stage: Stage::Charge(Axis::Hashed),
            admitted: [0; 6],
            exceedance: Some(Exceedance {
                axis: Axis::Hashed,
                requested: None,
                limit: u64::MAX,
            }),
        },
        1,
    );
    assert!(evidence.valid());
    assert_eq!(
        evidence.graphs[0]
            .unwrap()
            .first_exceedance
            .unwrap()
            .exceedance
            .requested,
        None
    );
}

#[test]
fn malformed_or_missing_serialized_accounting_cannot_be_complete_evidence() {
    let mut evidence = Evidence::default();
    evidence.record_scalar(Scalar::ResolutionRecordBytes, 17, 1);
    evidence.record_graph(graph(1, [0; 6]), 1);
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
    value["graphs"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::Value::Null);
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
fn invalid_identity_or_refusal_marks_evidence_incomplete() {
    for observation in [
        graph(0, [0; 6]),
        Graph {
            exceedance: Some(Exceedance {
                axis: Axis::Paths,
                requested: Some(7),
                limit: 8,
            }),
            ..graph(1, [0; 6])
        },
        // An admitted amount above its target is the recorded exceedance
        // state, so only a request that did not cross the target is invalid.
        Graph {
            exceedance: Some(Exceedance {
                axis: Axis::Paths,
                requested: Some(8),
                limit: 8,
            }),
            ..graph(1, [9; 6])
        },
    ] {
        let mut evidence = Evidence::default();
        evidence.record_graph(observation, 1);
        assert_eq!(
            evidence.fault,
            Some(Fault::InvalidGraph(observation.account))
        );
        assert!(!evidence.qualifies(&[]));
    }
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
    // Node(native-product), comparisons, aggregate-overflow and inconsistent
    // are the longest serialized tags in their respective variable positions.
    let stage = Stage::Node(NodeKind::NativeProduct);
    evidence.graphs.fill(Some(super::GraphRow {
        account: u64::MAX,
        observations: u64::MAX,
        first_stage: stage,
        last_stage: stage,
        high_water: [u64::MAX; 6],
        exceedances: u64::MAX,
        first_exceedance: Some(super::Exceeded {
            stage,
            admitted: [u64::MAX; 6],
            exceedance: Exceedance {
                axis: Axis::Comparisons,
                requested: Some(u64::MAX),
                limit: u64::MAX,
            },
        }),
    }));
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
    assert_eq!(evidence.graphs.len(), 32);
    assert_eq!(evidence.native.len(), 32);
    // The full inline payload owns no secondary allocation. These fixed schema
    // terms are regression expectations, not production resource limits. Keep
    // their component bounds explicit so boxing cannot disguise retained bytes.
    assert!(!std::mem::needs_drop::<Evidence>());
    for (name, bytes, bound) in [
        ("scalar-row", std::mem::size_of::<super::ScalarRow>(), 48),
        (
            "graph-row",
            std::mem::size_of::<Option<super::GraphRow>>(),
            168,
        ),
        (
            "native-row",
            std::mem::size_of::<Option<super::NativeRow>>(),
            136,
        ),
        ("fault", std::mem::size_of::<Option<Fault>>(), 16),
    ] {
        assert!(bytes <= bound, "{name}={bytes}, bound={bound}");
    }
    let retained_bound = 83 * 48 + 32 * 168 + 32 * 136 + 16;

    // Compact JSON: 20 digits per u64, 10 per u32, 3 per u8, quoted closed
    // tags/field names, braces and separators. The largest scalar tag is
    // semantic-refused-required(api-transaction). All 83 concrete tags are
    // checked below. 256 covers top-level names/framing and 156 row commas.
    // The graph row's `refusals`/`first_refusal`/`refusal` fields
    // were renamed to `exceedances`/`first_exceedance`/`exceedance`: nine more
    // bytes per row.
    let encoded_bound = 256 + 83 * 183 + 32 * 615 + 32 * 521 + 88;
    for row in &evidence.scalars {
        assert!(
            json_size(row) <= 183,
            "scalar {:?}: {} bytes",
            row.scalar,
            json_size(row)
        );
    }
    for row in evidence.graphs.iter().flatten() {
        assert!(json_size(row) <= 615, "graph: {} bytes", json_size(row));
    }
    for row in evidence.native.iter().flatten() {
        assert!(json_size(row) <= 521, "native: {} bytes", json_size(row));
    }
    assert!(json_size(&evidence.fault) <= 88);

    #[cfg(feature = "import-baseline")]
    let (retained_bound, encoded_bound) = {
        use metrics::{bundle, publication};

        let target =
            metrics::pipeline::target::Target::observe(crate::store::bundle::BundleRef::new(
                crate::node::ExecutionKey::from_bytes([u8::MAX; 32]),
                crate::store::bundle::BundleId::from_bytes([u8::MAX; 32]),
            ));
        let bundle = bundle::Row {
            target: Some(target),
            allocated: u64::MAX,
            initialized: u64::MAX,
            initialization_failed: u64::MAX,
            seal_started: u64::MAX,
            sealed: u64::MAX,
            publication_started: u64::MAX,
            published: u64::MAX,
            retained: u64::MAX,
            cleanup_started: u64::MAX,
            thaw_failed: u64::MAX,
            removed: u64::MAX,
            removal_failed: u64::MAX,
        };
        evidence.bundles = Some(bundle::Evidence {
            resolution: bundle,
            runner: bundle,
            fault: Some(bundle::Fault::Overflow),
        });
        let counts = publication::Counts {
            started: u64::MAX,
            created: u64::MAX,
            adopted: u64::MAX,
            renamed: u64::MAX,
            existing: u64::MAX,
            lost: u64::MAX,
            operation_failed: u64::MAX,
            cleanup_started: u64::MAX,
            cleaned: u64::MAX,
            cleanup_failed: u64::MAX,
            returned: u64::MAX,
            unresolved: u64::MAX,
            anomaly: u64::MAX,
        };
        let mut history = publication::Evidence::default();
        assert_eq!(history.nodes.len(), 11);
        assert_eq!(history.pins.len(), 4);
        assert_eq!(history.replacements.len(), 13);
        assert_eq!(history.retirements.len(), 7);
        for row in &mut history.nodes {
            row.action = counts;
            row.content = counts;
            row.index = counts;
        }
        for row in &mut history.pins {
            row.counts = counts;
            row.released = counts;
            row.stale = counts;
        }
        for row in &mut history.replacements {
            row.counts = counts;
            row.retired = counts;
        }
        for row in &mut history.retirements {
            row.counts = counts;
        }
        history.recovered_files = counts;
        history.recovered_directories = counts;
        history.fault = Some(publication::Fault::Overflow);
        evidence.publications = Some(history);

        for (name, bytes, bound) in [
            (
                "bundles",
                std::mem::size_of::<Option<bundle::Evidence>>(),
                344,
            ),
            (
                "publications",
                std::mem::size_of::<Option<publication::Evidence>>(),
                8_608,
            ),
        ] {
            assert!(bytes <= bound, "{name}={bytes}, bound={bound}");
        }
        // Two bundle rows each contain a 64-byte target rendered as u8 arrays
        // and twelve u64 counters. The 80 publication count rows each render
        // thirteen u64s in at most 430 bytes; 2,048 covers all closed family
        // labels, member names, arrays and separators (currently 1,811 bytes).
        assert_eq!(json_size(&counts), 430);
        assert!(json_size(&evidence.bundles) <= 1_490);
        assert!(json_size(&evidence.publications) <= 80 * 430 + 2_048);
        (
            retained_bound + 344 + 8_608,
            encoded_bound + 1_490 + 80 * 430 + 2_048,
        )
    };

    let retained_bytes = std::mem::size_of::<Evidence>();
    let encoded = serde_json::to_vec(&evidence).unwrap();
    let encoded_bytes = encoded.len();
    assert!(
        retained_bytes <= retained_bound,
        "retained={retained_bytes}, bound={retained_bound}, encoded={encoded_bytes}, scalar-row={}, graph-row={}, native-row={}",
        std::mem::size_of::<super::ScalarRow>(),
        std::mem::size_of::<Option<super::GraphRow>>(),
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
    metrics::record_graph(graph(99, [99; 6]));
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
