use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use super::*;

const COMMIT: &str = "375b1431b7d89d1c2e2bc168c011848ae12b7d14";
const ARGUMENTS: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn production_canonical_limits_and_a_latched_account() {
    let limits = cargo::Limits::standard();
    assert_eq!(limits.item_limit(), 2_500_000);
    assert_eq!(limits.byte_limit(), 33_554_432);
    assert_eq!(limits.retained_graph_items, 6_500_000);
    assert_eq!(limits.retained_graph_bytes, 536_870_912);
    assert_eq!(limits.classified_contract_items, 6_500_000);
    assert_eq!(limits.classified_contract_bytes, 671_088_640);
    // Terminal only through an injected genuine cause: a latched account
    // fails every charge first and does no further work.
    let mut refused = Allocation::refused();
    assert_refused(refused.items(1));
    assert_refused(refused.bytes(1));
    assert!(refused.is_refused());
    assert_eq!(refused.usage(), (0, 0), "a latched account charges nothing");
}

#[test]
fn canonical_evidence_epoch_descends_from_capture_authority() {
    let limits = cargo::Limits::standard();
    let evidence = Allocation::new(&limits)
        .into_evidence(&limits, protocol::Population::default())
        .expect("normal capture authority transforms into evidence authority");
    assert_eq!(limits.canonical_items, 19_000_000);
    assert_eq!(limits.canonical_bytes, 1_207_959_552);
    assert_eq!(evidence.byte_limit, CANONICAL_EVIDENCE_BYTES);
    assert_eq!(evidence.string_limit, limits.string_bytes);

    // A latched account stays terminal across the evidence transformation.
    let mut latched = Allocation::new(&limits);
    latched.refuse();
    let mut latched = latched
        .into_evidence(&limits, protocol::Population::default())
        .expect("the evidence transformation checks authority, not the latch");
    assert!(latched.is_refused());
    assert_refused(latched.items(1));
    assert_refused(latched.bytes(1));
}

#[test]
fn canonical_evidence_returns_only_scalars_without_cache_publication() {
    let definition = struct_definition(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
        ["Thing"],
        [],
    );
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["definitions"] = json!([definition]);
    root["reachable"] = json!([export(
        "root",
        ["Thing"],
        "type",
        id(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111",
        ),
        [],
    )]);
    let bytes = document(std::slice::from_ref(&root));
    let limits = cargo::Limits::standard();
    let mut allocation = Allocation::new(&limits);
    let document = protocol::decode_accounted(&bytes, &limits, &mut allocation)
        .expect("fixture document decodes under production authority");
    let mut invocations = Invocations::seal_document_with_allocation(&document, allocation)
        .expect("fixture capture seals");
    invocations.allocation.moved_document();
    let session = crate::process::EvidenceSession::new();

    crate::cache::reset_publications();
    let evidence = canonicalize_evidence_document(
        document,
        invocations,
        Policy::empty(),
        &api::Context::fixture(),
        &limits,
        &session,
    )
    .expect("private evidence canonicalization completes");
    assert!(evidence.items() > 0);
    assert!(evidence.items() <= CANONICAL_EVIDENCE_ITEMS);
    assert!(evidence.bytes() > 0);
    assert!(evidence.bytes() <= CANONICAL_EVIDENCE_BYTES);
    assert_eq!(crate::cache::publications(), 0);
}

#[test]
fn extraction_context_carries_build_unit_and_stable_role_and_namespace_text() {
    let value =
        serde_json::to_value(api::Context::fixture()).expect("the extraction context serializes");
    let build_unit = value["build_unit"]
        .as_array()
        .expect("the Build unit digest is an array");
    assert_eq!(build_unit.len(), 32);
    assert!(build_unit.iter().all(|byte| byte.as_u64() == Some(9)));
    assert!(value.get("check_unit").is_none());
    assert!(value.get("doc_unit").is_none());
    assert_eq!(cargo::config::Role::Library.as_str(), "library");
    assert_eq!(cargo::config::Role::Runtime.as_str(), "runtime");
    assert_eq!(cargo::config::Role::Compiler.as_str(), "compiler");
    assert_eq!(identity::Namespace::Type.as_str(), "type");
    assert_eq!(identity::Namespace::Value.as_str(), "value");
    assert_eq!(identity::Namespace::Macro.as_str(), "macro");
}

/// A cold build compiles what a warm one reuses; Cargo says so in each
/// artifact's `fresh`. The graph is the build's, not the build directory's:
/// both give one graph, byte for byte, with one fingerprint.
#[test]
fn a_reused_artifact_and_a_compiled_one_have_byte_identical_graphs() {
    let with = |fresh: bool| {
        let mut root = fragment("root 0.1.0 (path+file:///root)", "root", Some("root"));
        root["handshake"]["artifact"]["fresh"] = json!(fresh);
        graph(&[root], Policy::empty())
    };
    let compiled = with(false);
    let reused = with(true);
    assert_eq!(compiled.canonical_bytes(), reused.canonical_bytes());
    assert_eq!(compiled.fingerprint(), reused.fingerprint());
}

#[test]
fn shuffled_fragments_and_definitions_have_byte_identical_graphs() {
    let support = fragment("support 0.1.0 (path+file:///support)", "support", None);
    let mut root = fragment("root 0.1.0 (path+file:///root)", "root", Some("root"));
    root["reachable"] = json!([
        export(
            "root",
            ["make"],
            "value",
            id(
                "root 0.1.0 (path+file:///root)",
                "root",
                "22222222222222222222222222222222"
            ),
            []
        ),
        export(
            "root",
            ["Thing"],
            "type",
            id(
                "support 0.1.0 (path+file:///support)",
                "support",
                "11111111111111111111111111111111"
            ),
            [alias(["hidden", "Thing"], ["Thing"])]
        )
    ]);
    root["definitions"] = json!([
        function_definition(
            "root 0.1.0 (path+file:///root)",
            "root",
            "22222222222222222222222222222222",
            ["make"],
            definition_type(
                "support 0.1.0 (path+file:///support)",
                "support",
                "11111111111111111111111111111111"
            ),
        ),
        module_definition(
            "root 0.1.0 (path+file:///root)",
            "root",
            "33333333333333333333333333333333",
            ["hidden"]
        ),
    ]);
    let mut support = support;
    support["definitions"] = json!([struct_definition(
        "support 0.1.0 (path+file:///support)",
        "support",
        "11111111111111111111111111111111",
        ["hidden", "Thing"],
        []
    )]);
    let first = vec![root.clone(), support.clone()];
    let mut reversed_root = root;
    reversed_root["definitions"]
        .as_array_mut()
        .expect("test definitions are an array")
        .reverse();
    reversed_root["reachable"]
        .as_array_mut()
        .expect("test exports are an array")
        .reverse();
    let second = vec![support, reversed_root];

    let first_graph = graph(&first, Policy::empty());
    let second_graph = graph(&second, Policy::empty());
    assert_eq!(
        first_graph.canonical_bytes(),
        second_graph.canonical_bytes()
    );
    assert_eq!(first_graph.fingerprint(), second_graph.fingerprint());
}

#[test]
fn fragments_read_on_one_worker_and_on_eight_give_one_graph_and_one_account() {
    const CRATES: usize = 12;
    let package = |index: usize| format!("crate{index} 0.1.0 (path+crate{index})");
    let name = |index: usize| format!("crate{index}");
    let hash = |index: usize, item: usize| format!("{:032x}", index * 16 + item + 1);
    // Each crate's structs name the next crate's first struct, which each
    // fragment also carries, as a selected crate's walk records the
    // definitions its own items reach.
    let thing = |index: usize, item: usize, field: Option<Value>| {
        let path = format!("Thing{item}");
        struct_definition(
            &package(index),
            &name(index),
            &hash(index, item),
            [path.as_str()],
            [field.unwrap_or_else(|| json!({"kind":"primitive", "name":"bool"}))],
        )
    };
    let fragments = (0..CRATES)
        .map(|index| {
            let next = (index + 1) % CRATES;
            let alias = name(index);
            let mut fragment = fragment(&package(index), &name(index), Some(&alias));
            let mut definitions = Vec::new();
            let mut reachable = Vec::new();
            for item in 0..5 {
                let field = definition_type(&package(next), &name(next), &hash(next, 0));
                definitions.push(thing(index, item, (item > 0).then_some(field)));
                let path = format!("Thing{item}");
                reachable.push(export(
                    &alias,
                    [path.as_str()],
                    "type",
                    id(&package(index), &name(index), &hash(index, item)),
                    [],
                ));
            }
            definitions.push(thing(next, 0, None));
            definitions.push(definition(
                &package(index),
                &name(index),
                &hash(index, 9),
                ["Opaque"],
                json!({"kind":"unknown", "compiler_kind":"future_ty"}),
            ));
            reachable.push(export(
                &alias,
                ["Opaque"],
                "type",
                id(&package(index), &name(index), &hash(index, 9)),
                [],
            ));
            let target = id(&package(index), &name(index), &hash(index, 1));
            fragment["implementations"] = json!([implementation_record(
                &package(index),
                &name(index),
                &hash(index, 10),
                [target.clone()],
                json!({"kind":"inherent"}),
                json!({"kind":"definition", "id":target, "arguments":[]}),
                [],
            )]);
            fragment["definitions"] = Value::Array(definitions);
            fragment["reachable"] = Value::Array(reachable);
            fragment
        })
        .collect::<Vec<_>>();
    let bytes = document(&fragments);
    let limits = cargo::Limits::standard();
    let on = |workers: usize| {
        let Invocations {
            authority,
            mut allocation,
        } = expected(&fragments, &limits);
        let policy = Policy::empty();
        let context = api::Context::fixture();
        allocation
            .retain(policy.usage)
            .expect("an empty policy fits the account");
        let document = protocol::decode_accounted(&bytes, &limits, &mut allocation)
            .expect("the fixture decodes");
        let fragments = correlate(document.fragments, &authority, &mut allocation)
            .expect("the fixture's fragments are the expected units");
        allocation.category(EvidenceCategory::Candidate);
        validate_resolution_context(&fragments, &context, &mut allocation)
            .expect("the fixture's handshakes name the fixture context");
        let catalog = Catalog::from_fragments(&fragments, &authority, &mut allocation)
            .expect("the fixture's units are distinct");
        let candidate =
            Candidate::from_fragments_on(workers, fragments, &catalog, &limits, &mut allocation)
                .expect("the fixture's fragments join");
        let graph = candidate
            .finish(
                &policy,
                &context,
                &limits,
                &mut allocation,
                SealMode::Direct,
            )
            .expect("the fixture canonicalizes");
        (graph, allocation.usage())
    };
    let (serial, serial_account) = on(1);
    let (parallel, parallel_account) = on(8);
    assert_eq!(serial.definitions().len(), CRATES * 6);
    assert_eq!(serial.implementations().len(), CRATES);
    assert_eq!(serial.canonical_bytes(), parallel.canonical_bytes());
    assert_eq!(serial.fingerprint(), parallel.fingerprint());
    assert_eq!(
        serial.semantic_fingerprint(),
        parallel.semantic_fingerprint()
    );
    assert_eq!(serial.transaction(), parallel.transaction());
    assert_eq!(serial.allocation(), parallel.allocation());
    assert_eq!(serial_account, parallel_account);
}

#[test]
fn a_failing_fragment_fails_one_worker_and_eight_with_the_loops_error() {
    // The second fragment's definition conflicts with the first's; the
    // third's has a malformed path. A loop over the fragments meets the
    // conflict first.
    let package = |index: usize| format!("crate{index} 0.1.0 (path+crate{index})");
    let mut fragments = (0..3)
        .map(|index| fragment(&package(index), &format!("crate{index}"), None))
        .collect::<Vec<_>>();
    let shared = |field: Value| {
        struct_definition(
            &package(0),
            "crate0",
            "11111111111111111111111111111111",
            ["Thing"],
            [field],
        )
    };
    fragments[0]["definitions"] = json!([shared(json!({"kind":"primitive", "name":"bool"}))]);
    fragments[1]["definitions"] = json!([shared(json!({"kind":"primitive", "name":"char"}))]);
    fragments[2]["definitions"] = json!([definition(
        &package(2),
        "crate2",
        "22222222222222222222222222222222",
        [],
        json!({"kind":"module"}),
    )]);
    let bytes = document(&fragments);
    let limits = cargo::Limits::standard();
    for workers in [1, 8] {
        let Invocations {
            authority,
            mut allocation,
        } = expected(&fragments, &limits);
        let document = protocol::decode_accounted(&bytes, &limits, &mut allocation)
            .expect("the fixture decodes");
        let fragments = correlate(document.fragments, &authority, &mut allocation)
            .expect("the fixture's fragments are the expected units");
        let catalog = Catalog::from_fragments(&fragments, &authority, &mut allocation)
            .expect("the fixture's units are distinct");
        assert_code(
            Candidate::from_fragments_on(workers, fragments, &catalog, &limits, &mut allocation),
            "conflicting-definition",
        );
    }
}

