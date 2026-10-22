use super::*;

#[test]
fn evidence_keeps_original_usage_without_retaining_graph_scratch() {
    let graph = GraphBudget::new();
    let marker = std::sync::Arc::new(());
    let weak = std::sync::Arc::downgrade(&marker);
    let key = node::account::Key::new().unwrap();
    key.seed(&graph, marker).unwrap();
    let usage = node::account::evidence::Usage::retain(&graph);
    drop(graph);
    assert!(weak.upgrade().is_none());
    let (charge, observation) = usage.charge(7);
    charge.unwrap();
    assert!(observation.unwrap().admitted[3] >= 7);
}

#[test]
fn evidence_frame_refusal_is_sticky_and_original_account_exceedance_records() {
    let graph = GraphBudget::new();
    let observation = Observation {
        event: Ok(Event::Renamed { attempt: 1 }),
        budget: Account::Graph(&graph),
    };
    let mut history = History::default();
    history.wire_bytes = crate::metrics::cell::REPORT_BYTES;
    history.record(&observation);
    assert_eq!(history.fault, Some(Fault::Frame));
    assert!(history.events.is_empty());
    history.wire_bytes = 2;
    history.record(&observation);
    assert!(history.events.is_empty());

    // The decoded target is accounting. An evidence charge past it is
    // recorded on the original graph account and the event is retained.
    let mut history = History::default();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .unwrap();
    history.record(&observation);
    assert_eq!(history.fault, None);
    assert_eq!(history.events.len(), 1);
    let axis = *graph
        .exceedances()
        .axis(node::DECODED_RESOURCE)
        .expect("the decoded crossing is recorded");
    assert_eq!(axis.target, node::MAX_DECODED_BYTES_PER_GRAPH);
    assert!(axis.high_water > node::MAX_DECODED_BYTES_PER_GRAPH);
}

#[test]
fn snapshot_clones_share_rows_and_late_transport_charges_are_retained() {
    let graph = GraphBudget::new();
    let mut history = History::default();
    history.record(&Observation {
        event: Ok(Event::Renamed { attempt: 1 }),
        budget: Account::Graph(&graph),
    });
    let (mut evidence, copied) = history.snapshot();
    let copy = evidence.clone();
    assert!(std::sync::Arc::ptr_eq(
        evidence.events.rows.as_ref().unwrap(),
        copy.events.rows.as_ref().unwrap()
    ));
    assert_eq!(
        serde_json::to_vec(&evidence.events).unwrap().len(),
        evidence.events.wire_bytes
    );
    let mut accounting = super::super::accounting::Evidence::default();
    accounting.record_graph(copied.unwrap(), 100);
    let before_transport = graph.census_vector().unwrap();
    evidence
        .prepare_cell_transport(&mut accounting, 100)
        .unwrap();
    let usage = graph.census_vector().unwrap();
    assert_eq!(
        usage[3] - before_transport[3],
        2 * evidence.events.wire_bytes as u64
    );
    assert_eq!(
        usage[5] - before_transport[5],
        evidence.events.wire_bytes as u64
    );
    assert!(accounting
        .graphs
        .iter()
        .flatten()
        .any(|row| row.high_water == usage));
    let encoded = serde_json::to_vec(&evidence).unwrap();
    assert_eq!(encoded, serde_json::to_vec(&evidence).unwrap());
    assert_eq!(
        usage,
        graph.census_vector().unwrap(),
        "pure encoding does not spend a hidden new account"
    );
}

#[test]
fn a_second_original_account_cannot_take_over_the_history() {
    let first = GraphBudget::new();
    let same = first.clone();
    let second = GraphBudget::new();
    let mut history = History::default();
    for budget in [&first, &same] {
        history.record(&Observation {
            event: Ok(Event::Renamed { attempt: 1 }),
            budget: Account::Graph(budget),
        });
    }
    assert!(history.fault.is_none());
    let second_before = second.census_vector().unwrap();
    history.record(&Observation {
        event: Ok(Event::Renamed { attempt: 2 }),
        budget: Account::Graph(&second),
    });
    assert_eq!(history.fault, Some(Fault::Account));
    assert_eq!(history.events.len(), 2);
    assert_eq!(second.census_vector().unwrap(), second_before);
    assert!(history.budget.as_ref().unwrap().shares_with(&first));
}

#[test]
fn transport_hash_exceedance_is_recorded_and_transport_continues() {
    let graph = GraphBudget::new();
    let mut history = History::default();
    history.record(&Observation {
        event: Ok(Event::Renamed { attempt: 1 }),
        budget: Account::Graph(&graph),
    });
    let (mut evidence, copied) = history.snapshot();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    let before = graph.census_vector().unwrap();
    let mut accounting = super::super::accounting::Evidence::default();
    accounting.record_graph(copied.unwrap(), 100);
    // The hashed target is an accounting target; crossing it records the
    // axis on the original graph account and the transport still completes.
    assert_eq!(
        evidence.prepare_cell_transport(&mut accounting, 100),
        Ok(())
    );
    assert!(evidence.cell_transport);
    assert_eq!(
        graph.census_vector().unwrap()[3] - before[3],
        2 * evidence.events.wire_bytes as u64
    );
    let exceeded = accounting
        .graphs
        .iter()
        .flatten()
        .find_map(|row| row.first_exceedance)
        .unwrap();
    assert_eq!(
        exceeded.exceedance.axis,
        super::super::accounting::Axis::Hashed
    );
    assert_eq!(
        exceeded.exceedance.requested,
        Some(node::MAX_HASHED_BYTES_PER_GRAPH + evidence.events.wire_bytes as u64)
    );
    let record = graph.exceedances();
    let axis = record.axis(node::HASHED_RESOURCE).unwrap();
    assert_eq!(axis.target, node::MAX_HASHED_BYTES_PER_GRAPH);
    assert_eq!(
        axis.high_water,
        node::MAX_HASHED_BYTES_PER_GRAPH + evidence.events.wire_bytes as u64
    );
}

#[test]
fn inactive_observation_does_not_spend_the_graph() {
    let graph = GraphBudget::new();
    let before = graph.census_vector().unwrap();
    let identity = Identity::observe(
        Path::new("/store"),
        Path::new("/store/content"),
        node::Kind::Api,
        1,
        node::ActionKey::from_bytes([1; 32]),
        node::Digest::from_bytes([2; 32]),
        Target::Index { lookup: [3; 32] },
        &graph,
    );
    let attempt = Attempt::start(identity, &graph);
    attempt.row(b"unused row");
    attempt.sealed(
        Commitment {
            bytes: 1,
            sha256: [4; 32],
        },
        Some(b"unused allocation"),
    );
    attempt.renamed();
    drop(attempt);
    assert_eq!(before, graph.census_vector().unwrap());
}
