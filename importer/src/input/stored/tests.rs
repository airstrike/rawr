use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use super::{codec, entries, Budget, Error, Package, Projection, Recipe, Stored, MAGIC};
use crate::cargo::{self, toolchain, universe};
use crate::input::{self, package, proof, Allocation, Baseline, Usage};
use crate::node;

fn path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()))
}

fn expected() -> Box<[proof::Entry]> {
    let mut entries = vec![
        proof::Entry::Workspace {
            paths: Box::from([Box::<str>::from("/unobserved/workspace/Cargo.toml")]),
            digest: [9; 32],
        },
        proof::Entry::File {
            role: proof::Role::Authority,
            package_id: Some("pkg".into()),
            path: "/unobserved/input".into(),
            bytes: 3,
            digest: [1; 32],
        },
        proof::Entry::Directory {
            role: proof::Role::OutDirectory,
            package_id: Some("pkg".into()),
            path: "/unobserved/out".into(),
            directories: vec![proof::TreeDirectory {
                path: "empty".into(),
            }]
            .into_boxed_slice(),
            files: vec![proof::TreeFile {
                path: "source".into(),
                bytes: 7,
                digest: [2; 32],
            }]
            .into_boxed_slice(),
        },
        proof::Entry::Registry {
            role: proof::Role::DefaultPackageSource,
            package_id: "registry#pkg".into(),
            source: "registry".into(),
            checksum: [3; 32],
            root: "/unobserved/registry".into(),
            marker_schema: 1,
            marker_digest: [4; 32],
        },
        proof::Entry::Git {
            role: proof::Role::DefaultPackageSource,
            package_id: "git#pkg".into(),
            source: "git+fixture".into(),
            oid: "abc".into(),
            root: "/unobserved/git".into(),
            files: Box::default(),
        },
        proof::Entry::Missing {
            role: proof::Role::Config,
            package_id: None,
            path: "/unobserved/missing".into(),
        },
        proof::Entry::Environment {
            package_id: "pkg".into(),
            execution: "out".into(),
            key: "KEY".into(),
            value: Some("".into()),
        },
        proof::Entry::EnvironmentProjection {
            schema: 3,
            key: "ABSENT".into(),
            value: None,
        },
        proof::Entry::Fact {
            role: proof::Role::Config,
            package_id: None,
            name: "escaped\"\n".into(),
            value: "\u{0} é \\".into(),
        },
        proof::Entry::BuildMessage {
            package_id: "pkg".into(),
            execution: "/unobserved/out".into(),
            name: "saved-output".into(),
            source: Some("/unobserved/output".into()),
            value: "abcdef".into(),
        },
        proof::Entry::Directive {
            package_id: "pkg".into(),
            execution: "out".into(),
            index: 9,
            value: "cargo::rerun-if-env-changed=KEY".into(),
        },
    ];
    for role in [
        proof::Role::Authority,
        proof::Role::Harness,
        proof::Role::Config,
        proof::Role::Toolchain,
        proof::Role::Target,
        proof::Role::Profile,
        proof::Role::PackageManifest,
        proof::Role::DependencySource,
        proof::Role::DepInfo,
        proof::Role::BuildScript,
        proof::Role::DeclaredInput,
        proof::Role::DefaultPackageSource,
        proof::Role::OutDirectory,
        proof::Role::BuiltOutput,
        proof::Role::BuildMessage,
    ] {
        entries.push(proof::Entry::Fact {
            role,
            package_id: Some("pkg".into()),
            name: "all-roles".into(),
            value: "v".into(),
        });
    }
    entries.sort_unstable();
    entries.into_boxed_slice()
}

pub(super) fn baseline() -> Baseline {
    Baseline {
        #[cfg(feature = "rustc-driver")]
        partitions: None,
        proof: proof::Proof::fixture_stored(expected()),
        recipes: vec![
            proof::Recipe::File {
                role: proof::Role::Authority,
                package_id: Some("pkg".into()),
                path: path(b"/unobserved/\xff/file"),
            },
            proof::Recipe::Path {
                role: proof::Role::Config,
                package_id: None,
                path: path(b"/unobserved/missing"),
            },
            proof::Recipe::Directory {
                role: proof::Role::OutDirectory,
                package_id: Some("pkg".into()),
                path: path(b"/unobserved/out"),
            },
            proof::Recipe::DepInfo {
                package_id: "pkg".into(),
                path: path(b"/unobserved/a.d"),
                package_root: path(b"/unobserved/pkg"),
                out_directories: vec![path(b"/unobserved/out-a"), path(b"/unobserved/out-b")]
                    .into_boxed_slice(),
            },
            proof::Recipe::SavedOutput {
                package_id: "pkg".into(),
                out_dir: path(b"/unobserved/out"),
            },
            proof::Recipe::Package(package::Set::fixture_stored_registry()),
            proof::Recipe::Environment {
                package_id: "pkg".into(),
                execution: "out".into(),
                key: "KEY".into(),
                value: Some("".into()),
            },
            proof::Recipe::EnvironmentProjection {
                schema: 3,
                key: "ABSENT".into(),
                value: None,
            },
            proof::Recipe::Fact {
                role: proof::Role::Profile,
                package_id: None,
                name: "profile".into(),
                value: "dev".into(),
            },
            proof::Recipe::BuildMessage {
                package_id: "pkg".into(),
                execution: "out".into(),
                name: "cfg".into(),
                value: "feature=\"value\"".into(),
            },
            proof::Recipe::Directive {
                package_id: "pkg".into(),
                execution: "out".into(),
                index: 4,
                value: "cargo::rerun-if-changed=src".into(),
            },
            proof::Recipe::Workspace {
                roots: Box::from([path(b"/unobserved/workspace/Cargo.toml")]),
                graph: node::GraphBudget::new(),
            },
        ]
        .into_boxed_slice(),
        recipe_allocation: Usage {
            items: 101,
            bytes: 8192,
        },
        allocation: Allocation::for_proof(
            Usage {
                items: 151,
                bytes: 16384,
            },
            &cargo::Limits::standard(),
        ),
        universe_build: Some(universe::BuildProjection::from_canonical(
            b"[4,[]]\n".as_slice().into(),
        )),
        package_replay: None,
    }
}