#[test]
fn duplicate_crate_names_at_distinct_package_ids_remain_distinct() {
    let mut first = fragment("same 1.0.0 (registry+one)", "same", Some("one"));
    first["definitions"] = json!([struct_definition(
        "same 1.0.0 (registry+one)",
        "same",
        "11111111111111111111111111111111",
        ["Thing"],
        []
    )]);
    first["reachable"] = json!([export(
        "one",
        ["Thing"],
        "type",
        id(
            "same 1.0.0 (registry+one)",
            "same",
            "11111111111111111111111111111111"
        ),
        []
    )]);
    let mut second = fragment("same 2.0.0 (registry+two)", "same", Some("two"));
    second["definitions"] = json!([struct_definition(
        "same 2.0.0 (registry+two)",
        "same",
        "11111111111111111111111111111111",
        ["Thing"],
        []
    )]);
    second["reachable"] = json!([export(
        "two",
        ["Thing"],
        "type",
        id(
            "same 2.0.0 (registry+two)",
            "same",
            "11111111111111111111111111111111"
        ),
        []
    )]);

    let graph = graph(&[first, second], Policy::empty());
    let packages = graph
        .definitions()
        .map(|definition| {
            definition
                .id()
                .unit()
                .package()
                .expect("test definitions are Cargo-owned")
                .cargo_id()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        packages,
        ["same 1.0.0 (registry+one)", "same 2.0.0 (registry+two)"]
    );
}

#[test]
fn private_module_reexport_keeps_one_definition_and_exact_alias_chain() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["definitions"] = json!([struct_definition(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
        ["private", "Thing"],
        []
    )]);
    root["reachable"] = json!([export(
        "root",
        ["Thing"],
        "type",
        id(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111"
        ),
        [alias(["private", "Thing"], ["Thing"])]
    )]);

    let graph = graph(std::slice::from_ref(&root), Policy::empty());
    assert_eq!(graph.definitions().len(), 1);
    let path = graph.exports().next().expect("one path outcome exists");
    let ledger::Path::Imported(export) = path else {
        panic!("the valid reexport must import");
    };
    assert_eq!(export.aliases().len(), 1);
}

#[test]
fn exact_target_closure_is_computed_once_without_collapsing_alias_paths() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["definitions"] = json!([struct_definition(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
        ["private", "Thing"],
        []
    )]);
    let target = id(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
    );
    root["reachable"] = json!([
        export(
            "root",
            ["Thing"],
            "type",
            target.clone(),
            [alias(["private", "Thing"], ["Thing"])]
        ),
        export(
            "root",
            ["Alias"],
            "type",
            target,
            [alias(["private", "Thing"], ["Alias"])]
        )
    ]);

    let evidence = evidence(&[root]);
    assert_eq!(evidence.counters.closure_computations, 1);
    assert_eq!(evidence.counters.closure_cache_hits, 1);
    assert_eq!(evidence.counters.candidate_definitions, 1);
    assert_eq!(evidence.counters.candidate_exports, 2);
    assert_eq!(evidence.counters.final_definitions, 1);
    assert_eq!(evidence.counters.final_exports, 2);
}

#[test]
fn global_fixed_point_owns_shared_diamond_nodes_and_edges_once() {
    let package = "root 0.1.0 (path+root)";
    let root_a = "11111111111111111111111111111111";
    let root_b = "22222222222222222222222222222222";
    let left = "33333333333333333333333333333333";
    let right = "44444444444444444444444444444444";
    let shared = "55555555555555555555555555555555";
    let mut fragment = fragment(package, "root", Some("root"));
    fragment["definitions"] = json!([
        struct_definition(
            package,
            "root",
            root_a,
            ["RootA"],
            [
                definition_type(package, "root", left),
                definition_type(package, "root", right),
            ]
        ),
        struct_definition(
            package,
            "root",
            root_b,
            ["RootB"],
            [
                definition_type(package, "root", left),
                definition_type(package, "root", right),
            ]
        ),
        struct_definition(
            package,
            "root",
            left,
            ["Left"],
            [definition_type(package, "root", shared)]
        ),
        struct_definition(
            package,
            "root",
            right,
            ["Right"],
            [definition_type(package, "root", shared)]
        ),
        struct_definition(package, "root", shared, ["Shared"], []),
    ]);
    fragment["reachable"] = json!([
        export("root", ["RootA"], "type", id(package, "root", root_a), []),
        export("root", ["RootB"], "type", id(package, "root", root_b), []),
    ]);

    let evidence = evidence(&[fragment]);
    assert_eq!(evidence.counters.fixed_nodes, 5);
    assert_eq!(evidence.counters.fixed_edges, 6);
    assert_eq!(evidence.counters.fixed_targets, 2);
    assert_eq!(evidence.counters.forward_queue_entries, 5);
    assert_eq!(evidence.counters.reverse_queue_entries, 0);
    assert_eq!(evidence.counters.retained_queue_entries, 5);
    assert_eq!(evidence.counters.final_definitions, 5);
    assert_eq!(evidence.counters.final_exports, 2);
    assert_eq!(evidence.counters.final_support, 6);
}

#[test]
fn equal_distance_rejection_uses_lowest_forward_edge_and_exact_referring_span() {
    let package = "root 0.1.0 (path+root)";
    let root_id = "11111111111111111111111111111111";
    let left = "22222222222222222222222222222222";
    let right = "33333333333333333333333333333333";
    let left_missing = "44444444444444444444444444444444";
    let right_missing = "55555555555555555555555555555555";
    let mut left_definition = struct_definition(
        package,
        "root",
        left,
        ["Left"],
        [definition_type(package, "root", left_missing)],
    );
    left_definition["span"]["file"] = json!("left.rs");
    let mut right_definition = struct_definition(
        package,
        "root",
        right,
        ["Right"],
        [definition_type(package, "root", right_missing)],
    );
    right_definition["span"]["file"] = json!("right.rs");
    let mut fragment = fragment(package, "root", Some("root"));
    fragment["definitions"] = json!([
        struct_definition(
            package,
            "root",
            root_id,
            ["Root"],
            [
                definition_type(package, "root", left),
                definition_type(package, "root", right),
            ]
        ),
        left_definition,
        right_definition,
    ]);
    fragment["reachable"] = json!([export(
        "root",
        ["Root"],
        "type",
        id(package, "root", root_id),
        []
    )]);

    let graph = graph(&[fragment], Policy::empty());
    let ledger::Path::Rejected { reason, span, .. } =
        graph.exports().next().expect("one path outcome exists")
    else {
        panic!("the missing descendant must reject the selected path");
    };
    let ledger::Reason::UnresolvedDefinition(id) = reason else {
        panic!("the exact missing descendant must be retained");
    };
    assert_eq!(id.def_path_hash(), left_missing);
    assert_eq!(span.file(), "left.rs");
}

#[test]
fn global_fixed_point_high_fanout_has_linear_exact_work_bound() {
    const ROOTS: usize = 32;
    let package = "root 0.1.0 (path+root)";
    let shared = "ffffffffffffffffffffffffffffffff";
    let mut definitions = vec![struct_definition(package, "root", shared, ["Shared"], [])];
    let mut reachable = Vec::new();
    for index in 1..=ROOTS {
        let hash = format!("{index:032x}");
        let name = format!("Root{index:02}");
        definitions.push(struct_definition(
            package,
            "root",
            &hash,
            [name.as_str()],
            [definition_type(package, "root", shared)],
        ));
        reachable.push(export(
            "root",
            [name.as_str()],
            "type",
            id(package, "root", &hash),
            [],
        ));
    }
    let mut fragment = fragment(package, "root", Some("root"));
    fragment["definitions"] = Value::Array(definitions);
    fragment["reachable"] = Value::Array(reachable);

    let evidence = evidence(&[fragment]);
    let counters = evidence.counters;
    assert_eq!(counters.fixed_nodes, ROOTS + 1);
    assert_eq!(counters.fixed_edges, ROOTS);
    assert_eq!(counters.fixed_targets, ROOTS);
    assert_eq!(counters.forward_queue_entries, ROOTS + 1);
    assert_eq!(counters.reverse_queue_entries, 0);
    assert_eq!(counters.retained_queue_entries, ROOTS + 1);
    assert_eq!(counters.final_definitions, ROOTS + 1);
    assert_eq!(counters.final_exports, ROOTS);
    assert_eq!(counters.final_support, ROOTS);
    let fixed_work = counters
        .forward_queue_entries
        .saturating_add(counters.reverse_queue_entries)
        .saturating_add(counters.retained_queue_entries)
        .saturating_add(counters.fixed_edges)
        .saturating_add(counters.fixed_targets);
    let checked_linear_bound = counters
        .fixed_nodes
        .saturating_mul(3)
        .saturating_add(counters.fixed_edges.saturating_mul(2))
        .saturating_add(counters.fixed_targets);
    assert_eq!(fixed_work, 4 * ROOTS + 2);
    assert!(fixed_work <= checked_linear_bound);
}

#[test]
fn cached_rejection_is_cloned_for_every_path_in_canonical_order() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    let target = id(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
    );
    root["reachable"] = json!([
        export("root", ["Zulu"], "type", target.clone(), []),
        export("root", ["Alpha"], "type", target, [])
    ]);

    let limits = cargo::Limits::standard();
    let graph = canonicalize(
        &document(std::slice::from_ref(&root)),
        expected(std::slice::from_ref(&root), &limits),
        Policy::empty(),
        &api::Context::fixture(),
        &limits,
    )
    .expect("one unresolved target becomes two path-local rejections");
    assert_eq!(
        graph
            .exports()
            .map(|path| path.key().path().collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [vec!["Alpha"], vec!["Zulu"]],
    );
    assert!(graph
        .exports()
        .all(|path| matches!(path, ledger::Path::Rejected { .. })));

    let evidence = evidence(&[root]);
    assert_eq!(evidence.counters.closure_computations, 1);
    assert_eq!(evidence.counters.closure_cache_hits, 1);
    assert_eq!(evidence.counters.final_exports, 2);
}

#[test]
fn evidence_counters_are_fixed_scalars_and_do_not_charge_allocation() {
    let limits = cargo::Limits::standard();
    let mut allocation = Allocation::new(&limits)
        .into_evidence(&limits, protocol::Population::default())
        .expect("normal allocation transforms into private evidence");
    let before = allocation.usage();
    allocation.candidate_population(1, 2, 3, 4, 5);
    allocation.queue_entries(6);
    allocation.dedup_entries(7);
    allocation.index_entries(8);
    allocation.sort_keys(9);
    allocation.temporary_conversions(10);
    allocation.closure_computation();
    allocation.closure_cache_hit();
    allocation.fixed_population(12, 13, 14);
    allocation.fixed_queue(15, 16, 17);
    allocation.support_edges(11);
    allocation.raw_canonical_bytes(12);
    allocation.final_graph(1, 2, 3, 4, 5, 6);
    assert_eq!(allocation.usage(), before);
    let counters = allocation
        .evidence_counters()
        .expect("private scalar counters exist");
    assert_eq!(counters.queue_entries, 6);
    assert_eq!(counters.dedup_entries, 7);
    assert_eq!(counters.index_entries, 8);
    assert_eq!(counters.sort_keys, 9);
    assert_eq!(counters.temporary_conversions, 10);
    assert_eq!(counters.fixed_nodes, 12);
    assert_eq!(counters.fixed_edges, 13);
    assert_eq!(counters.fixed_targets, 14);
    assert_eq!(counters.forward_queue_entries, 15);
    assert_eq!(counters.reverse_queue_entries, 16);
    assert_eq!(counters.retained_queue_entries, 17);
    assert_eq!(counters.final_graph_items, 21);
}

