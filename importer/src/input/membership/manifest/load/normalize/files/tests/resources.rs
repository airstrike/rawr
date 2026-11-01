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

fn run(image: &stored::Stored) -> Result<(), Error<crate::input::membership::Error>> {
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    allocation.items(7).unwrap();
    allocation.bytes(31).unwrap();
    let mut host = match host::Host::from_image(
        image,
        Budget {
            allocation: &mut allocation,
        },
    ) {
        Ok(host) => host,
        Err(error) => return Err(Error::Host(error)),
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
    assert!(allocation.check().is_ok());
    result
}

#[test]
fn helpers_complete_on_the_original_account() {
    run(&fixture()).unwrap();
}

#[test]
fn a_refused_account_prevents_allocation() {
    let image = fixture();
    // Only a genuinely refused original account stops the probe before any
    // allocation or observation.
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    allocation.items(7).unwrap();
    allocation.bytes(31).unwrap();
    let mut host = host::Host::from_image(
        &image,
        Budget {
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
    let mut allocation = input::Allocation::new(&limits);
    let mut host = host::Host::from_image(
        &image,
        Budget {
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