fn empty() -> Baseline {
    Baseline {
        #[cfg(feature = "rustc-driver")]
        partitions: None,
        proof: proof::Proof::fixture_stored(Box::default()),
        recipes: Box::default(),
        recipe_allocation: Usage { items: 0, bytes: 0 },
        allocation: Allocation::for_proof(
            Usage {
                items: 7,
                bytes: 31,
            },
            &cargo::Limits::standard(),
        ),
        universe_build: None,
        package_replay: None,
    }
}

fn data() -> Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let (stored, _) = Stored::capture(baseline(), &limits, &graph).into_parts();
    stored.expect("cold data moves")
}

#[test]
fn replay_digest_admits_exact_bytes_records_exceedance_and_latches_on_failure() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let bytes = b"retained manifest";
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize - bytes.len())
        .expect("leave exact digest allowance");
    let mut allocation = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    assert_eq!(
        budget.digest(bytes).expect("exact allowance"),
        cargo::digest(bytes)
    );
    assert!(
        !graph.exceedances().is_exceeded(),
        "exact target is not an exceedance"
    );
    // The hashed-bytes target is an accounting axis. One byte over is
    // still digested; the graph records the axis with a truthful high water.
    assert_eq!(
        budget
            .digest(b"x")
            .expect("one over the target still hashes"),
        cargo::digest(b"x")
    );
    let record = graph.exceedances();
    let axis = record
        .axis(node::HASHED_RESOURCE)
        .expect("hashed axis recorded");
    assert_eq!(axis.target, node::MAX_HASHED_BYTES_PER_GRAPH);
    assert_eq!(axis.first_requested, node::MAX_HASHED_BYTES_PER_GRAPH + 1);
    assert_eq!(axis.high_water, node::MAX_HASHED_BYTES_PER_GRAPH + 1);
    assert!(budget.check().is_ok(), "an exceedance never latches");
    // A genuine failure (checked-arithmetic overflow) latches the account.
    assert!(matches!(
        budget.hash(usize::MAX),
        Err(Error::Graph(node::Error::Allocation))
    ));
    assert!(matches!(budget.digest(b""), Err(Error::Refused)));
    let fresh_graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &fresh_graph);
    assert!(matches!(budget.digest(bytes), Err(Error::Refused)));
}

fn encode(value: &Stored) -> Vec<u8> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    value
        .encode(&mut Budget::borrow(&limits, &mut allocation, &graph))
        .expect("data encodes")
}

fn decode(bytes: &[u8]) -> super::Result<Stored> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    Stored::decode(bytes, &mut Budget::borrow(&limits, &mut allocation, &graph))
}

fn proof_offset() -> usize {
    MAGIC.len() + 8 + toolchain::supported_cargo_commit().len()
}

#[test]
fn every_representable_recipe_and_expected_entry_roundtrips_exactly() {
    let stored = data();
    let encoded = encode(&stored);
    let decoded = decode(&encoded).expect("complete data decodes");
    assert_eq!(decoded.recipes().len(), 12);
    assert_eq!(decoded.entries().len(), 26);
    assert_eq!(encode(&decoded), encoded);
    assert_eq!(decoded.schema(), proof::SCHEMA);
    assert_eq!(
        decoded.usage(),
        Usage {
            items: 101,
            bytes: 8192
        }
    );
    assert_eq!(decoded.fingerprint(), stored.fingerprint());
    assert!(decoded.entries() == stored.entries());
    let projection = decoded.projection().expect("protected bytes retained");
    assert_eq!(projection.bytes(), b"[4,[]]\n");
    assert_eq!(projection.schema(), universe::BUILD_PROJECTION_SCHEMA);
    assert_eq!(
        projection.digest(),
        stored.projection().expect("cold projection").digest()
    );
    let Recipe::File { path, .. } = &decoded.recipes()[0] else {
        panic!("file recipe");
    };
    assert_eq!(
        path.as_os_str().as_encoded_bytes(),
        b"/unobserved/\xff/file"
    );
    let Recipe::Package(Package::Registry(registry)) = &decoded.recipes()[5] else {
        panic!("registry recipe");
    };
    assert_eq!(registry.base_usage, Usage { items: 2, bytes: 3 });
    assert_eq!(
        registry.usage,
        Usage {
            items: 11,
            bytes: 127
        }
    );
    assert_eq!(registry.marker_digest, [8; 32]);
    assert_eq!(registry.root, path_from("/unobserved/registry/pkg"));
}

fn path_from(value: &str) -> PathBuf {
    PathBuf::from(value)
}

#[test]
fn capture_moves_payloads_and_charges_only_the_new_recipe_layout() {
    let baseline = baseline();
    let prior = baseline.allocation.usage();
    let recipe_usage = baseline.recipe_allocation;
    assert!(prior.items > recipe_usage.items);
    assert!(prior.bytes > recipe_usage.bytes);
    let expected_pointer = baseline.proof.stored_parts().1.as_ptr();
    let recipe_pointer = match &baseline.recipes[0] {
        proof::Recipe::File { path, .. } => path.as_os_str().as_encoded_bytes().as_ptr(),
        _ => panic!("file recipe"),
    };
    let protected_pointer = baseline
        .universe_build
        .as_ref()
        .expect("projection")
        .bytes()
        .as_ptr();
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .expect("hash floor");
    let capture = Stored::capture(baseline, &limits, &graph);
    assert_eq!(capture.allocations, 1);
    let (stored, allocation) = capture.into_parts();
    let stored = stored.expect("capture moves without rehashing");
    assert_eq!(
        allocation.usage(),
        Usage {
            items: prior.items + 12,
            bytes: prior.bytes + 12 * std::mem::size_of::<Recipe>()
        }
    );
    assert_eq!(stored.usage(), recipe_usage);
    assert_eq!(stored.entries().as_ptr(), expected_pointer);
    let Recipe::File { path, .. } = &stored.recipes()[0] else {
        panic!("file recipe");
    };
    assert_eq!(path.as_os_str().as_encoded_bytes().as_ptr(), recipe_pointer);
    assert_eq!(
        stored.projection().expect("projection").bytes().as_ptr(),
        protected_pointer
    );
}