#[test]
fn dependency_reexport_adds_support_without_exposing_dependency_namespace() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["reachable"] = json!([export(
        "root",
        ["Thing"],
        "type",
        id(
            "dep 0.1.0 (path+dep)",
            "dep",
            "11111111111111111111111111111111"
        ),
        [alias(["dep", "Thing"], ["Thing"])]
    )]);
    let mut dependency = fragment("dep 0.1.0 (path+dep)", "dep", None);
    dependency["definitions"] = json!([
        struct_definition(
            "dep 0.1.0 (path+dep)",
            "dep",
            "11111111111111111111111111111111",
            ["dep", "Thing"],
            [definition_type(
                "dep 0.1.0 (path+dep)",
                "dep",
                "22222222222222222222222222222222"
            )]
        ),
        struct_definition(
            "dep 0.1.0 (path+dep)",
            "dep",
            "22222222222222222222222222222222",
            ["dep", "Support"],
            []
        ),
        struct_definition(
            "dep 0.1.0 (path+dep)",
            "dep",
            "33333333333333333333333333333333",
            ["dep", "Unrelated"],
            []
        ),
    ]);

    let graph = graph(&[root, dependency], Policy::empty());
    assert_eq!(graph.exports().len(), 1);
    assert_eq!(graph.definitions().len(), 2);
    assert_eq!(graph.support().len(), 1);
    assert!(graph
        .definitions()
        .all(|definition| definition.id().def_path_hash() != "33333333333333333333333333333333"));
}

#[test]
fn recursive_support_terminates_through_visited_identities() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["definitions"] = json!([
        struct_definition(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111",
            ["A"],
            [definition_type(
                "root 0.1.0 (path+root)",
                "root",
                "22222222222222222222222222222222"
            )]
        ),
        struct_definition(
            "root 0.1.0 (path+root)",
            "root",
            "22222222222222222222222222222222",
            ["B"],
            [definition_type(
                "root 0.1.0 (path+root)",
                "root",
                "11111111111111111111111111111111"
            )]
        ),
    ]);
    root["reachable"] = json!([export(
        "root",
        ["A"],
        "type",
        id(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111"
        ),
        []
    )]);

    let graph = graph(std::slice::from_ref(&root), Policy::empty());
    assert_eq!(graph.definitions().len(), 2);
    assert_eq!(graph.support().len(), 2);
    assert!(matches!(
        graph.exports().next(),
        Some(ledger::Path::Imported(_))
    ));
    let evidence = evidence(&[root]);
    assert_eq!(evidence.counters.fixed_nodes, 2);
    assert_eq!(evidence.counters.fixed_edges, 2);
    assert_eq!(evidence.counters.fixed_targets, 1);
    assert_eq!(evidence.counters.forward_queue_entries, 2);
    assert_eq!(evidence.counters.reverse_queue_entries, 0);
    assert_eq!(evidence.counters.retained_queue_entries, 2);
}

#[test]
fn relevant_implementation_and_associated_definition_form_distinct_edges() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    let thing = id(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
    );
    let mut method = function_definition(
        "root 0.1.0 (path+root)",
        "root",
        "22222222222222222222222222222222",
        ["Thing", "method"],
        json!({"kind":"primitive", "name":"u32"}),
    );
    method["owner"] = json!({
        "kind":"implementation",
        "id":id(
            "root 0.1.0 (path+root)",
            "root",
            "44444444444444444444444444444444"
        )
    });
    root["definitions"] = json!([
        struct_definition(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111",
            ["Thing"],
            []
        ),
        method,
    ]);
    root["reachable"] = json!([export("root", ["Thing"], "type", thing.clone(), [])]);
    root["implementations"] = json!([{
        "key":"44444444444444444444444444444444",
        "id":id("root 0.1.0 (path+root)", "root", "44444444444444444444444444444444"),
        "relevance":[thing.clone()],
        "trait_ref":{"kind":"inherent"},
        "self_type":{"kind":"definition", "id":thing, "arguments":[]},
        "safety":"safe",
        "polarity":"positive",
        "const_impl":false,
        "default_impl":false,
        "generics":empty_generics(),
        "associated":[value_id("root 0.1.0 (path+root)", "root", "22222222222222222222222222222222")],
        "span":span_value(),
        "origin":{"kind":"source"}
    }]);

    let graph = graph(std::slice::from_ref(&root), Policy::empty());
    assert_eq!(graph.implementations().len(), 1);
    assert_eq!(graph.definitions().len(), 2);
    assert_eq!(graph.support().len(), 2);

    let mut missing_owner = root;
    missing_owner["definitions"][1]["owner"]["id"]["def_path_hash"] =
        json!("55555555555555555555555555555555");
    let limits = cargo::Limits::standard();
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&missing_owner)),
            expected(std::slice::from_ref(&missing_owner), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "implementation-owner-cardinality",
    );
}

#[test]
fn direct_associated_root_admits_only_its_exact_owner_implementation() {
    let package = "root 0.1.0 (path+root)";
    let first_type = "11111111111111111111111111111111";
    let first_method = "22222222222222222222222222222222";
    let first_impl = "33333333333333333333333333333333";
    let sibling_type = "44444444444444444444444444444444";
    let sibling_method = "55555555555555555555555555555555";
    let sibling_impl = "66666666666666666666666666666666";
    let mut first = function_definition(
        package,
        "root",
        first_method,
        ["First", "method"],
        json!({"kind":"primitive", "name":"u32"}),
    );
    first["owner"] = json!({
        "kind":"implementation",
        "id":id(package, "root", first_impl)
    });
    let mut sibling = function_definition(
        package,
        "root",
        sibling_method,
        ["Sibling", "method"],
        json!({"kind":"primitive", "name":"u32"}),
    );
    sibling["owner"] = json!({
        "kind":"implementation",
        "id":id(package, "root", sibling_impl)
    });
    let mut root = fragment(package, "root", Some("root"));
    root["definitions"] = json!([
        struct_definition(package, "root", first_type, ["First"], []),
        first,
        struct_definition(package, "root", sibling_type, ["Sibling"], []),
        sibling,
    ]);
    root["implementations"] = json!([
        implementation_record(
            package,
            "root",
            first_impl,
            [id(package, "root", first_type)],
            json!({"kind":"inherent"}),
            definition_type(package, "root", first_type),
            [value_id(package, "root", first_method)]
        ),
        implementation_record(
            package,
            "root",
            sibling_impl,
            [id(package, "root", sibling_type)],
            json!({"kind":"inherent"}),
            definition_type(package, "root", sibling_type),
            [value_id(package, "root", sibling_method)]
        ),
    ]);
    root["reachable"] = json!([export(
        "root",
        ["First", "method"],
        "value",
        value_id(package, "root", first_method),
        []
    )]);

    let canonical_graph = graph(std::slice::from_ref(&root), Policy::empty());
    assert_eq!(canonical_graph.implementations().len(), 1);
    let implementation = canonical_graph
        .implementations()
        .next()
        .expect("the direct associated root admits its owner");
    assert_eq!(implementation.id().def_path_hash(), first_impl);
    assert_eq!(canonical_graph.definitions().len(), 2);
    assert!(canonical_graph
        .definitions()
        .all(|definition| definition.id().def_path_hash() != sibling_method));
    let method = canonical_graph
        .definitions()
        .find(|definition| definition.id().def_path_hash() == first_method)
        .expect("the direct method is retained");
    assert!(matches!(
        method.owner(),
        model::Owner::Implementation(owner) if owner == implementation.id()
    ));

    let mut shuffled = root.clone();
    shuffled["definitions"]
        .as_array_mut()
        .expect("definitions are an array")
        .reverse();
    shuffled["implementations"]
        .as_array_mut()
        .expect("implementations are an array")
        .reverse();
    assert_eq!(
        canonical_graph.canonical_bytes(),
        graph(&[shuffled], Policy::empty()).canonical_bytes()
    );

    let evidence = evidence(&[root]);
    assert_eq!(evidence.counters.final_implementations, 1);
    assert_eq!(evidence.counters.final_definitions, 2);
}

#[test]
fn associated_owner_high_fanout_has_one_linear_impl_cycle() {
    const METHODS: usize = 32;
    let package = "root 0.1.0 (path+root)";
    let target = "ffffffffffffffffffffffffffffffff";
    let impl_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    let mut definitions = vec![struct_definition(package, "root", target, ["Target"], [])];
    let mut associated = Vec::new();
    let mut reachable = Vec::new();
    for index in 1..=METHODS {
        let hash = format!("{index:032x}");
        let name = format!("method_{index:02}");
        let mut method = function_definition(
            package,
            "root",
            &hash,
            ["Target", name.as_str()],
            json!({"kind":"primitive", "name":"u32"}),
        );
        method["owner"] = json!({
            "kind":"implementation",
            "id":id(package, "root", impl_id)
        });
        definitions.push(method);
        associated.push(value_id(package, "root", &hash));
        reachable.push(export(
            "root",
            ["Target", name.as_str()],
            "value",
            value_id(package, "root", &hash),
            [],
        ));
    }
    let mut root = fragment(package, "root", Some("root"));
    root["definitions"] = Value::Array(definitions);
    root["implementations"] = json!([{
        "key":impl_id,
        "id":id(package, "root", impl_id),
        "relevance":[id(package, "root", target)],
        "trait_ref":{"kind":"inherent"},
        "self_type":definition_type(package, "root", target),
        "safety":"safe",
        "polarity":"positive",
        "const_impl":false,
        "default_impl":false,
        "generics":empty_generics(),
        "associated":associated,
        "span":span_value(),
        "origin":{"kind":"source"}
    }]);
    root["reachable"] = Value::Array(reachable);

    let evidence = evidence(&[root]);
    let counters = evidence.counters;
    assert_eq!(counters.fixed_nodes, METHODS + 2);
    assert_eq!(counters.fixed_edges, METHODS.saturating_mul(2) + 2);
    assert_eq!(counters.fixed_targets, METHODS);
    assert_eq!(counters.forward_queue_entries, METHODS + 2);
    assert_eq!(counters.reverse_queue_entries, 0);
    assert_eq!(counters.retained_queue_entries, METHODS + 2);
    assert_eq!(counters.final_definitions, METHODS + 1);
    assert_eq!(counters.final_implementations, 1);
    assert_eq!(counters.final_exports, METHODS);
    assert_eq!(counters.final_support, METHODS + 1);
    let fixed_work = counters
        .forward_queue_entries
        .saturating_add(counters.reverse_queue_entries)
        .saturating_add(counters.retained_queue_entries)
        .saturating_add(counters.fixed_edges)
        .saturating_add(counters.fixed_targets);
    let checked_linear_bound = counters
        .fixed_nodes
        .saturating_mul(3)
        .saturating_add(counters.fixed_edges.saturating_mul(2))
        .saturating_add(counters.fixed_targets);
    assert!(fixed_work <= checked_linear_bound);
}

#[test]
fn exact_trait_default_owner_is_accepted_but_a_sibling_trait_is_rejected() {
    let package = "root 0.1.0 (path+root)";
    let target = "11111111111111111111111111111111";
    let trait_id = "22222222222222222222222222222222";
    let sibling_trait = "33333333333333333333333333333333";
    let default_method = "44444444444444444444444444444444";
    let impl_id = "55555555555555555555555555555555";
    let mut method = function_definition(
        package,
        "root",
        default_method,
        ["Contract", "default_method"],
        json!({"kind":"primitive", "name":"u32"}),
    );
    method["owner"] = json!({
        "kind":"definition",
        "id":id(package, "root", trait_id)
    });
    let mut root = fragment(package, "root", Some("root"));
    root["definitions"] = json!([
        struct_definition(package, "root", target, ["Target"], []),
        trait_definition(package, "root", trait_id, ["Contract"]),
        trait_definition(package, "root", sibling_trait, ["SiblingContract"]),
        method,
    ]);
    root["implementations"] = json!([implementation_record(
        package,
        "root",
        impl_id,
        [id(package, "root", target)],
        json!({
            "kind":"trait",
            "id":id(package, "root", trait_id),
            "arguments":[]
        }),
        definition_type(package, "root", target),
        [value_id(package, "root", default_method)]
    )]);
    root["reachable"] = json!([export(
        "root",
        ["Target"],
        "type",
        id(package, "root", target),
        []
    )]);

    let graph = graph(std::slice::from_ref(&root), Policy::empty());
    assert_eq!(graph.implementations().len(), 1);
    let default = graph
        .definitions()
        .find(|definition| definition.id().def_path_hash() == default_method)
        .expect("the exact effective default is retained");
    assert!(matches!(
        default.owner(),
        model::Owner::Definition(owner) if owner.def_path_hash() == trait_id
    ));

    let mut forged = root;
    forged["definitions"][3]["owner"]["id"] = id(package, "root", sibling_trait);
    let limits = cargo::Limits::standard();
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&forged)),
            expected(std::slice::from_ref(&forged), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "implementation-associated-owner",
    );
}

