#[test]
fn handwritten_preview_owner_remains_operational_during_profile_isolation() {
    let checked = rawr_iced::preview::Compiler::new()
        .unwrap()
        .check_named_source("generated-preview.rawr", SOURCE)
        .unwrap();
    let prepared = checked.prepare().unwrap();
    assert_eq!(prepared.source().identity(), "generated-preview.rawr");

    let limits = rawr_import::cargo::Limits::standard();
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    let graph = rawr_import::test_fixture::iced_profile();
    let contract = rawr_import::profile::classify(&definition, &graph, &limits).unwrap();
    assert_eq!(contract.evidence().len(), 19);
    for (kind, expected) in [
        (rawr_import::profile::contract::Kind::Definition, 11),
        (rawr_import::profile::contract::Kind::Export, 7),
        (rawr_import::profile::contract::Kind::Implementation, 1),
    ] {
        assert_eq!(
            contract
                .evidence()
                .filter(|evidence| evidence.kind() == kind)
                .count(),
            expected,
        );
    }

    let alias = contract.alias_targets().next().expect("one Element alias");
    assert_eq!(contract.alias_targets().len(), 1);
    assert_eq!((alias.root(), alias.path()), ("iced", "Element"));

    let source = contract
        .projection_sources()
        .next()
        .expect("one exact Text source site");
    assert_eq!(contract.projection_sources().len(), 1);
    assert_eq!((source.root(), source.path()), ("iced", "widget::Text"));
    assert_eq!(source.target_selector(), alias.selector());

    let mut roles = contract
        .extension_abis()
        .map(rawr_import::profile::contract::ExtensionAbi::role)
        .collect::<Vec<_>>();
    roles.sort_unstable();
    assert_eq!(roles, ["Color", "Length", "Rectangle", "Vector"]);
    assert_eq!(contract.extension_abis().len(), 4);
    assert!(contract
        .extension_abis()
        .all(|abi| abi.root() == "iced" && abi.path() == abi.role()));

    let projection = contract
        .projections()
        .next()
        .expect("one exact direct projection");
    assert_eq!(contract.projections().len(), 1);
    assert_eq!(projection.source_selector(), source.selector());
    assert_eq!(projection.alias_selector(), alias.selector());

    let endpoint = contract
        .operation_endpoints()
        .next()
        .expect("one exact text operation endpoint");
    assert_eq!(contract.operation_endpoints().len(), 1);
    assert_eq!(
        (endpoint.site().root(), endpoint.site().path()),
        ("iced", "widget::text")
    );
    assert_eq!(endpoint.source_selector(), source.selector());
    assert_eq!(endpoint.projection_selector(), projection.selector());

    let mut implementations = contract
        .evidence()
        .filter(|evidence| evidence.kind() == rawr_import::profile::contract::Kind::Implementation)
        .map(|evidence| *evidence.digest())
        .collect::<Vec<_>>();
    implementations.sort_unstable();
    assert_eq!(implementations, [*source.widget_evidence()]);
}

const SOURCE: &str = r#"
use iced::Element;
use iced::widget::text;

#[preview(name = "isolated")]
pub fn sample() -> Element<'_, ()> { text("Preview") }
"#;