#[test]
fn capture_keeps_spending_after_actual_proof_and_protected_projection() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    allocation.items(1).expect("recipe slot");
    let recipes: Box<[proof::Recipe]> =
        Box::from([
            proof::Recipe::fact(proof::Role::Profile, "profile", "dev", &mut allocation)
                .expect("actual recipe transformation"),
        ]);
    let recipe_allocation = allocation.usage();
    let proof = proof::evaluate(&recipes, false, &limits, &mut allocation)
        .expect("actual cold proof evaluation");
    let proof_usage = allocation.usage();
    assert!(proof_usage.items > recipe_allocation.items);
    assert!(proof_usage.bytes > recipe_allocation.bytes);
    let evidence = input::build::Evidence {
        usage: Usage { items: 0, bytes: 0 },
        packages: Default::default(),
        scripts: Box::default(),
        artifacts: Box::default(),
        environment: Box::default(),
    };
    let protected = universe::MetadataProof::fixture_package(
        "fixture#pkg@1.0.0",
        "pkg",
        "1.0.0",
        "",
        path(b"/unobserved/pkg"),
    );
    let projection = proof::protected_digest(
        &proof,
        &evidence,
        &protected,
        "aarch64-apple-darwin",
        &limits,
        &mut allocation,
    )
    .expect("actual protected projection transformation");
    let final_usage = allocation.usage();
    assert!(final_usage.bytes > proof_usage.bytes);
    let pointer = projection.bytes().as_ptr();
    let baseline = Baseline {
        #[cfg(feature = "rustc-driver")]
        partitions: None,
        proof,
        recipes,
        recipe_allocation,
        allocation,
        universe_build: Some(projection),
        package_replay: None,
    };
    let (stored, mut allocation) = Stored::capture(baseline, &limits, &graph).into_parts();
    let stored = stored.expect("capture uses final account");
    assert_eq!(stored.usage(), recipe_allocation);
    assert_eq!(
        stored.projection().expect("projection").bytes().as_ptr(),
        pointer
    );
    assert_eq!(
        allocation.usage(),
        Usage {
            items: final_usage.items + 1,
            bytes: final_usage.bytes + std::mem::size_of::<Recipe>(),
        }
    );
    let before_encode = allocation.usage();
    let encoded = stored
        .encode(&mut Budget::borrow(&limits, &mut allocation, &graph))
        .expect("same account continues into encode");
    assert_eq!(
        allocation.usage(),
        Usage {
            items: before_encode.items + 1,
            bytes: before_encode.bytes + encoded.len(),
        }
    );
}

#[test]
fn capture_obeys_original_exact_remaining_layout_and_records_exceedance() {
    let limits = cargo::Limits::standard();
    for item_floor in [false, true] {
        for floor in [0, 1] {
            let mut baseline = baseline();
            let prior = baseline.allocation.usage();
            let count = baseline.recipes.len();
            let bytes = count * std::mem::size_of::<Recipe>();
            baseline.allocation.item_limit =
                prior.items + count - if item_floor { floor } else { 0 };
            baseline.allocation.byte_limit =
                prior.bytes + bytes - if item_floor { 0 } else { floor };
            let graph = node::GraphBudget::new();
            let capture = Stored::capture(baseline, &limits, &graph);
            // The layout is charged in full either way; a one-under
            // ceiling records the axis instead of refusing the capture.
            assert_eq!(capture.allocations, 1);
            let (result, mut allocation) = capture.into_parts();
            let stored = result.expect("original remaining layout is admitted");
            assert_eq!(
                allocation.usage(),
                Usage {
                    items: prior.items + count,
                    bytes: prior.bytes + bytes,
                }
            );
            let record = allocation.exceedances();
            if floor == 0 {
                assert!(!record.is_exceeded(), "exact remaining layout");
                let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
                // One byte past the exact remaining layout is owned and recorded.
                budget
                    .bytes(b"x")
                    .expect("one over the target is still owned");
                let record = budget.exceedances();
                let axis = record
                    .axis(input::INPUT_BYTES_RESOURCE)
                    .expect("byte axis recorded");
                assert_eq!(axis.target as usize, prior.bytes + bytes);
                assert_eq!(axis.high_water as usize, prior.bytes + bytes + 1);
                assert!(budget.check().is_ok());
                // A genuine failure latches; the latched account encodes nothing.
                budget.refuse();
                let allocations = budget.allocations();
                assert!(matches!(stored.encode(&mut budget), Err(Error::Refused)));
                assert_eq!(budget.allocations(), allocations);
            } else {
                let (name, total) = if item_floor {
                    (input::INPUT_ITEMS_RESOURCE, prior.items + count)
                } else {
                    (input::INPUT_BYTES_RESOURCE, prior.bytes + bytes)
                };
                let axis = record.axis(name).expect("one-under axis recorded");
                assert_eq!(axis.target as usize, total - 1);
                assert_eq!(axis.first_requested as usize, total);
                assert_eq!(axis.high_water as usize, total, "exact layout charge");
                assert_eq!(record.axes().count(), 1);
                assert!(allocation.bytes(0).is_ok(), "an exceedance never latches");
            }
            // A genuine failure latches the account terminally.
            allocation.refuse();
            let spent = allocation.usage();
            assert!(matches!(
                allocation.bytes(0),
                Err(crate::Error::InputAccountRefused)
            ));
            let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
            assert!(matches!(budget.items::<u8>(0), Err(Error::Refused)));
            assert_eq!(budget.allocations(), 0);
            assert_eq!(budget.usage(), spent);
        }
    }
}