#[test]
fn missing_or_forged_associated_owner_fails_before_graph_publication() {
    let package = "root 0.1.0 (path+root)";
    let target = "11111111111111111111111111111111";
    let method = "22222222222222222222222222222222";
    let impl_id = "33333333333333333333333333333333";
    let missing_impl = "44444444444444444444444444444444";
    let mut definition = function_definition(
        package,
        "root",
        method,
        ["Target", "method"],
        json!({"kind":"primitive", "name":"u32"}),
    );
    definition["owner"] = json!({
        "kind":"implementation",
        "id":id(package, "root", missing_impl)
    });
    let mut root = fragment(package, "root", Some("root"));
    root["definitions"] = json!([
        struct_definition(package, "root", target, ["Target"], []),
        definition,
    ]);
    root["implementations"] = json!([implementation_record(
        package,
        "root",
        impl_id,
        [id(package, "root", target)],
        json!({"kind":"inherent"}),
        definition_type(package, "root", target),
        [value_id(package, "root", method)]
    )]);
    root["reachable"] = json!([export(
        "root",
        ["Target", "method"],
        "value",
        value_id(package, "root", method),
        []
    )]);
    let limits = cargo::Limits::standard();
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&root)),
            expected(std::slice::from_ref(&root), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "implementation-owner-cardinality",
    );

    root["definitions"][1]["owner"]["id"] = id(package, "root", impl_id);
    root["implementations"][0]["associated"] = json!([]);
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&root)),
            expected(std::slice::from_ref(&root), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "implementation-owner-associated",
    );
}

#[test]
fn explicit_exclusion_is_positioned_and_does_not_import_target() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    let thing = id(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
    );
    let mut method = function_definition(
        "root 0.1.0 (path+root)",
        "root",
        "22222222222222222222222222222222",
        ["Thing", "method"],
        json!({"kind":"primitive", "name":"u32"}),
    );
    method["owner"] = json!({
        "kind":"implementation",
        "id":id(
            "root 0.1.0 (path+root)",
            "root",
            "44444444444444444444444444444444"
        )
    });
    root["definitions"] = json!([
        struct_definition(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111",
            ["Thing"],
            []
        ),
        method,
    ]);
    root["implementations"] = json!([{
        "key":"44444444444444444444444444444444",
        "id":id("root 0.1.0 (path+root)", "root", "44444444444444444444444444444444"),
        "relevance":[thing.clone()],
        "trait_ref":{"kind":"inherent"},
        "self_type":{"kind":"definition", "id":thing, "arguments":[]},
        "safety":"safe",
        "polarity":"positive",
        "const_impl":false,
        "default_impl":false,
        "generics":empty_generics(),
        "associated":[value_id("root 0.1.0 (path+root)", "root", "22222222222222222222222222222222")],
        "span":span_value(),
        "origin":{"kind":"source"}
    }]);
    root["reachable"] = json!([export(
        "root",
        ["Thing"],
        "type",
        id(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111"
        ),
        []
    )]);
    let key = Key {
        alias: "root".into(),
        path: vec![Box::<str>::from("Thing")].into_boxed_slice(),
        namespace: identity::Namespace::Type,
    };
    let policy = Policy::excluding(key.clone(), "not part of the contract", test_span());
    let policy_identity = policy.cache_identity().unwrap();
    assert_ne!(policy_identity, Policy::empty().cache_identity().unwrap());
    assert_ne!(
        policy_identity,
        Policy::excluding(key.clone(), "changed reason", test_span())
            .cache_identity()
            .unwrap()
    );
    assert_ne!(
        policy_identity,
        Policy::excluding(
            key.clone(),
            "not part of the contract",
            api::model::Span::new("Cargo.toml".into(), 3, 9, 2, 1, 2, 7)
        )
        .cache_identity()
        .unwrap()
    );
    let included = graph(std::slice::from_ref(&root), Policy::empty());
    let excluded = graph(std::slice::from_ref(&root), policy);
    assert_eq!(included.canonical_bytes(), excluded.canonical_bytes());
    assert_eq!(included.fingerprint(), excluded.fingerprint());
    assert_eq!(excluded.definitions().len(), 0);
    assert_eq!(excluded.implementations().len(), 0);
    assert_eq!(excluded.support().len(), 0);
    assert!(matches!(
        excluded.exports().next(),
        Some(ledger::Path::Excluded { .. })
    ));

    let changed_policy = Policy::excluding(
        key,
        "excluded for a different Rawr policy reason",
        api::model::Span::new("Cargo.toml".into(), 3, 9, 2, 1, 2, 7),
    );
    assert_ne!(policy_identity, changed_policy.cache_identity().unwrap());
    let changed = graph(std::slice::from_ref(&root), changed_policy);
    assert_eq!(excluded.canonical_bytes(), changed.canonical_bytes());
    assert_eq!(excluded.fingerprint(), changed.fingerprint());
    let standard_contract = excluded
        .publish(crate::input::proof::Proof::fixture())
        .expect("the first exclusion proof publishes")
        .classify(
            &crate::classify::Policy::standard(),
            &cargo::Limits::standard(),
        )
        .expect("the first exclusion policy classifies");
    assert!(matches!(
        standard_contract
            .outcomes()
            .next()
            .map(|outcome| outcome.representability()),
        Some(crate::classify::representability::Representability::Excluded(_))
    ));
    let changed_contract = changed
        .publish(crate::input::proof::Proof::fixture())
        .expect("the changed exclusion proof publishes")
        .classify(
            &crate::classify::Policy::standard(),
            &cargo::Limits::standard(),
        )
        .expect("the changed exclusion policy classifies");
    assert_ne!(
        standard_contract.canonical_bytes(),
        changed_contract.canonical_bytes()
    );
    assert_ne!(
        standard_contract.fingerprint(),
        changed_contract.fingerprint()
    );

    let mut changed_signature = root;
    changed_signature["definitions"][1]["shape"]["signature"]["output"] =
        json!({"kind":"primitive", "name":"i64"});
    let changed_raw = graph(&[changed_signature], Policy::empty());
    assert_ne!(included.canonical_bytes(), changed_raw.canonical_bytes());
    assert_ne!(included.fingerprint(), changed_raw.fingerprint());
}

#[test]
fn excluded_alias_does_not_seed_but_included_alias_retains_shared_target_once() {
    let package = "root 0.1.0 (path+root)";
    let target = "11111111111111111111111111111111";
    let support = "22222222222222222222222222222222";
    let mut root = fragment(package, "root", Some("root"));
    root["definitions"] = json!([
        struct_definition(
            package,
            "root",
            target,
            ["Thing"],
            [definition_type(package, "root", support)]
        ),
        struct_definition(package, "root", support, ["Support"], []),
    ]);
    root["reachable"] = json!([
        export(
            "root",
            ["Drop"],
            "type",
            id(package, "root", target),
            [alias(["Thing"], ["Drop"])]
        ),
        export(
            "root",
            ["Keep"],
            "type",
            id(package, "root", target),
            [alias(["Thing"], ["Keep"])]
        ),
    ]);
    let policy = Policy::excluding(
        Key {
            alias: "root".into(),
            path: vec![Box::<str>::from("Drop")].into_boxed_slice(),
            namespace: identity::Namespace::Type,
        },
        "not part of the contract",
        test_span(),
    );

    let graph = graph(&[root], policy);
    assert_eq!(graph.definitions().len(), 2);
    assert_eq!(graph.support().len(), 1);
    assert_eq!(
        graph
            .exports()
            .filter(|path| matches!(path, ledger::Path::Imported(_)))
            .count(),
        1
    );
    assert_eq!(
        graph
            .exports()
            .filter(|path| matches!(path, ledger::Path::Excluded { .. }))
            .count(),
        1
    );
}

#[test]
fn every_reported_path_and_support_identity_has_one_outcome() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["definitions"] = json!([
        struct_definition(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111",
            ["Good"],
            [definition_type(
                "root 0.1.0 (path+root)",
                "root",
                "22222222222222222222222222222222"
            )]
        ),
        struct_definition(
            "root 0.1.0 (path+root)",
            "root",
            "22222222222222222222222222222222",
            ["Support"],
            []
        ),
    ]);
    root["reachable"] = json!([
        export(
            "root",
            ["Good"],
            "type",
            id(
                "root 0.1.0 (path+root)",
                "root",
                "11111111111111111111111111111111"
            ),
            []
        ),
        export(
            "root",
            ["Missing"],
            "type",
            id(
                "root 0.1.0 (path+root)",
                "root",
                "33333333333333333333333333333333"
            ),
            []
        ),
    ]);

    let graph = graph(std::slice::from_ref(&root), Policy::empty());
    assert_eq!(graph.exports().len(), 2);
    assert_eq!(graph.support().len(), 1);
    assert_eq!(
        graph
            .exports()
            .filter(|path| matches!(path, ledger::Path::Rejected { .. }))
            .count(),
        1
    );
    let evidence = evidence(&[root]);
    assert_eq!(evidence.counters.fixed_nodes, 3);
    assert_eq!(evidence.counters.fixed_edges, 1);
    assert_eq!(evidence.counters.fixed_targets, 2);
    assert_eq!(evidence.counters.forward_queue_entries, 3);
    assert_eq!(evidence.counters.reverse_queue_entries, 1);
    assert_eq!(evidence.counters.retained_queue_entries, 3);
}

#[test]
fn unresolved_signature_support_rejects_path_and_records_support_identity() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["definitions"] = json!([struct_definition(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
        ["Public"],
        [definition_type(
            "root 0.1.0 (path+root)",
            "root",
            "22222222222222222222222222222222",
        )]
    )]);
    root["reachable"] = json!([export(
        "root",
        ["Public"],
        "type",
        id(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111"
        ),
        []
    )]);

    let graph = graph(&[root], Policy::empty());
    assert!(matches!(
        graph.exports().next(),
        Some(ledger::Path::Rejected { .. })
    ));
    assert!(matches!(
        graph.support().next(),
        Some(ledger::Support::Rejected { .. })
    ));
}

#[test]
fn unknown_compiler_shape_is_a_total_positioned_rejected_definition() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["definitions"] = json!([definition(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
        ["Opaque"],
        json!({"kind":"unknown", "compiler_kind":"future_ty"})
    )]);
    root["reachable"] = json!([export(
        "root",
        ["Opaque"],
        "type",
        id(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111"
        ),
        []
    )]);

    let graph = graph(&[root], Policy::empty());
    let definition = graph
        .definitions()
        .next()
        .expect("the rejected declaration remains in the total definition ledger");
    let model::Kind::Rejected { class, reason } = definition.kind() else {
        panic!("unknown compiler evidence must retain a closed rejected node");
    };
    assert_eq!(*class, model::RejectedClass::Unavailable);
    assert!(matches!(
        reason,
        ledger::Reason::UnknownCompilerShape { compiler_kind, .. }
            if compiler_kind.as_ref() == "future_ty"
    ));
    assert_eq!(definition.span().file(), "src/lib.rs");
    assert!(matches!(
        graph.exports().next(),
        Some(ledger::Path::Imported(export)) if export.target() == definition.id()
    ));
}

