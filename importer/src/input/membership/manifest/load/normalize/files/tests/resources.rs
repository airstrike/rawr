use super::*;

const SINGLE: &str = "one/two/../three//../custom/build.rs";

fn fixture() -> stored::Stored {
    image(vec![
        (b"/package/build.rs", observed::Value::Absent),
        (b"/package/README.md", observed::Value::Error(13)),
        (b"/package/README.txt", observed::Value::Absent),
        (b"/package/README", metadata(observed::Kind::File)),
    ])
}

fn exercise(session: &mut Session<'_, host::Host<'_, '_>>) -> Step<()> {
    let root = Path::new("/package");
    assert!(matches!(build(None, root, session)?, Build::Auto(false)));
    assert!(matches!(
        build(Some(&primitive::Build::Auto(true)), root, session)?,
        Build::Single(_)
    ));
    let single = primitive::Build::Single(value::Text::Borrowed(SINGLE));
    let Build::Single(text) = build(Some(&single), root, session)? else {
        panic!("single");
    };
    assert_eq!(text.as_str(), "one/custom/build.rs");
    assert_eq!(readme(root, None, session)?.unwrap().as_str(), "README");
    let explicit = primitive::StringOrBool::String(value::Text::Borrowed("unchanged/../readme"));
    assert_eq!(
        readme(root, Some(&explicit), session)?.unwrap().as_str(),
        "unchanged/../readme"
    );
    Ok(())
}

fn run(
    image: &stored::Stored,
    items: usize,
    bytes: usize,
    string: usize,
    paths: usize,
) -> (
    Result<(), Error<crate::input::membership::Error>>,
    input::Usage,
    crate::exceedance::Record,
) {
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH - paths)
        .unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .unwrap();
    let mut allocation = input::Allocation::with_limits(items, bytes, string);
    allocation.items(7).unwrap();
    allocation.bytes(31).unwrap();
    let mut host = match host::Host::from_image(
        image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    ) {
        Ok(host) => host,
        Err(error) => {
            return (
                Err(Error::Host(error)),
                allocation.usage(),
                allocation.exceedances(),
            )
        }
    };
    let mut session = Session::new(&mut host);
    let result = exercise(&mut session);
    if result.is_err() {
        let paths = storage::ALLOCATIONS.with(|count| count.get());
        let queries = crate::input::membership::OWNED_BYTES.with(|count| count.get());
        assert!(matches!(
            readme(Path::new("/after-failure"), None, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            build(
                Some(&primitive::Build::Auto(true)),
                Path::new("/"),
                &mut session
            ),
            Err(Stop::Refused)
        ));
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), paths);
        assert_eq!(
            crate::input::membership::OWNED_BYTES.with(|count| count.get()),
            queries
        );
    }
    let result = session.finish(result);
    if result.is_ok() {
        host.finish().unwrap();
    } else {
        drop(host);
    }
    if paths == 3 {
        // The fourth path check past the target is recorded, not refused.
        let exceeded = graph.exceedances();
        let axis = exceeded
            .axis(node::PATHS_RESOURCE)
            .expect("exhausted path target recorded");
        assert_eq!(
            axis.first_requested,
            node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
        );
        assert_eq!(axis.high_water, node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1);
    } else {
        assert!(!graph.exceedances().is_exceeded());
    }
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (result, allocation.usage(), allocation.exceedances())
}