#[test]
fn already_refused_baseline_cannot_capture_even_an_empty_record() {
    let limits = cargo::Limits::standard();
    let mut baseline = empty();
    let prior = baseline.allocation.usage();
    baseline.allocation.byte_limit = prior.bytes;
    // One byte past the ceiling is charged and recorded, never refused.
    baseline
        .allocation
        .bytes(1)
        .expect("one over the byte target is still charged");
    let record = baseline.allocation.exceedances();
    let axis = record
        .axis(input::INPUT_BYTES_RESOURCE)
        .expect("byte axis recorded");
    assert_eq!(axis.target as usize, prior.bytes);
    assert_eq!(axis.high_water as usize, prior.bytes + 1);
    let prior = baseline.allocation.usage();
    // A genuine failure latches the baseline account before capture.
    baseline.allocation.refuse();
    let graph = node::GraphBudget::new();
    let capture = Stored::capture(baseline, &limits, &graph);
    assert_eq!(capture.allocations, 0);
    let (result, mut allocation) = capture.into_parts();
    assert!(matches!(result, Err(Error::Refused)));
    assert_eq!(allocation.usage(), prior);
    assert!(matches!(
        Budget::borrow(&limits, &mut allocation, &graph).bytes(b""),
        Err(Error::Refused)
    ));
}

#[test]
fn path_and_git_capture_refuse_missing_witness_before_any_owned_work() {
    for git in [false, true] {
        let mut baseline = empty();
        baseline.recipes = vec![proof::Recipe::Package(package::Set::fixture_stored_listed(
            git,
        ))]
        .into_boxed_slice();
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let capture = Stored::capture(baseline, &limits, &graph);
        assert_eq!(capture.allocations, 0);
        let (result, allocation) = capture.into_parts();
        assert!(matches!(result, Err(Error::IncompleteWitness)));
        assert_eq!(
            allocation.usage(),
            Usage {
                items: 7,
                bytes: 31
            }
        );
    }
}

#[test]
fn codec_never_inspects_any_retained_source_path() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
        .expect("all filesystem work is already spent");
    let (stored, mut allocation) = Stored::capture(baseline(), &limits, &graph).into_parts();
    let stored = stored.expect("inert paths are never opened");
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let encoded = stored.encode(&mut budget).expect("encode is pure");
    Stored::decode(&encoded, &mut budget).expect("decode is pure");
}

#[test]
fn every_truncated_empty_record_and_trailing_byte_rejects() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let (stored, _) = Stored::capture(empty(), &limits, &graph).into_parts();
    let stored = stored.expect("empty data");
    let bytes = encode(&stored);
    for length in 0..bytes.len() {
        assert!(decode(&bytes[..length]).is_err(), "prefix {length}");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(matches!(
        decode(&trailing),
        Err(Error::Encoding("trailing record bytes"))
    ));
    assert!(decode(&bytes).is_ok());
}

#[test]
fn old_schema_unknown_pin_and_unknown_expected_schema_reject() {
    let bytes = encode(&data());
    for offset in [MAGIC.len() - 1, MAGIC.len() + 8, proof_offset()] {
        let mut changed = bytes.clone();
        changed[offset] ^= 0x40;
        assert!(decode(&changed).is_err(), "identity byte {offset}");
    }
}

#[test]
fn pre_workspace_proof_schema_cannot_decode_as_current_evidence() {
    let mut bytes = encode(&data());
    assert_eq!(proof::SCHEMA, 10);
    let offset = proof_offset();
    bytes[offset..offset + 4].copy_from_slice(&9_u32.to_le_bytes());
    assert!(decode(&bytes).is_err());
}

#[test]
fn workspace_recipe_and_watch_paths_require_canonical_order() {
    let mut stored = data();
    let recipe = stored
        .recipes
        .iter_mut()
        .find_map(|recipe| match recipe {
            Recipe::Workspace { roots } => Some(roots),
            _ => None,
        })
        .expect("workspace recipe");
    *recipe = Box::from([path(b"/z/Cargo.toml"), path(b"/a/Cargo.toml")]);
    assert!(decode(&encode(&stored)).is_err());
    let mut stored = data();
    let watched = stored
        .entries
        .iter_mut()
        .find_map(|entry| match entry {
            proof::Entry::Workspace { paths, .. } => Some(paths),
            _ => None,
        })
        .expect("workspace entry");
    *watched = Box::from([
        Box::<str>::from("/z/Cargo.toml"),
        Box::<str>::from("/a/Cargo.toml"),
    ]);
    assert!(decode(&encode(&stored)).is_err());
}

#[test]
fn canonical_entry_order_and_fingerprint_are_checked() {
    let mut stored = data();
    stored.entries.swap(0, 1);
    assert!(matches!(
        decode(&encode(&stored)),
        Err(Error::Encoding("expected entries are not strictly ordered"))
    ));
    stored.entries.swap(0, 1);
    let mut changed = *stored.fingerprint.as_bytes();
    changed[0] ^= 1;
    stored.fingerprint = input::fingerprint::Fingerprint::from_bytes(changed);
    assert!(matches!(
        decode(&encode(&stored)),
        Err(Error::Encoding("cold proof fingerprint differs"))
    ));
}

#[test]
fn protection_digest_and_schema_are_checked_without_claiming_current_authentication() {
    let bytes = encode(&data());
    let mut changed = bytes.clone();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    assert!(matches!(
        decode(&changed),
        Err(Error::Encoding("protected projection digest differs"))
    ));
    let schema = bytes.len() - 32 - b"[4,[]]\n".len() - 8 - 4;
    let mut changed = bytes;
    changed[schema] ^= 1;
    assert!(matches!(
        decode(&changed),
        Err(Error::Encoding("unknown protected projection schema"))
    ));
}