#[test]
#[cfg(feature = "rustc-driver")]
fn unavailable_external_artifact_authority_rejects_only_dependent_paths() {
    let dependency = "dep 1.0.0 (path+dep)";
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    let external = json!([{
        "crate_name": "dep",
        "stable_crate_id": stable_id(dependency),
        "artifacts": ["/unjoined/libdep.rlib"]
    }]);
    let loaded = root["handshake"]["loaded_crates"]
        .as_array_mut()
        .expect("loaded crate fixture is an array");
    loaded.push(external[0].clone());
    loaded.sort_unstable_by(|left, right| {
        left["crate_name"]
            .as_str()
            .cmp(&right["crate_name"].as_str())
    });
    root["handshake"]["external_crates"] = external;
    root["definitions"] = json!([
        function_definition(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111",
            ["bad"],
            definition_type(dependency, "dep", "33333333333333333333333333333333")
        ),
        function_definition(
            "root 0.1.0 (path+root)",
            "root",
            "22222222222222222222222222222222",
            ["good"],
            json!({"kind":"primitive", "name":"u32"})
        ),
        struct_definition(
            dependency,
            "dep",
            "33333333333333333333333333333333",
            ["External"],
            []
        )
    ]);
    root["reachable"] = json!([
        export(
            "root",
            ["bad"],
            "value",
            value_id(
                "root 0.1.0 (path+root)",
                "root",
                "11111111111111111111111111111111"
            ),
            []
        ),
        export(
            "root",
            ["good"],
            "value",
            value_id(
                "root 0.1.0 (path+root)",
                "root",
                "22222222222222222222222222222222"
            ),
            []
        )
    ]);
    let limits = cargo::Limits::standard();
    let invocations = expected(std::slice::from_ref(&root), &limits)
        .bind_external(vec![ExternalBinding {
            crate_name: "dep".into(),
            stable_crate_id: stable_id(dependency).into_boxed_str(),
            artifacts: vec!["/unjoined/libdep.rlib".into()].into_boxed_slice(),
            authority: None,
            package: None,
        }])
        .expect("unavailable external compiler identity remains bounded evidence");
    let graph = canonicalize(
        &document(&[root]),
        invocations,
        Policy::empty(),
        &api::Context::fixture(),
        &limits,
    )
    .expect("unavailable Cargo authority is a path-local outcome");

    assert_eq!(graph.exports().len(), 2);
    assert_eq!(graph.definitions().len(), 2);
    assert_eq!(
        graph
            .definitions()
            .filter(|definition| matches!(definition.kind(), model::Kind::Rejected { .. }))
            .count(),
        1,
        "the path-local rejection retains its total definition node",
    );
    assert_eq!(
        graph
            .exports()
            .filter(|path| matches!(path, ledger::Path::Imported(_)))
            .count(),
        2
    );
    let reason = graph
        .definitions()
        .find_map(|definition| match definition.kind() {
            model::Kind::Rejected { reason, .. } => Some(reason),
            _ => None,
        });
    assert!(matches!(
        reason,
        Some(ledger::Reason::ExternalAuthorityUnavailable {
            crate_name,
            stable_crate_id
        }) if crate_name.as_ref() == "dep"
            && stable_crate_id.as_ref() == stable_id(dependency)
    ));
}

#[test]
fn missing_non_sysroot_loaded_evidence_remains_a_strict_rejection() {
    let dependency = "dep 1.0.0 (path+dep)";
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["handshake"]["external_crates"] = json!([{
        "crate_name": "dep",
        "stable_crate_id": stable_id(dependency),
        "artifacts": ["/fixture/libdep.rmeta"]
    }]);
    let handshake = serde_json::from_value(root["handshake"].clone())
        .expect("the missing-loaded fixture matches the wire schema");
    assert_code(
        Invocations::seal(vec![handshake], &cargo::Limits::standard()),
        "external-crate-not-loaded",
    );
}

#[test]
fn missing_extra_duplicate_and_mismatched_correlations_reject_before_graph() {
    let first = fragment("one 0.1.0 (path+one)", "one", Some("one"));
    let second = fragment("two 0.1.0 (path+two)", "two", Some("two"));
    let limits = cargo::Limits::standard();
    let context = api::Context::fixture();
    let expected_set = expected(&[first.clone(), second.clone()], &limits);
    let missing = document(std::slice::from_ref(&first));
    assert_code(
        canonicalize(&missing, expected_set, Policy::empty(), &context, &limits),
        "missing-fragment",
    );

    let expected_set = expected(std::slice::from_ref(&first), &limits);
    let extra = document(&[first.clone(), second.clone()]);
    assert_code(
        canonicalize(&extra, expected_set, Policy::empty(), &context, &limits),
        "extra-fragment",
    );

    let expected_set = expected(&[first.clone(), second], &limits);
    let duplicate = document(&[first.clone(), first.clone()]);
    assert_code(
        canonicalize(&duplicate, expected_set, Policy::empty(), &context, &limits),
        "duplicate-unit-correlation",
    );

    let expected_set = expected(std::slice::from_ref(&first), &limits);
    let mut mismatched = first;
    mismatched["handshake"]["probe_arguments_digest"] = Value::String("b".repeat(64));
    let bytes = document(&[mismatched]);
    assert_code(
        canonicalize(&bytes, expected_set, Policy::empty(), &context, &limits),
        "mismatched-unit-correlation",
    );
}

#[test]
fn unknown_schema_cfg_doc_and_non_normal_authority_reject() {
    let root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    let limits = cargo::Limits::standard();
    let context = api::Context::fixture();
    let expected_root = expected(std::slice::from_ref(&root), &limits);
    let bytes = serde_json::to_vec(&json!({"schema":99, "fragments":[root.clone()]}))
        .expect("test protocol serializes");
    assert_code(
        canonicalize(&bytes, expected_root, Policy::empty(), &context, &limits),
        "unknown-sidecar-schema",
    );

    let expected_root = expected(std::slice::from_ref(&root), &limits);
    let mut doc_root = root.clone();
    doc_root["handshake"]["unknown_cfg_doc"] = Value::Bool(true);
    let bytes = document(&[doc_root]);
    assert_code(
        canonicalize(&bytes, expected_root, Policy::empty(), &context, &limits),
        "sidecar-schema",
    );

    let expected_root = expected(std::slice::from_ref(&root), &limits);
    let mut non_normal = root;
    non_normal["handshake"]["compilation"] = Value::String("host".into());
    let bytes = document(&[non_normal]);
    assert_code(
        canonicalize(&bytes, expected_root, Policy::empty(), &context, &limits),
        "sidecar-schema",
    );
}

#[test]
fn sidecar_depth_is_a_physical_bound() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["definitions"] = json!([
        struct_definition(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111",
            ["One"],
            []
        ),
        struct_definition(
            "root 0.1.0 (path+root)",
            "root",
            "22222222222222222222222222222222",
            ["Two"],
            []
        ),
    ]);
    let bytes = document(std::slice::from_ref(&root));
    protocol::decode(&bytes, &cargo::Limits::standard()).expect("the document decodes");

    // Depth is a physical bound of the fixed-frame scanner: it still refuses.
    let mut limits = cargo::Limits::standard();
    limits.depth = 3;
    assert_limit(protocol::decode(&bytes, &limits), "sidecar depth");
}

/// A field type nested past serde_json's default 128 levels decodes, and a
/// fragment that writes each id once (`sidecar::intern`) decodes to the
/// same document as the one that spells them all.
#[test]
fn a_type_nested_past_128_levels_decodes_with_its_ids_written_once() {
    let package = "root 0.1.0 (path+root)";
    let hash = "11111111111111111111111111111111";
    let thing = id(package, "root", hash);
    let mut ty = json!({"kind":"definition", "id":thing, "arguments":[]});
    for _ in 0..50 {
        ty = json!({"kind":"definition", "id":thing, "arguments":[{"kind":"type", "ty":ty}]});
    }
    let mut root = fragment(package, "root", Some("root"));
    root["definitions"] = json!([struct_definition(package, "root", hash, ["Deep"], [ty])]);
    let full = document(std::slice::from_ref(&root));
    let written = sidecar::intern(&serde_json::to_vec(&root).expect("the fragment serializes"));
    let mut interned = format!("{{\"schema\":{},\"fragments\":[", protocol::SCHEMA).into_bytes();
    interned.extend_from_slice(&written);
    interned.extend_from_slice(b"]}");
    assert!(
        interned.len() * 2 < full.len(),
        "{} of {}",
        interned.len(),
        full.len()
    );
    let limits = cargo::Limits::standard();
    let spelled = protocol::decode(&full, &limits).expect("the deep document decodes");
    let once = protocol::decode(&interned, &limits).expect("the interned document decodes");
    assert_eq!(once, spelled);
}

#[test]
fn unknown_top_level_key_is_a_schema_failure_after_the_scan() {
    let bytes = format!(
        r#"{{"schema":{},"fragments":[],"display_path":[0,1]}}"#,
        protocol::SCHEMA
    );
    let limits = cargo::Limits::standard();
    let mut allocation = Allocation::new(&limits);
    assert_code(
        protocol::decode_accounted(bytes.as_bytes(), &limits, &mut allocation),
        "sidecar-schema",
    );
    assert!(!allocation.is_refused());
}

#[test]
fn canonical_rejection_fanout_charges_exact_recursive_owned_payload() {
    let package = identity::Package::new(
        "root 0.1.0 (path+root)".into(),
        "root".into(),
        "0.1.0".into(),
        "path+root".into(),
    );
    let unit = identity::Unit::new(
        identity::Authority::Cargo(package),
        "root:lib".into(),
        "lib".into(),
        "root".into(),
        identity::Compilation::Target,
        Box::new([]),
        "1111111111111111".into(),
    );
    let id = identity::Definition::new(
        unit,
        "11111111111111111111111111111111".into(),
        identity::Namespace::Type,
    );
    let rejected = Rejected {
        reason: ledger::Reason::UnknownCompilerShape {
            definition: id,
            compiler_kind: "fanout-shape".into(),
        },
        span: model::Span::new("src/lib.rs".into(), 0, 1, 1, 0, 1, 1),
    };
    let mut measured = Allocation::with_limits(usize::MAX, usize::MAX);
    measured
        .clone_rejected(&rejected)
        .expect("one recursive rejection clone is measurable");
    let items = measured.items;
    let bytes = measured.bytes;

    let mut both = Allocation::with_limits(usize::MAX, usize::MAX);
    both.clone_rejected(&rejected)
        .and_then(|_| both.clone_rejected(&rejected))
        .expect("the two-path fanout clones both paths");
    assert_eq!(both.usage(), (items * 2, bytes * 2));

    // Terminal only through a latched account: the clone charges nothing.
    let mut refused = Allocation::refused();
    assert_refused(refused.clone_rejected(&rejected));
    assert_eq!(refused.usage(), (0, 0));
}

#[test]
#[cfg(feature = "rustc-driver")]
fn invocation_seal_and_external_bind_share_one_allocation() {
    fn handshake() -> protocol::Handshake {
        let value = fragment("root 0.1.0 (path+root)", "root", Some("root"));
        serde_json::from_value(value["handshake"].clone())
            .expect("the fixture handshake matches the wire schema")
    }

    fn binding() -> ExternalBinding {
        ExternalBinding {
            crate_name: "dep".into(),
            stable_crate_id: "2222222222222222".into(),
            artifacts: Box::from([Box::<str>::from("/target/libdep.rmeta")]),
            authority: Some(crate::input::ArtifactAuthority {
                semantic_key: "33".repeat(32).into_boxed_str(),
                package_id: "dep 1.0.0 (path+dep)".into(),
                target: "/dep/Cargo.toml:lib".into(),
                target_kinds: Box::from([Box::<str>::from("lib")]),
                crate_types: Box::from([Box::<str>::from("rlib")]),
                crate_name: "dep".into(),
                features: Box::from([Box::<str>::from("feature")]),
            }),
            package: Some(("dep".into(), "1.0.0".into(), "path+dep".into())),
        }
    }

    let measured = Allocation::with_limits(usize::MAX, usize::MAX);
    let sealed = Invocations::seal_with_allocation(vec![handshake()], measured)
        .expect("the invocation seals");
    let sealed_usage = sealed.allocation.usage();
    let bound = sealed
        .bind_external(vec![binding()])
        .expect("the external authority binds");
    assert!(bound.allocation.usage().0 > sealed_usage.0);

    // Terminal only through a latched account: the seal's first charge fails.
    assert_refused(Invocations::seal_with_allocation(
        vec![handshake()],
        Allocation::refused(),
    ));
}