#[test]
fn helpers_use_exact_original_remaining_items_bytes_strings_and_graph_work() {
    use crate::input::membership::manifest::tests::one_short;
    let image = fixture();
    let (result, usage, _) = run(&image, usize::MAX, usize::MAX, usize::MAX, 4);
    result.unwrap();
    let (result, exact, exceeded) = run(&image, usage.items, usage.bytes, SINGLE.len(), 4);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(!exceeded.is_exceeded());
    // One short of an aggregate target admits and records that axis.
    let (result, exact, exceeded) = run(&image, usage.items - 1, usage.bytes, SINGLE.len(), 4);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_ITEMS_RESOURCE,
        usage.items
    ));
    let (result, exact, exceeded) = run(&image, usage.items, usage.bytes - 1, SINGLE.len(), 4);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes
    ));
    // One short of the longest string records the decoded-string axis
    // while the same helpers succeed with the same work.
    let (result, exact, exceeded) = run(&image, usage.items, usage.bytes, SINGLE.len() - 1, 4);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(
        one_short(
            &exceeded,
            "membership decoded TOML string bytes",
            SINGLE.len()
        ),
        "{exceeded:?}"
    );
    let axis = exceeded
        .axis("membership decoded TOML string bytes")
        .unwrap();
    assert_eq!(
        (axis.target, axis.first_requested),
        (SINGLE.len() as u64 - 1, SINGLE.len() as u64)
    );
    let (result, exact, exceeded) = run(&image, usage.items, usage.bytes, SINGLE.len(), 3);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(!exceeded.is_exceeded());
}

#[test]
fn one_short_native_path_layout_is_recorded_and_a_refused_account_prevents_allocation() {
    use crate::input::membership::manifest::tests::one_short;
    // One byte short of the native path layout admits the whole probe.
    // The layout is the first charge past the target, before any allocation
    // or observation, and the probe's total is the bytes high water.
    let image = fixture();
    let layout = 31 + image.records().len() + b"/package/build.rs".len();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, layout - 1, usize::MAX);
    allocation.items(7).unwrap();
    allocation.bytes(31).unwrap();
    let mut host = host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let mut session = Session::new(&mut host);
    assert!(matches!(
        build(None, Path::new("/package"), &mut session),
        Ok(Build::Auto(false))
    ));
    session.finish(Ok(())).unwrap();
    let spent = host.budget().allocation.usage().bytes;
    assert!(spent > layout);
    let exceeded = host.budget().allocation.exceedances();
    assert!(one_short(&exceeded, input::INPUT_BYTES_RESOURCE, spent));
    let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
    assert_eq!(axis.first_requested, layout as u64);
    assert!(host.budget().allocation.check().is_ok());
    drop(host);

    // Only a genuinely refused original account stops the probe before any
    // allocation or observation.
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    allocation.items(7).unwrap();
    allocation.bytes(31).unwrap();
    let mut host = host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    host.budget().allocation.refuse();
    let mut session = Session::new(&mut host);
    storage::ALLOCATIONS.with(|count| count.set(0));
    crate::input::membership::OWNED_BYTES.with(|count| count.set(0));
    assert!(matches!(
        build(None, Path::new("/package"), &mut session),
        Err(Stop::Refused)
    ));
    assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
    assert_eq!(
        crate::input::membership::OWNED_BYTES.with(|count| count.get()),
        0
    );
    assert!(matches!(
        readme(Path::new("/package"), None, &mut session),
        Err(Stop::Refused)
    ));
    assert!(session.finish(Ok(())).is_err());
    assert!(matches!(
        host.finish(),
        Err(crate::input::membership::Error::Unconsumed)
    ));
}

#[test]
fn missing_probe_is_terminal_instead_of_skipping_to_a_later_readme() {
    let image = image(vec![(
        b"/package/README.txt",
        metadata(observed::Kind::File),
    )]);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut host = host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let mut session = Session::new(&mut host);
    assert!(matches!(
        readme(Path::new("/package"), None, &mut session),
        Err(Stop::Refused)
    ));
    let queries = crate::input::membership::OWNED_BYTES.with(|count| count.get());
    assert!(matches!(
        readme(Path::new("/package"), None, &mut session),
        Err(Stop::Refused)
    ));
    assert_eq!(
        crate::input::membership::OWNED_BYTES.with(|count| count.get()),
        queries
    );
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Host(crate::input::membership::Error::Unobserved))
    ));
    assert!(matches!(
        host.finish(),
        Err(crate::input::membership::Error::Unconsumed)
    ));
}