#[test]
fn aggregate_decode_ownership_exact_floor_and_prior_usage_are_preserved() {
    let bytes = encode(&data());
    let limits = cargo::Limits::standard();
    let prior = Usage {
        items: 13,
        bytes: 71,
    };
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(prior, &limits);
    {
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        Stored::decode(&bytes, &mut budget).expect("measure actual account");
    }
    let retained = Usage {
        items: allocation.usage.items - prior.items,
        bytes: allocation.usage.bytes - prior.bytes,
    };
    for (extra_items, extra_bytes) in [(0, 0), (1, 0), (0, 1)] {
        let graph = node::GraphBudget::new();
        let mut allocation = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
        allocation
            .items(limits.items - retained.items + extra_items)
            .expect("prior items");
        allocation
            .bytes(limits.input_proof_bytes - retained.bytes + extra_bytes)
            .expect("prior bytes");
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        // The decode is owned in full; prior usage one over the exact
        // floor records the axis with the exact total as its high water.
        Stored::decode(&bytes, &mut budget).expect("decode is owned, not gated");
        assert_eq!(
            budget.usage(),
            Usage {
                items: limits.items + extra_items,
                bytes: limits.input_proof_bytes + extra_bytes,
            }
        );
        let record = budget.exceedances();
        let items = record.axis(input::INPUT_ITEMS_RESOURCE);
        let bytes_axis = record.axis(input::INPUT_BYTES_RESOURCE);
        assert_eq!(items.is_some(), extra_items == 1);
        assert_eq!(bytes_axis.is_some(), extra_bytes == 1);
        if let Some(axis) = items {
            assert_eq!(axis.target as usize, limits.items);
            assert_eq!(axis.high_water as usize, limits.items + 1);
        }
        if let Some(axis) = bytes_axis {
            assert_eq!(axis.target as usize, limits.input_proof_bytes);
            assert_eq!(axis.high_water as usize, limits.input_proof_bytes + 1);
        }
        assert!(budget.check().is_ok(), "an exceedance never latches");
    }
}

#[test]
fn encode_exceedance_is_recorded_and_not_terminal() {
    let stored = data();
    let mut limits = cargo::Limits::standard();
    limits.input_proof_bytes = 1;
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    // The encoded length against `input_proof_bytes` is accounting; the
    // encoding completes and the crossing is recorded without latching.
    let encoded = stored
        .encode(&mut budget)
        .expect("the encoded-bytes target records and admits");
    assert!(budget.check().is_ok(), "an exceedance never latches");
    let axis = *allocation
        .exceedances()
        .axis("encoded stored baseline bytes")
        .expect("the encoded-bytes crossing is recorded");
    assert_eq!(axis.target, 1);
    assert_eq!(axis.high_water as usize, encoded.len());
}

#[test]
fn encode_physical_refusal_is_prospective_terminal_and_does_not_allocate() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    assert!(matches!(
        codec::encode(&mut budget, past_content_ceiling),
        Err(Error::Input(crate::Error::Limit { .. }))
    ));
    assert_eq!(budget.allocations(), 0);
    assert!(matches!(budget.bytes(b""), Err(Error::Refused)));
}

/// Measure one encoding past the physical node content ceiling without
/// allocating it: the measuring pass only counts.
fn past_content_ceiling(writer: &mut codec::Writer<'_>) -> super::Result<()> {
    let chunk = [0_u8; 1024 * 1024];
    for _ in 0..=node::MAX_CONTENT_BYTES / chunk.len() {
        writer.raw(&chunk)?;
    }
    Ok(())
}

#[test]
fn wire_decode_graph_exact_floor_and_prior_work_are_preserved() {
    let bytes = encode(&data());
    let limits = cargo::Limits::standard();
    for extra in [0, 1] {
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize - bytes.len() + extra)
            .expect("prior decode work");
        let mut allocation = Allocation::for_proof(
            Usage {
                items: 7,
                bytes: 31,
            },
            &limits,
        );
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        // The wire decode is charged to the graph in full; one byte of
        // prior work over the exact floor records the decoded axis.
        Stored::decode(&bytes, &mut budget).expect("wire decode is measured, not gated");
        let record = graph.exceedances();
        assert_eq!(record.is_exceeded(), extra == 1, "prior work {extra}");
        if extra == 1 {
            let axis = record
                .axis(node::DECODED_RESOURCE)
                .expect("decoded axis recorded");
            assert_eq!(axis.target, node::MAX_DECODED_BYTES_PER_GRAPH);
            assert_eq!(axis.high_water, node::MAX_DECODED_BYTES_PER_GRAPH + 1);
            assert!(axis.site.is_some(), "decode charges track their caller");
            assert!(budget.check().is_ok(), "an exceedance never latches");
            // A genuine failure latches; the latched account owns nothing more.
            budget.refuse();
            let allocations = budget.allocations();
            assert!(matches!(budget.bytes(b"later"), Err(Error::Refused)));
            assert_eq!(budget.allocations(), allocations);
        }
    }
}

#[test]
fn native_paths_and_derived_proof_text_do_not_gain_a_new_scalar_cap() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let (stored, mut allocation) = Stored::capture(empty(), &limits, &graph).into_parts();
    let mut stored = stored.expect("empty data");
    stored.recipes.push(Recipe::File {
        role: proof::Role::Authority,
        package_id: None,
        path: path(b"/native/path/longer/than/scalar"),
    });
    stored.entries = vec![
        proof::Entry::Fact {
            role: proof::Role::DepInfo,
            package_id: None,
            name: "derived path name".into(),
            value: "0123456789abcdef".into(),
        },
        proof::Entry::BuildMessage {
            package_id: "p".into(),
            execution: "/long/execution".into(),
            name: "s".into(),
            source: Some("/long/source".into()),
            value: "0123456789abcdef".into(),
        },
    ]
    .into_boxed_slice();
    stored.entries.sort_unstable();
    stored.fingerprint = entries::fingerprint(
        proof::SCHEMA,
        &stored.entries,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("canonical fixture");
    let bytes = encode(&stored);
    let mut limits = limits;
    limits.string_bytes = 1;
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
    Stored::decode(
        &bytes,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("path and derived fields use aggregate bytes");
}

#[test]
fn undefined_path_and_git_wire_tags_are_not_missing_witness_success_values() {
    for tag in [1, 3] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = Allocation::new(&limits);
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let bytes = codec::encode(&mut budget, |writer| {
            writer.header(proof::SCHEMA, Usage { items: 0, bytes: 0 })?;
            writer.entries(&[])?;
            writer.raw(&[0; 32])?;
            writer.count(1)?;
            writer.byte(5)?;
            writer.byte(tag)?;
            writer.byte(0)
        })
        .expect("malformed fixture encodes");
        assert!(matches!(decode(&bytes), Err(Error::IncompleteWitness)));
    }
}

