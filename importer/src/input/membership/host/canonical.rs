use super::{observed, stored, Budget, Error, Host};
use crate::{cargo, input};

fn image(native_error: bool) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
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
        let mut allocation = input::Allocation::new(&limits);
        let mut host = Host::from_image(
            &image,
            Budget {
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
        let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, 4);
        allocation.items(7).expect("prior items");
        allocation.bytes(19).expect("prior bytes");
        let mut host = Host::from_image(
            &image,
            Budget {
                allocation: &mut allocation,
            },
        )
        .expect("host");
        host.canonicalize(b"/p/\xff")
            .expect("first native outcome")
            .ok();
        // Every outcome owns its query and spends its lookup.
        let result = host.canonicalize(b"/p/\xff");
        assert!(result.is_ok());
        assert_eq!(host.budget().allocation.usage().items, 10);
        assert_eq!(host.budget().allocation.usage().bytes, 28);
        host.finish().expect("first query consumed");
    }
}