#[test]
fn canonicalize_charges_identity_fanout_on_the_canonical_account() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["handshake"]["package"]["name"] = Value::String("identity-fanout".repeat(512));
    let mut definitions = Vec::new();
    let mut reachable = Vec::new();
    for index in 1_u32..=8 {
        let hash = format!("{index:032x}");
        let name = format!("Thing{index}");
        definitions.push(struct_definition(
            "root 0.1.0 (path+root)",
            "root",
            &hash,
            [name.as_str()],
            [],
        ));
        reachable.push(export(
            "root",
            [name.as_str()],
            "type",
            id("root 0.1.0 (path+root)", "root", &hash),
            [],
        ));
    }
    root["definitions"] = Value::Array(definitions);
    root["reachable"] = Value::Array(reachable);
    let bytes = document(std::slice::from_ref(&root));

    let (completed, standard_account) = canonicalize_recorded(
        &bytes,
        expected(std::slice::from_ref(&root), &cargo::Limits::standard()),
        Policy::empty(),
        &api::Context::fixture(),
        &cargo::Limits::standard(),
    )
    .expect("the standard canonical epoch completes the fixture");
    let items = completed.transaction().items;
    assert_eq!(standard_account.usage().0, items);

    // Terminal only through a latched account.
    let Invocations { authority, .. } =
        expected(std::slice::from_ref(&root), &cargo::Limits::standard());
    let refused = Invocations {
        authority,
        allocation: Allocation::refused(),
    };
    assert_refused(canonicalize(
        &bytes,
        refused,
        Policy::empty(),
        &api::Context::fixture(),
        &cargo::Limits::standard(),
    ));
}

#[test]
fn moved_document_decodes_and_accounts_once_with_canonical_cardinality_and_order() {
    let mut zeta = fragment("zeta 0.1.0 (path+zeta)", "zeta", Some("zeta"));
    zeta["definitions"] = json!([struct_definition(
        "zeta 0.1.0 (path+zeta)",
        "zeta",
        "22222222222222222222222222222222",
        ["Zed"],
        [],
    )]);
    zeta["reachable"] = json!([export(
        "zeta",
        ["Zed"],
        "type",
        id(
            "zeta 0.1.0 (path+zeta)",
            "zeta",
            "22222222222222222222222222222222",
        ),
        [],
    )]);
    let mut alpha = fragment("alpha 0.1.0 (path+alpha)", "alpha", Some("alpha"));
    alpha["definitions"] = json!([struct_definition(
        "alpha 0.1.0 (path+alpha)",
        "alpha",
        "11111111111111111111111111111111",
        ["Able"],
        [],
    )]);
    alpha["reachable"] = json!([export(
        "alpha",
        ["Able"],
        "type",
        id(
            "alpha 0.1.0 (path+alpha)",
            "alpha",
            "11111111111111111111111111111111",
        ),
        [],
    )]);
    let bytes = document(&[zeta, alpha]);
    let limits = cargo::Limits::standard();
    protocol::reset_accounted_decodes();
    let mut allocation = Allocation::new(&limits);
    let document = protocol::decode_accounted(&bytes, &limits, &mut allocation)
        .expect("the complete document decodes once");
    let invocations = Invocations::seal_document_with_allocation(&document, allocation)
        .expect("borrowed seal-time invariants hold without moving handshakes");
    let graph = canonicalize_document(
        document,
        invocations,
        Policy::empty(),
        &api::Context::fixture(),
        &limits,
    )
    .expect("the moved document canonicalizes without a second decode");

    assert_eq!(protocol::accounted_decodes(), 1);
    assert_eq!(graph.invocation_count(), 2);
    assert_eq!(graph.definitions().len(), 2);
    assert_eq!(graph.exports().len(), 2);
    assert_eq!(
        graph
            .exports()
            .map(|export| export.key().root_alias())
            .collect::<Vec<_>>(),
        ["alpha", "zeta"],
    );
}

#[test]
fn field_form_schema_and_unit_payload_proofs_fail_closed() {
    let limits = cargo::Limits::standard();
    let old_schema = format!(r#"{{"schema":{},"fragments":[]}}"#, protocol::SCHEMA - 1);
    assert_code(
        protocol::decode(old_schema.as_bytes(), &limits),
        "unknown-sidecar-schema",
    );

    let mut missing = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    let mut missing_definition = struct_definition(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
        ["Thing"],
        [],
    );
    missing_definition["shape"]
        .as_object_mut()
        .expect("fixture struct shape is an object")
        .remove("field_form");
    missing["definitions"] = Value::Array(vec![missing_definition]);
    assert_code(
        protocol::decode(&document(std::slice::from_ref(&missing)), &limits),
        "sidecar-schema",
    );

    let mut malformed = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    let hash = "22222222222222222222222222222222";
    let mut malformed_definition = struct_definition(
        "root 0.1.0 (path+root)",
        "root",
        hash,
        ["UnitWithPayload"],
        [json!({"kind":"primitive", "name":"u8"})],
    );
    malformed_definition["shape"]["field_form"] = Value::String("unit".into());
    malformed["definitions"] = Value::Array(vec![malformed_definition]);
    malformed["reachable"] = Value::Array(vec![export(
        "root",
        ["UnitWithPayload"],
        "type",
        id("root 0.1.0 (path+root)", "root", hash),
        [],
    )]);
    let expected = expected(std::slice::from_ref(&malformed), &limits);
    let graph = canonicalize(
        &document(std::slice::from_ref(&malformed)),
        expected,
        Policy::empty(),
        &api::Context::fixture(),
        &limits,
    )
    .expect("malformed compiler shape becomes one stable rejected definition");
    let definition = graph
        .definitions()
        .next()
        .expect("the malformed declaration remains in the total ledger");
    assert!(matches!(
        definition.kind(),
        model::Kind::Rejected {
            reason: ledger::Reason::UnknownCompilerShape { compiler_kind, .. },
            ..
        } if compiler_kind.as_ref() == "malformed public field proof"
    ));
    assert!(matches!(
        graph.exports().next(),
        Some(ledger::Path::Imported(export)) if export.target() == definition.id()
    ));
}

#[test]
fn type_constant_wire_shape_is_distinct_and_validates_owner_rhs_and_namespace() {
    let package = "root 0.1.0 (path+root)";
    let hash = "71717171717171717171717171717171";
    let mut root = fragment(package, "root", Some("root"));
    let mut type_constant = definition(
        package,
        "root",
        hash,
        ["FREE_LENGTH"],
        json!({
            "kind":"type_const",
            "ty":{"kind":"primitive", "name":"usize"},
            "rhs":{"kind":"present", "value":{"kind":"literal", "ty":"const", "value":"4"}}
        }),
    );
    type_constant["id"] = value_id(package, "root", hash);
    root["definitions"] = json!([type_constant]);
    root["reachable"] = json!([export(
        "root",
        ["FREE_LENGTH"],
        "value",
        value_id(package, "root", hash),
        []
    )]);

    let graph = graph(std::slice::from_ref(&root), Policy::empty());
    let retained = graph
        .definitions()
        .next()
        .expect("the type constant is retained exactly once");
    assert_eq!(retained.id().namespace(), identity::Namespace::Value);
    assert!(matches!(
        retained.kind(),
        model::Kind::TypeConst {
            ty: model::Type::Primitive(ty),
            rhs: model::Default::Present(model::Const::Literal { value, .. }),
        } if ty.as_ref() == "usize" && value.as_ref() == "4"
    ));

    let limits = cargo::Limits::standard();
    let mut missing = root.clone();
    missing["definitions"][0]["shape"]["rhs"] = json!({"kind":"absent"});
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&missing)),
            expected(std::slice::from_ref(&missing), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "type-constant-missing-rhs",
    );

    let mut wrong_namespace = root;
    wrong_namespace["definitions"][0]["id"]["namespace"] = json!("type");
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&wrong_namespace)),
            expected(std::slice::from_ref(&wrong_namespace), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "type-constant-namespace",
    );
}

#[test]
fn uppercase_protocol_digests_and_definition_hashes_reject() {
    let root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    let limits = cargo::Limits::standard();
    let context = api::Context::fixture();

    let expected_root = expected(std::slice::from_ref(&root), &limits);
    let mut uppercase_arguments = root.clone();
    uppercase_arguments["handshake"]["probe_arguments_digest"] = Value::String("A".repeat(64));
    assert_code(
        canonicalize(
            &document(&[uppercase_arguments]),
            expected_root,
            Policy::empty(),
            &context,
            &limits,
        ),
        "invalid-protocol-digest",
    );

    let expected_root = expected(std::slice::from_ref(&root), &limits);
    let mut uppercase_commit = root.clone();
    uppercase_commit["handshake"]["compiler_commit"] = Value::String(COMMIT.to_uppercase());
    assert_code(
        canonicalize(
            &document(&[uppercase_commit]),
            expected_root,
            Policy::empty(),
            &context,
            &limits,
        ),
        "invalid-protocol-digest",
    );

    let expected_root = expected(std::slice::from_ref(&root), &limits);
    let mut uppercase_definition = root;
    uppercase_definition["definitions"] = json!([struct_definition(
        "root 0.1.0 (path+root)",
        "root",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ["Thing"],
        []
    )]);
    assert_code(
        canonicalize(
            &document(&[uppercase_definition]),
            expected_root,
            Policy::empty(),
            &context,
            &limits,
        ),
        "invalid-protocol-digest",
    );
}

#[test]
fn invalid_alias_chain_rejects_without_guessing_a_path() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["definitions"] = json!([struct_definition(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
        ["private", "Thing"],
        []
    )]);
    root["reachable"] = json!([export(
        "root",
        ["Thing"],
        "type",
        id(
            "root 0.1.0 (path+root)",
            "root",
            "11111111111111111111111111111111"
        ),
        [alias(["wrong", "Thing"], ["Thing"])]
    )]);
    let limits = cargo::Limits::standard();
    let expected = expected(std::slice::from_ref(&root), &limits);
    let result = canonicalize(
        &document(&[root]),
        expected,
        Policy::empty(),
        &api::Context::fixture(),
        &limits,
    );
    assert_code(result, "invalid-alias-chain");
}

#[test]
fn duplicate_definition_identity_rejects_before_any_graph_exists() {
    let definition = struct_definition(
        "root 0.1.0 (path+root)",
        "root",
        "11111111111111111111111111111111",
        ["Thing"],
        [],
    );
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["definitions"] = json!([definition.clone(), definition]);
    let limits = cargo::Limits::standard();
    let expected = expected(std::slice::from_ref(&root), &limits);
    let result = canonicalize(
        &document(&[root]),
        expected,
        Policy::empty(),
        &api::Context::fixture(),
        &limits,
    );
    assert_code(result, "duplicate-definition");
}