#[test]
fn historical_projection_bytes_do_not_claim_a_protected_parser_or_current_universe() {
    let mut stored = data();
    stored.projection = Some(Projection::capture(
        universe::BuildProjection::from_canonical(
            b"historical bytes without a protected entry grammar"
                .as_slice()
                .into(),
        ),
    ));
    let decoded =
        decode(&encode(&stored)).expect("data preserves bytes without minting authentication");
    assert_eq!(
        decoded.projection().expect("bytes").bytes(),
        b"historical bytes without a protected entry grammar"
    );
}

#[test]
fn known_duplicate_tree_entries_and_unknown_tags_reject() {
    let mut stored = data();
    let directory = stored
        .entries
        .iter_mut()
        .find_map(|entry| match entry {
            proof::Entry::Directory { directories, .. } => Some(directories),
            _ => None,
        })
        .expect("directory entry");
    *directory = vec![
        proof::TreeDirectory {
            path: "same".into()
        };
        2
    ]
    .into_boxed_slice();
    assert!(matches!(
        decode(&encode(&stored)),
        Err(Error::Encoding("tree directories are not strictly ordered"))
    ));

    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::new(&limits);
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let bytes = codec::encode(&mut budget, |writer| {
        writer.header(proof::SCHEMA, Usage { items: 0, bytes: 0 })?;
        writer.count(1)?;
        writer.byte(255)?;
        writer.raw(&[0; 64])
    })
    .expect("unknown tag fixture");
    assert!(matches!(
        decode(&bytes),
        Err(Error::Encoding("unknown expected entry tag"))
    ));
}

#[test]
fn actual_buffer_and_layout_exceedances_record_and_genuine_failures_latch() {
    let limits = cargo::Limits::standard();
    for vector in [false, true] {
        let graph = node::GraphBudget::new();
        let requested = if vector {
            2 * std::mem::size_of::<u64>()
        } else {
            4
        };
        let mut allocation = Allocation::with_limits(100, requested + 7 - 1, limits.string_bytes);
        allocation.items(3).expect("prior items");
        allocation.bytes(7).expect("prior bytes");
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        // One byte over the byte target is charged, recorded and then
        // allocated; the account is not latched.
        let admitted = if vector {
            budget.items::<u64>(2).map(|_| ())
        } else {
            budget.bytes(b"four").map(|_| ())
        };
        admitted.expect("one over the byte target is still owned");
        assert_eq!(budget.allocations(), 1);
        assert_eq!(budget.usage().bytes, 7 + requested);
        assert!(
            budget.usage().items > 3,
            "successful prior charges are not refunded"
        );
        let record = budget.exceedances();
        let axis = record
            .axis(input::INPUT_BYTES_RESOURCE)
            .expect("byte axis recorded");
        assert_eq!(axis.target as usize, requested + 7 - 1);
        assert_eq!(axis.first_requested as usize, requested + 7);
        assert_eq!(axis.high_water as usize, requested + 7);
        assert!(record.axis(input::INPUT_ITEMS_RESOURCE).is_none());
        assert!(budget.check().is_ok(), "an exceedance never latches");
        // A genuine allocation failure latches before any buffer exists.
        assert!(matches!(
            budget.buffer(usize::MAX / 2 + 1),
            Err(Error::Allocation(_))
        ));
        assert_eq!(budget.allocations(), 1);
        assert!(matches!(budget.bytes(b""), Err(Error::Refused)));
    }
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::new(&limits);
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    assert!(matches!(
        budget.items::<u64>(usize::MAX),
        Err(Error::Overflow)
    ));
    assert_eq!(budget.allocations(), 0);
    assert!(matches!(budget.bytes(b""), Err(Error::Refused)));
}

/// Cause ordering: an exceedance never latches on its own, a genuine
/// failure always does, and the order of the two never changes the outcome.
#[test]
fn exceedance_and_genuine_failure_latch_in_either_order() {
    let limits = cargo::Limits::standard();
    // Exceedance, then genuine failure: the account works until the failure.
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::with_limits(2, 4, limits.string_bytes);
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    budget
        .bytes(b"12345")
        .expect("one over the byte target still succeeds");
    let record = budget.exceedances();
    let axis = record
        .axis(input::INPUT_BYTES_RESOURCE)
        .expect("byte axis recorded");
    assert_eq!((axis.target, axis.high_water), (4, 5));
    assert!(budget.check().is_ok());
    assert_eq!(budget.allocations(), 1);
    assert!(matches!(
        budget.buffer(usize::MAX / 2 + 1),
        Err(Error::Allocation(_))
    ));
    assert!(matches!(budget.check(), Err(Error::Refused)));
    assert!(matches!(budget.bytes(b""), Err(Error::Refused)));
    assert_eq!(budget.allocations(), 1);
    assert!(
        budget
            .exceedances()
            .axis(input::INPUT_BYTES_RESOURCE)
            .is_some(),
        "the latch keeps the recorded exceedance"
    );
    // Genuine failure, then an exceeding charge: the latch does no further work.
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::with_limits(2, 4, limits.string_bytes);
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    assert!(matches!(
        budget.items::<u64>(usize::MAX),
        Err(Error::Overflow)
    ));
    assert!(matches!(budget.bytes(b"12345"), Err(Error::Refused)));
    assert!(matches!(
        budget.limited("stored baseline bytes", 4, 5),
        Err(Error::Refused)
    ));
    assert!(matches!(budget.check(), Err(Error::Refused)));
    assert!(
        !budget.exceedances().is_exceeded(),
        "a latched account records no exceedance"
    );
    assert_eq!(budget.usage(), Usage { items: 0, bytes: 0 });
    assert_eq!(budget.allocations(), 0);
}

#[test]
fn encoder_never_spends_unadmitted_capacity_when_the_count_pass_changes() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::new(&limits);
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let pass = std::cell::Cell::new(0);
    let result = codec::encode(&mut budget, |writer| {
        let current = pass.get();
        pass.set(current + 1);
        if current == 0 {
            writer.raw(b"a")
        } else {
            writer.raw(b"ab")
        }
    });
    assert!(matches!(
        result,
        Err(Error::Input(crate::Error::Limit {
            limit: 1,
            actual: 2,
            ..
        }))
    ));
    assert_eq!(budget.allocations(), 1);
    assert_eq!(budget.usage(), Usage { items: 1, bytes: 1 });
    assert!(matches!(budget.bytes(b""), Err(Error::Refused)));
}

