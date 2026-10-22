use super::{observed, stored, Budget, Error, Host};
use crate::{cargo, input, node};

fn image(native_error: bool) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let record = observed::Record {
        query: observed::Query::Canonical(
            observed::Path::from_bytes(b"/p/\xff", &mut budget).expect("input"),
        ),
        value: observed::Value::Canonical(if native_error {
            Err(107)
        } else {
            Ok(observed::Path::from_bytes(b"/out/\xfe", &mut budget).expect("output"))
        }),
    };
    stored::Stored::from_records(&[record], &mut budget).expect("image")
}

#[test]
fn canonical_host_borrows_output_and_keeps_exact_query_and_native_error() {
    for native_error in [false, true] {
        let image = image(native_error);
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut host = Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .expect("host");
        for absent in [b"/p/./\xff".as_slice(), b"/p/\xfe"] {
            assert!(matches!(host.canonicalize(absent), Err(Error::Unobserved)));
        }
        let before = host.budget().allocation.usage();
        let actual = host.canonicalize(b"/p/\xff").expect("recorded outcome");
        if native_error {
            assert_eq!(actual, Err(107));
        } else {
            let observed::Value::Canonical(Ok(path)) = &image.records()[0].value else {
                panic!("canonical output")
            };
            assert!(std::ptr::eq(actual.expect("success"), path.bytes()));
        }
        assert_eq!(host.budget().allocation.usage().items - before.items, 1);
        assert_eq!(host.budget().allocation.usage().bytes - before.bytes, 4);
        host.finish().expect("consumed");
    }
}

#[test]
fn canonical_queries_use_original_ownership_and_graph_on_every_outcome() {
    for native_error in [false, true] {
        let image = image(native_error);
        for floor in 0..4 {
            let graph = node::GraphBudget::new();
            graph
                .paths(node::MAX_PATH_CHECKS_PER_GRAPH - 2 + usize::from(floor == 3))
                .expect("prior work");
            let mut allocation = input::Allocation::with_limits(
                10 - usize::from(floor == 1),
                19 + 1 + 8 - usize::from(floor == 2),
                4,
            );
            allocation.items(7).expect("prior items");
            allocation.bytes(19).expect("prior bytes");
            let mut host = Host::from_image(
                &image,
                Budget {
                    graph: &graph,
                    allocation: &mut allocation,
                },
            )
            .expect("host");
            host.canonicalize(b"/p/\xff")
                .expect("first native outcome")
                .ok();
            // Every outcome owns its query and spends its lookup; a
            // crossed target is recorded on the crossing account, not refused.
            let result = host.canonicalize(b"/p/\xff");
            assert!(result.is_ok());
            assert_eq!(host.budget().allocation.usage().items, 10);
            assert_eq!(host.budget().allocation.usage().bytes, 28);
            let (_, paths, _) = graph.census_usage().expect("census");
            assert_eq!(
                paths,
                node::MAX_PATH_CHECKS_PER_GRAPH + usize::from(floor == 3)
            );
            let owned = host.budget().allocation.exceedances();
            let graph_exceeded = graph.exceedances();
            match floor {
                0 => {
                    assert!(!owned.is_exceeded());
                    assert!(!graph_exceeded.is_exceeded());
                    graph.paths(1).expect("past the target is recorded");
                    assert_eq!(
                        graph
                            .exceedances()
                            .axis(node::PATHS_RESOURCE)
                            .unwrap()
                            .high_water,
                        node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
                    );
                }
                1 => {
                    let axis = owned.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
                    assert_eq!((axis.target, axis.high_water), (9, 10));
                    assert!(owned.axis(input::INPUT_BYTES_RESOURCE).is_none());
                    assert!(!graph_exceeded.is_exceeded());
                }
                2 => {
                    let axis = owned.axis(input::INPUT_BYTES_RESOURCE).unwrap();
                    assert_eq!((axis.target, axis.high_water), (27, 28));
                    assert!(owned.axis(input::INPUT_ITEMS_RESOURCE).is_none());
                    assert!(!graph_exceeded.is_exceeded());
                }
                3 => {
                    assert!(!owned.is_exceeded());
                    let axis = graph_exceeded.axis(node::PATHS_RESOURCE).unwrap();
                    assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
                    assert_eq!(
                        axis.first_requested,
                        node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
                    );
                    assert_eq!(axis.high_water, node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1);
                }
                _ => unreachable!(),
            }
            assert!(host.budget().allocation.items(0).is_ok(), "nothing latched");
            host.finish().expect("first query consumed");
        }
    }
}