#[test]
fn sysroot_authority_cannot_collide_with_a_captured_cargo_unit() {
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["handshake"]["sysroot"] = json!([{
        "compiler_commit": COMMIT,
        "selected_target": "aarch64-apple-darwin",
        "crate_name": "root",
        "stable_crate_id": stable_id("root 0.1.0 (path+root)")
    }]);
    let limits = cargo::Limits::standard();
    let expected = expected(std::slice::from_ref(&root), &limits);
    assert_code(
        canonicalize(
            &document(&[root]),
            expected,
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "forged-sysroot-unit",
    );
}

#[test]
fn sysroot_associated_ingress_preserves_exact_override_and_default_owners() {
    let package = "sysroot core";
    let target = "11111111111111111111111111111111";
    let trait_id = "22222222222222222222222222222222";
    let override_method = "33333333333333333333333333333333";
    let default_method = "44444444444444444444444444444444";
    let impl_id = "55555555555555555555555555555555";
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["handshake"]["sysroot"] = json!([{
        "compiler_commit":COMMIT,
        "selected_target":"aarch64-apple-darwin",
        "crate_name":"core",
        "stable_crate_id":stable_id(package)
    }]);
    let target_definition = definition(
        package,
        "core",
        target,
        ["core", "Target"],
        json!({"kind":"sysroot", "compiler_kind":"struct"}),
    );
    let trait_definition = definition(
        package,
        "core",
        trait_id,
        ["core", "Contract"],
        json!({"kind":"sysroot", "compiler_kind":"trait"}),
    );
    let mut overridden = definition(
        package,
        "core",
        override_method,
        ["core", "Target", "overridden"],
        json!({"kind":"sysroot", "compiler_kind":"associated-function"}),
    );
    overridden["id"] = value_id(package, "core", override_method);
    overridden["owner"] = json!({
        "kind":"implementation",
        "id":id(package, "core", impl_id)
    });
    let mut defaulted = definition(
        package,
        "core",
        default_method,
        ["core", "Contract", "defaulted"],
        json!({"kind":"sysroot", "compiler_kind":"associated-function"}),
    );
    defaulted["id"] = value_id(package, "core", default_method);
    defaulted["owner"] = json!({
        "kind":"definition",
        "id":id(package, "core", trait_id)
    });
    root["definitions"] = json!([target_definition, trait_definition, overridden, defaulted,]);
    root["implementations"] = json!([implementation_record(
        package,
        "core",
        impl_id,
        [id(package, "core", target)],
        json!({
            "kind":"trait",
            "id":id(package, "core", trait_id),
            "arguments":[]
        }),
        definition_type(package, "core", target),
        [
            value_id(package, "core", override_method),
            value_id(package, "core", default_method),
        ]
    )]);
    root["reachable"] = json!([export(
        "root",
        ["core", "Target", "overridden"],
        "value",
        value_id(package, "core", override_method),
        []
    )]);

    let graph = graph(std::slice::from_ref(&root), Policy::empty());
    let implementation = graph
        .implementations()
        .next()
        .expect("the direct sysroot associated root admits its exact implementation");
    let overridden = graph
        .definitions()
        .find(|definition| definition.id().def_path_hash() == override_method)
        .expect("the sysroot override is retained");
    assert!(matches!(
        overridden.owner(),
        model::Owner::Implementation(owner) if owner == implementation.id()
    ));
    let defaulted = graph
        .definitions()
        .find(|definition| definition.id().def_path_hash() == default_method)
        .expect("the exact effective sysroot default is retained");
    assert!(matches!(
        defaulted.owner(),
        model::Owner::Definition(owner) if owner.def_path_hash() == trait_id
    ));
}

#[test]
fn sysroot_ingress_rejects_forged_kind_owner_and_authority_evidence() {
    let package = "sysroot core";
    let target = "11111111111111111111111111111111";
    let method = "22222222222222222222222222222222";
    let impl_id = "33333333333333333333333333333333";
    let mut root = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    root["handshake"]["sysroot"] = json!([{
        "compiler_commit":COMMIT,
        "selected_target":"aarch64-apple-darwin",
        "crate_name":"core",
        "stable_crate_id":stable_id(package)
    }]);
    let target_definition = definition(
        package,
        "core",
        target,
        ["core", "Target"],
        json!({"kind":"sysroot", "compiler_kind":"struct"}),
    );
    let mut associated = definition(
        package,
        "core",
        method,
        ["core", "Target", "method"],
        json!({"kind":"sysroot", "compiler_kind":"associated-function"}),
    );
    associated["id"] = value_id(package, "core", method);
    associated["owner"] = json!({
        "kind":"implementation",
        "id":id(package, "core", impl_id)
    });
    root["definitions"] = json!([target_definition, associated]);
    root["implementations"] = json!([implementation_record(
        package,
        "core",
        impl_id,
        [id(package, "core", target)],
        json!({"kind":"inherent"}),
        definition_type(package, "core", target),
        [value_id(package, "core", method)]
    )]);
    root["reachable"] = json!([export(
        "root",
        ["core", "Target", "method"],
        "value",
        value_id(package, "core", method),
        []
    )]);
    let limits = cargo::Limits::standard();

    let mut free_associated = root.clone();
    free_associated["definitions"][1]["owner"] = json!({"kind":"free"});
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&free_associated)),
            expected(std::slice::from_ref(&free_associated), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "mismatched-local-definition",
    );

    let mut owned_free_item = root.clone();
    owned_free_item["definitions"][0]["owner"] = json!({
        "kind":"implementation",
        "id":id(package, "core", impl_id)
    });
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&owned_free_item)),
            expected(std::slice::from_ref(&owned_free_item), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "mismatched-local-definition",
    );

    let mut missing_impl = root.clone();
    missing_impl["definitions"][1]["owner"]["id"]["def_path_hash"] =
        json!("44444444444444444444444444444444");
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&missing_impl)),
            expected(std::slice::from_ref(&missing_impl), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "implementation-owner-cardinality",
    );

    let mut forged_authority = root;
    forged_authority["definitions"][1]["id"]["authority"]["crate_name"] = json!("alloc");
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&forged_authority)),
            expected(std::slice::from_ref(&forged_authority), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "mismatched-local-definition",
    );
}

fn graph(fragments: &[Value], policy: Policy) -> api::Graph<api::Captured> {
    let limits = cargo::Limits::standard();
    canonicalize(
        &document(fragments),
        expected(fragments, &limits),
        policy,
        &api::Context::fixture(),
        &limits,
    )
    .expect("the test sidecars canonicalize")
}

fn evidence(fragments: &[Value]) -> CanonicalizationEvidence {
    let limits = cargo::Limits::standard();
    let bytes = document(fragments);
    let mut allocation = Allocation::new(&limits);
    let document = protocol::decode_accounted(&bytes, &limits, &mut allocation)
        .expect("the evidence fixture decodes");
    let mut invocations = Invocations::seal_document_with_allocation(&document, allocation)
        .expect("the evidence fixture handshakes seal");
    invocations.allocation.moved_document();
    let session = crate::process::EvidenceSession::new();
    let evidence = canonicalize_evidence_document(
        document,
        invocations,
        Policy::empty(),
        &api::Context::fixture(),
        &limits,
        &session,
    )
    .expect("the private fixture canonicalization completes");
    assert_eq!(evidence.counters.decoded_documents, 1);
    assert_eq!(evidence.counters.moved_documents, 1);
    assert!(evidence.counters.document_nodes > 0);
    assert_eq!(evidence.stages.canonicalization.invocations(), 1);
    assert_eq!(evidence.stages.graph_sealing.invocations(), 1);
    evidence
}

#[test]
fn anonymous_import_nine_targets_share_one_binding_without_name_collision() {
    let root = anonymous_fixture(false);
    let graph = graph(&[root], Policy::empty());
    assert_eq!(graph.anonymous_imports().len(), 9);
    assert_eq!(graph.exports().len(), 0);
    assert_eq!(graph.definitions().len(), 0);
}

#[test]
fn anonymous_import_iced_shape_is_eighteen_records_over_nine_targets() {
    let root = anonymous_fixture(true);
    let graph = graph(&[root], Policy::empty());
    let targets = graph
        .anonymous_imports()
        .map(|import| import.target().def_path_hash())
        .collect::<BTreeSet<_>>();
    assert_eq!(graph.anonymous_imports().len(), 18);
    assert_eq!(targets.len(), 9);
}

#[test]
fn anonymous_import_order_is_canonical() {
    let first = anonymous_fixture(true);
    let mut shuffled = first.clone();
    shuffled["reachable"]
        .as_array_mut()
        .expect("reachable fixture is an array")
        .reverse();
    shuffled["definitions"]
        .as_array_mut()
        .expect("definition fixture is an array")
        .reverse();
    assert_eq!(
        graph(&[first], Policy::empty()).canonical_bytes(),
        graph(&[shuffled], Policy::empty()).canonical_bytes(),
    );
}

#[test]
fn anonymous_import_exact_duplicates_are_idempotent() {
    let mut root = anonymous_fixture(false);
    let duplicate = root["reachable"][0].clone();
    root["reachable"] = json!([duplicate.clone(), duplicate]);
    root["definitions"] = json!([root["definitions"][0].clone()]);
    assert_eq!(graph(&[root], Policy::empty()).anonymous_imports().len(), 1);
}

#[test]
fn anonymous_import_distinct_provenance_is_retained() {
    let mut root = anonymous_fixture(false);
    let mut changed = root["reachable"][0].clone();
    changed["reexports"][0]["evidence"]["item"]["def_path_hash"] =
        json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    root["reachable"] = json!([root["reachable"][0].clone(), changed]);
    root["definitions"] = json!([root["definitions"][0].clone()]);
    assert_eq!(graph(&[root], Policy::empty()).anonymous_imports().len(), 2);
}

#[test]
fn anonymous_import_drift_changes_only_support_graph_identity() {
    let original = anonymous_fixture(false);
    let mut changed = original.clone();
    changed["reachable"][0]["reexports"][0]["evidence"]["item"]["def_path_hash"] =
        json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    let original = graph(&[original], Policy::empty());
    let changed = graph(&[changed], Policy::empty());
    assert_ne!(original.fingerprint(), changed.fingerprint());
    assert_eq!(original.exports().len(), changed.exports().len());
    assert_eq!(original.definitions().len(), changed.definitions().len());
    assert_eq!(
        original.implementations().len(),
        changed.implementations().len()
    );
}

#[test]
fn anonymous_import_never_seeds_impl_closure_and_named_demand_still_promotes() {
    let mut anonymous = anonymous_fixture(false);
    anonymous["definitions"] = json!([anonymous["definitions"][0].clone()]);
    anonymous["reachable"] = json!([anonymous["reachable"][0].clone()]);
    let target = id(
        "root 0.1.0 (path+root)",
        "root",
        "00000000000000000000000000000001",
    );
    anonymous["implementations"] = json!([implementation_for(target.clone())]);
    let support_only = graph(std::slice::from_ref(&anonymous), Policy::empty());
    assert_eq!(support_only.definitions().len(), 0);
    assert_eq!(support_only.implementations().len(), 0);

    anonymous["reachable"]
        .as_array_mut()
        .expect("reachable fixture is an array")
        .push(export("root", ["Trait1"], "type", target, []));
    let promoted = graph(&[anonymous], Policy::empty());
    assert_eq!(promoted.anonymous_imports().len(), 1);
    assert_eq!(promoted.exports().len(), 1);
    assert_eq!(promoted.definitions().len(), 1);
    assert_eq!(promoted.implementations().len(), 1);
}

#[test]
fn nameable_collision_is_still_rejected() {
    let mut root = anonymous_fixture(false);
    root["definitions"] = json!([
        root["definitions"][0].clone(),
        root["definitions"][1].clone()
    ]);
    root["reachable"] = json!([
        export(
            "root",
            ["Same"],
            "type",
            id(
                "root 0.1.0 (path+root)",
                "root",
                "00000000000000000000000000000001"
            ),
            []
        ),
        export(
            "root",
            ["Same"],
            "type",
            id(
                "root 0.1.0 (path+root)",
                "root",
                "00000000000000000000000000000002"
            ),
            []
        )
    ]);
    let limits = cargo::Limits::standard();
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&root)),
            expected(std::slice::from_ref(&root), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "path-namespace-collision",
    );
}

#[test]
fn anonymous_import_of_a_nontrait_adds_nothing() {
    let mut root = anonymous_fixture(false);
    root["definitions"] = json!([struct_definition(
        "root 0.1.0 (path+root)",
        "root",
        "00000000000000000000000000000001",
        ["Trait1"],
        [],
    )]);
    root["reachable"] = json!([root["reachable"][0].clone()]);
    // `use Struct as _` brings nothing into scope, as in rustc.
    assert_eq!(graph(&[root], Policy::empty()).anonymous_imports().len(), 0);
}

#[test]
fn anonymous_import_without_alias_fails_closed() {
    let mut root = anonymous_fixture(false);
    root["definitions"] = json!([root["definitions"][0].clone()]);
    root["reachable"] = json!([root["reachable"][0].clone()]);
    root["reachable"][0]["aliases"] = json!([]);
    assert_anonymous_code(root, "anonymous-import-alias");
}

#[test]
fn anonymous_import_without_single_provenance_fails_closed() {
    let mut root = anonymous_fixture(false);
    root["definitions"] = json!([root["definitions"][0].clone()]);
    root["reachable"] = json!([root["reachable"][0].clone()]);
    root["reachable"][0]["reexports"][0]["kind"] = json!("glob");
    assert_anonymous_code(root, "anonymous-import-provenance");
}

#[test]
fn probe_control_locations_are_private_but_handshake_tampering_rejects() {
    let original = fragment("root 0.1.0 (path+root)", "root", Some("root"));
    let mut relocated = original.clone();
    relocated["handshake"]["probe_arguments_digest"] = json!(digest_text("relocated", 64));
    relocated["handshake"]["artifact"]["path"] = json!("/another/cache/libroot.rmeta");
    relocated["handshake"]["loaded_crates"][0]["artifacts"] =
        json!(["/another/cache/libroot.rmeta"]);
    relocated["handshake"]["artifact"]["probe_alias"] = json!("rawr_selected_relocated");
    assert_eq!(
        graph(std::slice::from_ref(&original), Policy::empty()).fingerprint(),
        graph(std::slice::from_ref(&relocated), Policy::empty()).fingerprint(),
    );

    let limits = cargo::Limits::standard();
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&relocated)),
            expected(std::slice::from_ref(&original), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        "mismatched-unit-correlation",
    );
}

fn expected(fragments: &[Value], limits: &cargo::Limits) -> Invocations {
    let handshakes = fragments
        .iter()
        .map(|fragment| {
            serde_json::from_value(fragment["handshake"].clone())
                .expect("test handshake matches the private wire schema")
        })
        .collect();
    Invocations::seal(handshakes, limits).expect("test expectations seal")
}