#[test]
fn explicit_encode_layout_has_exact_and_one_byte_floor_accounting() {
    let stored = data();
    let expected = encode(&stored);
    let limits = cargo::Limits::standard();
    for extra in [0, 1] {
        let graph = node::GraphBudget::new();
        let mut allocation =
            Allocation::with_limits(8, 31 + expected.len() - extra, limits.string_bytes);
        allocation.items(7).expect("prior items");
        allocation.bytes(31).expect("prior bytes");
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let result = stored.encode(&mut budget);
        // The exact layout is charged either way; a ceiling one byte
        // under records the byte axis with the exact total as high water.
        assert_eq!(result.expect("layout is owned, not gated"), expected);
        assert_eq!(
            budget.usage(),
            Usage {
                items: 8,
                bytes: 31 + expected.len()
            }
        );
        assert_eq!(budget.allocations(), 1);
        let record = budget.exceedances();
        if extra == 0 {
            assert!(!record.is_exceeded(), "exact layout");
        } else {
            let axis = record
                .axis(input::INPUT_BYTES_RESOURCE)
                .expect("byte axis recorded");
            assert_eq!(axis.target as usize, 31 + expected.len() - 1);
            assert_eq!(axis.first_requested as usize, 31 + expected.len());
            assert_eq!(axis.high_water as usize, 31 + expected.len());
            assert!(budget.check().is_ok(), "an exceedance never latches");
            // A genuine failure latches; a latched account encodes nothing.
            budget.refuse();
            assert!(matches!(stored.encode(&mut budget), Err(Error::Refused)));
            assert_eq!(budget.allocations(), 1);
            assert!(matches!(budget.bytes(b""), Err(Error::Refused)));
        }
    }
}

#[test]
fn scalar_exceedance_precedes_its_buffer_and_does_not_reset_after_decode() {
    let bytes = encode(&data());
    let mut limits = cargo::Limits::standard();
    limits.string_bytes = 1;
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    // The scalar string target is measured before each scalar buffer is
    // copied; crossing it records the axis and the decode still completes.
    let decoded = Stored::decode(&bytes, &mut budget).expect("scalars are measured, not gated");
    assert_eq!(decoded.entries().len(), 26);
    let record = budget.exceedances();
    let axis = record
        .axis("stored baseline string bytes")
        .expect("scalar string axis recorded");
    assert_eq!(axis.target, 1);
    assert!(
        axis.first_requested > 1,
        "the first PackageId exceeds the target"
    );
    assert!(axis.high_water >= axis.first_requested);
    // The entry vector and preceding workspace watch vector/path are admitted
    // before the first scalar; the scalar buffers follow their measurement.
    assert!(budget.allocations() > 3);
    assert!(budget.check().is_ok(), "an exceedance never latches");
    // A genuine failure latches and the latch does not reset after a decode.
    budget.refuse();
    let allocations = budget.allocations();
    assert!(matches!(
        Stored::decode(&bytes, &mut budget),
        Err(Error::Refused)
    ));
    assert!(matches!(budget.bytes(b"a"), Err(Error::Refused)));
    assert_eq!(budget.allocations(), allocations);
}

#[test]
fn every_stored_resource_refusal_survives_a_new_borrow_and_graph() {
    let limits = cargo::Limits::standard();
    for failure in 0..5 {
        let mut allocation = Allocation::for_proof(
            Usage {
                items: 7,
                bytes: 31,
            },
            &limits,
        );
        let graph = node::GraphBudget::new();
        let mut operation_limits = limits;
        if failure == 3 {
            // An accounting target crossed on the way is only recorded.
            operation_limits.input_proof_bytes = 1;
        }
        let mut budget = Budget::borrow(&operation_limits, &mut allocation, &graph);
        // Every latch below is a genuine failure (checked-arithmetic
        // overflow, allocation failure or the physical encoder limit); the
        // accounting targets crossed on the way are recorded, never refused.
        let result = match failure {
            0 => {
                graph
                    .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
                    .expect("prior decode");
                budget
                    .wire(1)
                    .expect("one over the decoded target is recorded");
                assert!(graph.exceedances().axis(node::DECODED_RESOURCE).is_some());
                budget.wire(usize::MAX)
            }
            1 => {
                graph
                    .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
                    .expect("prior hash");
                budget
                    .hash(1)
                    .expect("one over the hashed target is recorded");
                assert!(graph.exceedances().axis(node::HASHED_RESOURCE).is_some());
                budget.hash(usize::MAX)
            }
            2 => budget.items::<u64>(usize::MAX).map(|_| ()),
            3 => codec::encode(&mut budget, past_content_ceiling).map(|_| ()),
            _ => budget.buffer(usize::MAX / 2 + 1).map(|_| ()),
        };
        assert!(result.is_err(), "resource failure {failure}");
        match failure {
            0 | 1 => assert!(
                matches!(result, Err(Error::Graph(node::Error::Allocation))),
                "graph overflow {failure}"
            ),
            2 => assert!(matches!(result, Err(Error::Overflow))),
            3 => assert!(matches!(
                result,
                Err(Error::Input(crate::Error::Limit { .. }))
            )),
            _ => assert!(matches!(result, Err(Error::Allocation(_)))),
        }
        let spent = allocation.usage();
        assert!(matches!(
            allocation.items(0),
            Err(crate::Error::InputAccountRefused)
        ));
        assert!(matches!(
            allocation.bytes(0),
            Err(crate::Error::InputAccountRefused)
        ));
        assert!(matches!(
            allocation.string(""),
            Err(crate::Error::InputAccountRefused)
        ));
        let current_graph = node::GraphBudget::new();
        let mut next = Budget::borrow(&limits, &mut allocation, &current_graph);
        assert!(matches!(next.bytes(b""), Err(Error::Refused)));
        assert!(matches!(next.wire(1), Err(Error::Refused)));
        assert!(matches!(next.hash(1), Err(Error::Refused)));
        assert_eq!(next.allocations(), 0);
        assert_eq!(next.usage(), spent);
        current_graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("refusal did not charge the newly supplied graph");
        current_graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .expect("refusal did not hash through the newly supplied graph");
    }
}