fn document(fragments: &[Value]) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema":protocol::SCHEMA, "fragments":fragments}))
        .expect("test sidecar document serializes")
}

fn fragment(package_id: &str, crate_name: &str, alias: Option<&str>) -> Value {
    let root = json!({"kind":"selected", "aliases":[alias.unwrap_or(crate_name)]});
    let unit_key = digest_text(package_id, 64);
    json!({
        "handshake": {
            "schema": protocol::SCHEMA,
            "package": {
                "cargo_id": package_id,
                "name": crate_name,
                "version": if package_id.contains("2.0.0") {"2.0.0"} else {"0.1.0"},
                "source": package_id.split_once('(').map_or("path+unknown", |(_, source)| source.trim_end_matches(')')),
            },
            "root": root,
            "target": format!("{crate_name}:lib"),
            "target_kind": "lib",
            "target_kinds": ["lib"],
            "crate_types": ["lib"],
            "crate_name": crate_name,
            "edition": "2024",
            "profile": "dev",
            "compilation": "target",
            "host": "aarch64-apple-darwin",
            "selected_target": "aarch64-apple-darwin",
            "features": [],
            "probe_arguments_digest": ARGUMENTS,
            "compiler_commit": COMMIT,
            "invocation_nonce": digest_text(&format!("nonce:{package_id}"), 64),
            "unit_correlation": digest_text(&format!("unit:{package_id}"), 64),
            "unit_key": unit_key,
            "dependencies": [],
            "artifact": {
                "path": format!("/fixture/lib{crate_name}.rmeta"),
                "length": 1,
                "digest": ARGUMENTS,
                "profile": "{}",
                "fresh": false,
                "crate_hash": digest_text(&format!("crate:{package_id}"), 32),
                "stable_crate_id": stable_id(package_id),
                "probe_alias": format!("rawr_selected_{crate_name}")
            },
            "sysroot": [],
            "loaded_crates": [{
                "crate_name": crate_name,
                "stable_crate_id": stable_id(package_id),
                "artifacts": [format!("/fixture/lib{crate_name}.rmeta")]
            }],
            "external_crates": [],
        },
        "reachable": [],
        "definitions": [],
        "implementations": [],
        "scalar_predicate_proofs": []
    })
}

fn id(package_id: &str, crate_name: &str, hash: &str) -> Value {
    json!({
        "authority": {
            "kind":"compiler",
            "crate_name": crate_name,
            "stable_crate_id": stable_id(package_id)
        },
        "def_path_hash": hash,
        "namespace": "type"
    })
}

fn value_id(package_id: &str, crate_name: &str, hash: &str) -> Value {
    let mut value = id(package_id, crate_name, hash);
    value["namespace"] = Value::String("value".into());
    value
}

fn stable_id(package_id: &str) -> String {
    digest_text(package_id, 16)
}

fn digest_text(value: &str, length: usize) -> String {
    let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
    digest[..length].to_owned()
}

fn definition<const N: usize>(
    package_id: &str,
    crate_name: &str,
    hash: &str,
    path: [&str; N],
    shape: Value,
) -> Value {
    let path = path.to_vec();
    let identity = if shape["kind"].as_str() == Some("function") {
        value_id(package_id, crate_name, hash)
    } else {
        id(package_id, crate_name, hash)
    };
    json!({
        "id": identity,
        "display_path": path,
        "owner": {"kind":"free"},
        "visibility": {"kind":"public"},
        "shape": shape,
        "span": span_value(),
        "origin": {"kind":"source"}
    })
}

fn struct_definition<const N: usize, const M: usize>(
    package_id: &str,
    crate_name: &str,
    hash: &str,
    path: [&str; N],
    fields: [Value; M],
) -> Value {
    let fields = fields
        .into_iter()
        .enumerate()
        .map(|(index, ty)| {
            json!({
                "name": format!("field_{index}"),
                "index": index,
                "visibility": {"kind":"public"},
                "ty": ty,
                "span": span_value()
            })
        })
        .collect::<Vec<_>>();
    definition(
        package_id,
        crate_name,
        hash,
        path,
        json!({
            "kind":"struct",
            "field_form":"record",
            "fields":fields,
            "fields_complete":true,
            "non_exhaustive":false,
            "generics":empty_generics(),
            "layout":{"kind":"absent"}
        }),
    )
}

fn trait_definition<const N: usize>(
    package_id: &str,
    crate_name: &str,
    hash: &str,
    path: [&str; N],
) -> Value {
    definition(
        package_id,
        crate_name,
        hash,
        path,
        json!({
            "kind":"trait",
            "unsafe_trait":false,
            "auto_trait":false,
            "supertraits":[],
            "associated":[],
            "generics":empty_generics()
        }),
    )
}

fn anonymous_fixture(two_paths: bool) -> Value {
    let package = "root 0.1.0 (path+root)";
    let mut root = fragment(package, "root", Some("root"));
    let mut definitions = Vec::new();
    let mut reachable = Vec::new();
    for index in 1_u32..=9 {
        let target_hash = format!("{index:032x}");
        let name = format!("Trait{index}");
        definitions.push(trait_definition(
            package,
            "root",
            &target_hash,
            [name.as_str()],
        ));
        reachable.push(anonymous_export(
            "root",
            &["advanced", "graphics", "futures", "futures", "prelude", "_"],
            &[name.as_str()],
            id(package, "root", &target_hash),
            &format!("{:032x}", 0x100_u32 + index),
        ));
        if two_paths {
            reachable.push(anonymous_export(
                "root",
                &["futures", "prelude", "_"],
                &[name.as_str()],
                id(package, "root", &target_hash),
                &format!("{:032x}", 0x200_u32 + index),
            ));
        }
    }
    root["definitions"] = Value::Array(definitions);
    root["reachable"] = Value::Array(reachable);
    root
}

fn anonymous_export(
    root_alias: &str,
    path: &[&str],
    target_path: &[&str],
    mut target: Value,
    reexport_hash: &str,
) -> Value {
    target["namespace"] = json!("type");
    json!({
        "binding":"anonymous_import",
        "root_alias":root_alias,
        "path":path,
        "namespace":"type",
        "target":{"kind":"definition", "id":target},
        "aliases":[{
            "from":target_path,
            "to":path,
            "span":span_value()
        }],
        "reexports":[{
            "kind":"single",
            "evidence":{
                "kind":"identified",
                "item":{
                    "crate_name":"root",
                    "stable_crate_id":stable_id("root 0.1.0 (path+root)"),
                    "def_path_hash":reexport_hash
                },
                "span":span_value()
            }
        }],
        "span":span_value()
    })
}

fn implementation_for(target: Value) -> Value {
    json!({
        "key":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "id":id(
            "root 0.1.0 (path+root)",
            "root",
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
        ),
        "relevance":[target.clone()],
        "trait_ref":{"kind":"inherent"},
        "self_type":{"kind":"definition", "id":target, "arguments":[]},
        "safety":"safe",
        "polarity":"positive",
        "const_impl":false,
        "default_impl":false,
        "generics":empty_generics(),
        "associated":[],
        "span":span_value(),
        "origin":{"kind":"source"}
    })
}

fn implementation_record<const R: usize, const A: usize>(
    package_id: &str,
    crate_name: &str,
    hash: &str,
    relevance: [Value; R],
    trait_ref: Value,
    self_type: Value,
    associated: [Value; A],
) -> Value {
    let relevance = relevance.into_iter().collect::<Vec<_>>();
    let associated = associated.into_iter().collect::<Vec<_>>();
    json!({
        "key":hash,
        "id":id(package_id, crate_name, hash),
        "relevance":relevance,
        "trait_ref":trait_ref,
        "self_type":self_type,
        "safety":"safe",
        "polarity":"positive",
        "const_impl":false,
        "default_impl":false,
        "generics":empty_generics(),
        "associated":associated,
        "span":span_value(),
        "origin":{"kind":"source"}
    })
}

fn assert_anonymous_code(root: Value, expected_code: &'static str) {
    let limits = cargo::Limits::standard();
    assert_code(
        canonicalize(
            &document(std::slice::from_ref(&root)),
            expected(std::slice::from_ref(&root), &limits),
            Policy::empty(),
            &api::Context::fixture(),
            &limits,
        ),
        expected_code,
    );
}

fn module_definition<const N: usize>(
    package_id: &str,
    crate_name: &str,
    hash: &str,
    path: [&str; N],
) -> Value {
    definition(package_id, crate_name, hash, path, json!({"kind":"module"}))
}

fn function_definition<const N: usize>(
    package_id: &str,
    crate_name: &str,
    hash: &str,
    path: [&str; N],
    output: Value,
) -> Value {
    definition(
        package_id,
        crate_name,
        hash,
        path,
        json!({
            "kind":"function",
            "signature": {
                "safety":"safe", "const_function":false, "async_function":false,
                "abi":"\"Rust\"", "variadic":false,
                "binders":[], "receiver":{"kind":"none"}, "inputs":[], "output":output
            },
            "generics": empty_generics()
        }),
    )
}

fn definition_type(package_id: &str, crate_name: &str, hash: &str) -> Value {
    json!({"kind":"definition", "id":id(package_id, crate_name, hash), "arguments":[]})
}

fn export<const N: usize, const M: usize>(
    root_alias: &str,
    path: [&str; N],
    namespace: &str,
    mut target: Value,
    aliases: [Value; M],
) -> Value {
    let path = path.to_vec();
    let aliases = aliases.into_iter().collect::<Vec<_>>();
    target["namespace"] = Value::String(namespace.into());
    json!({
        "binding":"nameable",
        "root_alias":root_alias,
        "path":path,
        "namespace":namespace,
        "target":{"kind":"definition", "id":target},
        "aliases":aliases,
        "reexports":[],
        "span":span_value()
    })
}

fn alias<const N: usize, const M: usize>(from: [&str; N], to: [&str; M]) -> Value {
    let from = from.to_vec();
    let to = to.to_vec();
    json!({"from":from, "to":to, "span":span_value()})
}

fn empty_generics() -> Value {
    json!({"parameters":[], "predicates":[]})
}

fn span_value() -> Value {
    json!({
        "file":"src/lib.rs", "byte_start":0, "byte_end":8,
        "line_start":1, "column_start":0, "line_end":1, "column_end":8
    })
}

fn test_span() -> model::Span {
    model::Span::new("Cargo.toml".into(), 4, 12, 2, 0, 2, 8)
}

fn assert_code<T>(result: Result<T>, expected: &'static str) {
    let error = result.err().expect("the malformed protocol must reject");
    assert!(
        matches!(&error, Error::Extraction { code, .. } if *code == expected),
        "expected extraction code `{expected}`, got `{error}`"
    );
}

/// Physical bounds only: `depth` and the fixed scanner frames refuse.
fn assert_limit<T>(result: Result<T>, expected: &'static str) {
    let error = result
        .err()
        .expect("the protocol over a physical bound must reject");
    assert!(
        matches!(&error, Error::Limit { resource, .. } if *resource == expected),
        "expected physical bound `{expected}`, got `{error}`"
    );
}

/// The only terminal canonical-account outcome is a genuine cause that
/// latched the account; every later charge fails as the refused account.
fn assert_refused<T>(result: Result<T>) {
    let error = result.err().expect("a latched account must refuse");
    assert!(
        matches!(&error, Error::Node { operation, .. } if *operation == REFUSED_ACCOUNT_OPERATION),
        "expected the refused canonical account, got `{error}`"
    );
}

/// `canonicalize` with the canonical account returned beside the graph, so a
/// test can read the usage the transaction charged.
fn canonicalize_recorded(
    bytes: &[u8],
    invocations: Invocations,
    policy: Policy,
    context: &api::Context,
    limits: &cargo::Limits,
) -> Result<(api::Graph<api::Captured>, Allocation)> {
    let Invocations {
        authority,
        mut allocation,
    } = invocations;
    allocation.retain(policy.usage)?;
    let document = protocol::decode_accounted(bytes, limits, &mut allocation)?;
    let fragments = correlate(document.fragments, &authority, &mut allocation)?;
    let graph = canonicalize_fragments(
        fragments,
        authority,
        &policy,
        context,
        limits,
        &mut allocation,
        SealMode::Direct,
    )?;
    Ok((graph, allocation))
}