#[test]
fn caller_limits_cannot_widen_original_wire_or_string_admission() {
    let limits = cargo::Limits::standard();
    for scalar in [false, true] {
        let mut original = limits;
        original.input_proof_bytes = 8;
        original.string_bytes = 1;
        let mut allocation = Allocation::for_proof(Usage { items: 1, bytes: 1 }, &original);
        let graph = node::GraphBudget::new();
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let result = if scalar {
            budget.text(b"xx", true).map(|_| ())
        } else {
            budget.wire(9)
        };
        // The charge is measured against the ORIGINAL account's narrower
        // target, not the caller's wider limits; crossing it is recorded.
        result.expect("the original target is measured, not gated");
        let record = budget.exceedances();
        let (name, target, actual) = if scalar {
            ("stored baseline string bytes", 1, 2)
        } else {
            ("stored baseline bytes", 8, 9)
        };
        let axis = record.axis(name).expect("original ceiling axis recorded");
        assert_eq!(axis.target, target);
        assert_eq!(axis.high_water, actual);
        assert!(record.axis("input proof bytes").is_none());
        if scalar {
            assert_eq!(budget.allocations(), 1);
            assert_eq!(budget.usage(), Usage { items: 2, bytes: 3 });
        } else {
            assert_eq!(budget.allocations(), 0);
            assert_eq!(budget.usage(), Usage { items: 1, bytes: 1 });
            assert_eq!(
                graph.census_usage().expect("graph census").0,
                9,
                "the wire is charged to the graph after the original measurement"
            );
        }
        assert!(budget.check().is_ok(), "an exceedance never latches");
        // A genuine failure latches and no further graph work is charged.
        budget.refuse();
        assert!(matches!(budget.bytes(b""), Err(Error::Refused)));
        assert!(matches!(budget.wire(1), Err(Error::Refused)));
        let decoded = graph.census_usage().expect("graph census").0;
        assert_eq!(decoded, if scalar { 0 } else { 9 });
    }
}

#[test]
fn replay_account_join_preserves_exact_spending_and_terminal_refusal() {
    let mut prior = Allocation::with_limits(10, 20, 8);
    prior.items(4).unwrap();
    prior.bytes(9).unwrap();
    let mut cold = Allocation::with_limits(10, 20, 8);
    cold.items(6).unwrap();
    cold.bytes(11).unwrap();
    prior.absorb(cold).unwrap();
    assert_eq!((prior.usage.items, prior.usage.bytes), (10, 20));
    assert!(!prior.exceedances().is_exceeded(), "exact join");
    // Joining one byte past the target records the axis on the joined
    // account with the exact joined total; the account stays open.
    let mut extra = Allocation::with_limits(10, 20, 8);
    extra.bytes(1).unwrap();
    prior
        .absorb(extra)
        .expect("joined spending is recorded, not gated");
    assert_eq!((prior.usage.items, prior.usage.bytes), (10, 21));
    let record = prior.exceedances();
    let axis = record
        .axis(input::INPUT_BYTES_RESOURCE)
        .expect("joined byte axis");
    assert_eq!((axis.target, axis.high_water), (20, 21));
    assert!(prior.check().is_ok());

    // An exceeding side's record is folded into the join without resetting.
    let mut prior = Allocation::with_limits(10, 20, 8);
    let mut exceeded = Allocation::with_limits(10, 20, 8);
    exceeded
        .items(11)
        .expect("one over the item target is recorded");
    prior
        .absorb(exceeded)
        .expect("an exceeded side still joins");
    assert_eq!(prior.usage.items, 11);
    let record = prior.exceedances();
    let axis = record
        .axis(input::INPUT_ITEMS_RESOURCE)
        .expect("folded item axis");
    assert_eq!((axis.target, axis.high_water), (10, 11));
    assert!(prior.check().is_ok());
    // A genuinely refused side is terminal for the join.
    let mut prior = Allocation::with_limits(10, 20, 8);
    let mut refused = Allocation::with_limits(10, 20, 8);
    refused.refuse();
    assert!(matches!(
        prior.absorb(refused),
        Err(crate::Error::InputAccountRefused)
    ));
    assert!(prior.check().is_err());
    // A ceiling mismatch still refuses the join.
    let mut prior = Allocation::with_limits(10, 20, 8);
    assert!(prior.absorb(Allocation::with_limits(10, 21, 8)).is_err());
    assert!(prior.check().is_err());

    let mut prior = Allocation::with_limits(usize::MAX, usize::MAX, 8);
    prior.usage.items = usize::MAX;
    let mut extra = Allocation::with_limits(usize::MAX, usize::MAX, 8);
    extra.items(1).unwrap();
    assert!(prior.absorb(extra).is_err());
    assert!(prior.check().is_err());
}

#[test]
fn nested_wire_sections_are_framed_by_the_record_charge_and_standalone_decodes_charge_again() {
    let record = encode(&data());
    let outside = [7_u8; 17];
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let decoded = || graph.census_usage().expect("graph census").0;
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    // The enclosing record's wire is charged once by its length.
    budget.wire_record(&record).expect("record wire");
    assert_eq!(decoded(), record.len() as u64);
    // Sections framed by that record add no decode.
    budget
        .wire_section(&record[3..record.len() - 2])
        .expect("nested section");
    budget
        .wire_section(&record[..])
        .expect("whole framed extent");
    budget
        .wire_section(&record[record.len()..])
        .expect("empty tail");
    assert_eq!(decoded(), record.len() as u64);
    // A standalone section outside the frame charges in full.
    budget.wire_section(&outside).expect("standalone section");
    assert_eq!(decoded(), (record.len() + outside.len()) as u64);
    // Repeating the record decode is a distinct decode and charges again.
    budget.wire_record(&record).expect("second record wire");
    assert_eq!(decoded(), (2 * record.len() + outside.len()) as u64);
    assert_eq!(budget.usage().bytes, 31, "wire framing owns no input bytes");
}
