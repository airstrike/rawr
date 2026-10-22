use rawr_import::profile;
use sha2::{Digest as _, Sha256};

struct PriorVersion {
    definition: rawr_iced::profile::Definition,
    version: u32,
}

impl profile::Definition<rawr_iced::profile::Checked> for PriorVersion {
    fn identity(&self) -> profile::Identity {
        <rawr_iced::profile::Definition as profile::Definition<
            rawr_iced::profile::Checked,
        >>::identity(&self.definition)
    }

    fn version(&self) -> profile::Version {
        profile::Version::new(self.version).expect("prior profile version is nonzero")
    }

    fn classify(
        &self,
        graph: &rawr_import::api::Graph<rawr_import::api::Extracted>,
        output: &mut profile::contract::Builder<'_>,
    ) -> Result<(), profile::Error> {
        <rawr_iced::profile::Definition as profile::Definition<
            rawr_iced::profile::Checked,
        >>::classify(&self.definition, graph, output)
    }

    fn demand(
        &self,
        contract: &profile::Contract,
        checked: &rawr_iced::profile::Checked,
        output: &mut profile::demand::Builder<'_>,
    ) -> Result<(), profile::Error> {
        <rawr_iced::profile::Definition as profile::Definition<rawr_iced::profile::Checked>>::demand(
            &self.definition,
            contract,
            checked,
            output,
        )
    }

    fn generate(
        &self,
        demand: &profile::Demand,
        output: &mut profile::demand::Generator<'_>,
    ) -> Result<(), profile::Error> {
        <rawr_iced::profile::Definition as profile::Definition<
            rawr_iced::profile::Checked,
        >>::generate(&self.definition, demand, output)
    }
}

#[test]
fn pinned_profile_retains_only_exact_trait_and_widget_evidence() {
    let limits = rawr_import::cargo::Limits::standard();
    let graph = rawr_import::test_fixture::iced_profile();
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    let contract = profile::classify(&definition, &graph, &limits).unwrap();

    let kinds = contract
        .evidence()
        .map(profile::contract::Evidence::kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            profile::contract::Kind::Definition,
            profile::contract::Kind::Definition,
            profile::contract::Kind::Definition,
            profile::contract::Kind::Definition,
            profile::contract::Kind::Definition,
            profile::contract::Kind::Definition,
            profile::contract::Kind::Definition,
            profile::contract::Kind::Definition,
            profile::contract::Kind::Definition,
            profile::contract::Kind::Definition,
            profile::contract::Kind::Definition,
            profile::contract::Kind::Export,
            profile::contract::Kind::Export,
            profile::contract::Kind::Export,
            profile::contract::Kind::Export,
            profile::contract::Kind::Export,
            profile::contract::Kind::Export,
            profile::contract::Kind::Export,
            profile::contract::Kind::Implementation,
        ]
    );
    let selected_definition_paths = contract
        .evidence()
        .filter(|evidence| evidence.kind() == profile::contract::Kind::Definition)
        .filter_map(|evidence| graph.definitions().nth(evidence.index() as usize))
        .map(|definition| definition.display_path().collect::<Vec<_>>().join("::"))
        .collect::<Vec<_>>();
    assert!(selected_definition_paths
        .iter()
        .any(|path| path == "iced_widget::text::Text"));
    let selected_exports = contract
        .evidence()
        .filter(|evidence| evidence.kind() == profile::contract::Kind::Export)
        .filter_map(|evidence| graph.exports().nth(evidence.index() as usize))
        .map(|export| {
            (
                export.key().path().collect::<Vec<_>>().join("::"),
                export.key().namespace(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        selected_exports,
        [
            ("Color".into(), rawr_import::api::identity::Namespace::Type),
            (
                "Element".into(),
                rawr_import::api::identity::Namespace::Type
            ),
            ("Length".into(), rawr_import::api::identity::Namespace::Type),
            (
                "Rectangle".into(),
                rawr_import::api::identity::Namespace::Type
            ),
            ("Vector".into(), rawr_import::api::identity::Namespace::Type),
            (
                "widget::Text".into(),
                rawr_import::api::identity::Namespace::Type,
            ),
            (
                "widget::text".into(),
                rawr_import::api::identity::Namespace::Value,
            ),
        ]
    );
    let abis = contract.extension_abis().collect::<Vec<_>>();
    assert_eq!(abis.len(), 4);
    assert!(abis
        .windows(2)
        .all(|pair| pair[0].selector() < pair[1].selector()));
    for role in ["Color", "Length", "Rectangle", "Vector"] {
        assert_eq!(abis.iter().filter(|abi| abi.role() == role).count(), 1);
    }
    for abi in &abis {
        assert_eq!(abi.root(), "iced");
        assert_eq!(abi.path(), abi.role());
        assert_eq!(
            abi.namespace(),
            rawr_import::surface::facade::Namespace::Type
        );
        assert_ne!(abi.export_identity(), &[0; 32]);
        assert_ne!(abi.definition_identity(), &[0; 32]);
        assert_ne!(abi.shape_digest(), &[0; 32]);
        assert_ne!(abi.site_selector(), &[0; 32]);
        assert_ne!(abi.linked_fingerprint(), &[0; 32]);
        assert_ne!(abi.selector(), &[0; 32]);
    }
    let abi = |role| {
        abis.iter()
            .copied()
            .find(|abi| abi.role() == role)
            .expect("one exact extension ABI role")
    };
    assert!(abi("Color").arguments().is_empty());
    assert!(abi("Length").arguments().is_empty());
    assert_eq!(
        abi("Rectangle").arguments(),
        [profile::contract::AbiArgument::DefaultF32 { parameter: 0 }]
    );
    assert_eq!(
        abi("Vector").arguments(),
        [profile::contract::AbiArgument::DefaultF32 { parameter: 0 }]
    );
    let source_site = contract
        .projection_sources()
        .next()
        .expect("one exact Text source site");
    assert_eq!(contract.projection_sources().len(), 1);
    assert_eq!(source_site.root(), "iced");
    assert_eq!(source_site.path(), "widget::Text");
    assert_eq!(
        source_site
            .parameters()
            .iter()
            .map(|parameter| (
                parameter.kind(),
                parameter.alias_index(),
                parameter.target_index(),
            ))
            .collect::<Vec<_>>(),
        [
            (rawr_import::surface::facade::ParameterKind::Lifetime, 0, 0),
            (rawr_import::surface::facade::ParameterKind::Type, 1, 1),
        ]
    );
    assert_eq!(
        source_site.arguments(),
        [
            profile::contract::SourceArgument::Region {
                alias: 0,
                source: 0
            },
            profile::contract::SourceArgument::Parameter {
                kind: rawr_import::surface::facade::ParameterKind::Type,
                alias: 1,
                source: 1,
            },
        ]
    );
    let alias = contract.alias_targets().next().expect("one Element alias");
    assert_eq!(source_site.target_selector(), alias.selector());
    assert_ne!(source_site.widget_evidence(), &[0; 32]);
    assert_eq!(contract.projections().len(), 1);
    assert_eq!(
        contract
            .projections()
            .next()
            .expect("one direct projection")
            .arguments(),
        [profile::contract::Argument::fresh()]
    );
    let endpoint = contract
        .operation_endpoints()
        .next()
        .expect("one exact text operation endpoint");
    assert_eq!(contract.operation_endpoints().len(), 1);
    assert_eq!(endpoint.site().root(), "iced");
    assert_eq!(endpoint.site().path(), "widget::text");
    assert_eq!(endpoint.source_selector(), source_site.selector());
    assert_eq!(
        endpoint.projection_selector(),
        contract
            .projections()
            .next()
            .expect("one endpoint projection")
            .selector()
    );
    let mut linked = Sha256::new();
    linked.update(b"rawr.iced.operation-endpoint.linked-host/v3");
    linked.update(8_u32.to_le_bytes());
    linked.update(b"iced::widget::text");
    linked.update(b"owned String -> &'view str -> iced::widget::Text<'view, Theme>");
    linked.update(b"TextProxy/owned-immutable-string/v1");
    for hook in [
        b"tag".as_slice(),
        b"state",
        b"size",
        b"layout",
        b"update",
        b"mouse_interaction",
        b"draw",
        b"operate",
    ] {
        linked.update(hook.len().to_le_bytes());
        linked.update(hook);
    }
    for inherited in [b"diff".as_slice(), b"overlay"] {
        linked.update(inherited.len().to_le_bytes());
        linked.update(inherited);
    }
    linked.update(b"caller-owned Tree retains text paragraph state");
    assert_eq!(endpoint.linked(), &<[u8; 32]>::from(linked.finalize()));

    let checked = rawr_iced::profile::Checked::from_fingerprint([0x61; 32]);
    let demand = profile::demand(&definition, &contract, &checked, &limits).unwrap();
    assert_eq!(demand.hooks().len(), 1);

    let generated = profile::Proof::generate(&definition, &demand, &limits).unwrap();
    assert!(generated.has_generated_projections());
    assert_ne!(generated.fingerprint(), &[0; 32]);
    let source = std::str::from_utf8(generated.generated_source()).unwrap();
    assert!(source.contains("mod __rawr_profile"));
    assert!(source.contains("const PROJECTIONS"));
    assert!(source.contains("const PROJECTION_SOURCES"));
    assert!(source.contains("const ALIAS_TARGETS"));
    assert!(source.contains("const EXTENSION_ABIS"));
    assert!(source.contains("const OPERATION_ENDPOINTS"));
    assert_eq!(source.matches("OperationEndpoint::new(").count(), 1);
    assert_eq!(source.matches("ExtensionAbi::new(").count(), 4);
    assert_eq!(source.matches("DefaultF32 { parameter: 0 }").count(), 2);
    assert!(source.contains("AliasTarget::new(\"iced\", \"Element\""));
    assert!(source.contains("W: iced::advanced::Widget"));
    assert!(source.contains("iced::Element<'view"));
    assert_eq!(source.matches("fn project<'view").count(), 1);
    assert!(!source.contains("widget::button"));
    assert!(!source.contains("Dial"));
    assert!(!source.contains("widget::text::Text"));
    assert!(source.contains("iced::advanced::Widget"));
    assert!(source.contains("pub struct Replay<"));
    assert!(source.contains("insert_projectable"));
    assert!(source.contains("context.profile_event_mapper"));
    assert!(!source.contains("project_native"));
    for path in [
        "advanced::graphics::core::widget::Text",
        "advanced::graphics::core::widget::text::Text",
        "advanced::graphics::futures::core::widget::Text",
        "advanced::graphics::futures::core::widget::text::Text",
        "advanced::widget::Text",
        "advanced::widget::text::Text",
    ] {
        assert!(!source.contains(&format!("ProjectionSourceSite::new(\"iced\", \"{path}\"")));
    }
}

#[test]
fn ordinary_iced_items_do_not_become_profile_inventory() {
    let limits = rawr_import::cargo::Limits::standard();
    let graph = rawr_import::test_fixture::iced_profile();
    assert!(graph
        .definitions()
        .any(|definition| { definition.display_path().last() == Some("button") }));
    assert!(graph
        .definitions()
        .any(|definition| { definition.display_path().last() == Some("primary") }));

    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    let contract = profile::classify(&definition, &graph, &limits).unwrap();
    assert_eq!(contract.evidence().len(), 19);
    assert_eq!(contract.extension_abis().len(), 4);
    assert_eq!(contract.projections().len(), 1);
}

/// On iced master the `Widget` implementation is the whole projection
/// proof; a source type without one is not projectable.
#[test]
fn source_without_a_widget_impl_is_rejected() {
    let limits = rawr_import::cargo::Limits::standard();
    let graph = rawr_import::test_fixture::iced_profile_without_widget_impl();
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    assert!(matches!(
        profile::classify(&definition, &graph, &limits),
        Err(profile::Error::ForeignEvidence)
    ));
}

#[cfg(feature = "profile-selection-diagnostic")]
#[test]
fn private_selection_diagnostic_tracks_existing_exact_boundaries() {
    let limits = rawr_import::cargo::Limits::standard();
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();

    let missing = rawr_import::test_fixture::iced_profile_element_case(
        rawr_import::test_fixture::IcedElementCase::MissingAlias,
    )
    .unwrap();
    assert!(matches!(
        profile::classify(&definition, &missing, &limits),
        Err(profile::Error::ForeignEvidence)
    ));
    assert_eq!(
        rawr_iced::profile::selection_diagnostic(),
        Some("element-export")
    );

    let unprojectable = rawr_import::test_fixture::iced_profile_without_widget_impl();
    assert!(matches!(
        profile::classify(&definition, &unprojectable, &limits),
        Err(profile::Error::ForeignEvidence)
    ));
    assert_eq!(
        rawr_iced::profile::selection_diagnostic(),
        Some("widget-implementations")
    );

    let exact = rawr_import::test_fixture::iced_profile();
    assert!(profile::classify(&definition, &exact, &limits).is_ok());
    assert!(rawr_iced::profile::selection_diagnostic().is_none());
}

#[test]
fn public_element_alias_selects_primary_with_overlay_sibling_in_any_input_order() {
    let limits = rawr_import::cargo::Limits::standard();
    let primary = rawr_import::test_fixture::iced_profile_element_case(
        rawr_import::test_fixture::IcedElementCase::Primary,
    )
    .unwrap();
    assert_eq!(
        primary
            .definitions()
            .filter(|definition| definition.display_path().last() == Some("Element"))
            .count(),
        3
    );
    let shuffled = rawr_import::test_fixture::iced_profile_element_case(
        rawr_import::test_fixture::IcedElementCase::Shuffled,
    )
    .unwrap();
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    let primary_contract = profile::classify(&definition, &primary, &limits).unwrap();
    let shuffled_contract = profile::classify(&definition, &shuffled, &limits).unwrap();
    assert_eq!(
        primary_contract.rules_digest(),
        shuffled_contract.rules_digest()
    );
    assert_eq!(primary_contract.projections().len(), 1);
    assert_eq!(shuffled_contract.projections().len(), 1);
}

#[test]
fn malformed_public_element_aliases_fail_closed() {
    let limits = rawr_import::cargo::Limits::standard();
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    for case in [
        rawr_import::test_fixture::IcedElementCase::MissingAlias,
        rawr_import::test_fixture::IcedElementCase::RejectedAlias,
        rawr_import::test_fixture::IcedElementCase::NonTypeAlias,
        rawr_import::test_fixture::IcedElementCase::NonDefinitionTarget,
        rawr_import::test_fixture::IcedElementCase::ForeignTarget,
        rawr_import::test_fixture::IcedElementCase::OverlayTarget,
        rawr_import::test_fixture::IcedElementCase::NonStructTarget,
        rawr_import::test_fixture::IcedElementCase::MisorderedArguments,
    ] {
        let graph = rawr_import::test_fixture::iced_profile_element_case(case).unwrap();
        let result = profile::classify(&definition, &graph, &limits);
        assert!(
            matches!(result, Err(profile::Error::ForeignEvidence)),
            "{case:?}: {:?}",
            result.err()
        );
    }
    assert!(rawr_import::test_fixture::iced_profile_element_case(
        rawr_import::test_fixture::IcedElementCase::DuplicateAlias,
    )
    .is_err());
}

#[test]
fn anonymous_element_import_does_not_compete_with_the_public_alias() {
    let limits = rawr_import::cargo::Limits::standard();
    let graph = rawr_import::test_fixture::iced_profile_element_case(
        rawr_import::test_fixture::IcedElementCase::AnonymousSibling,
    )
    .unwrap();
    assert_eq!(graph.anonymous_imports().len(), 1);
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    let contract = profile::classify(&definition, &graph, &limits).unwrap();
    assert_eq!(contract.evidence().len(), 19);
    assert_eq!(contract.extension_abis().len(), 4);
    assert_eq!(contract.projections().len(), 1);
}

#[test]
fn profile_version_fourteen_changes_fingerprints_and_rejects_prior_proofs() {
    let limits = rawr_import::cargo::Limits::standard();
    let graph = rawr_import::test_fixture::iced_profile();
    let current = rawr_iced::profile::Definition::new(&limits).unwrap();
    let version_two = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 2,
    };
    let version_three = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 3,
    };
    let version_four = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 4,
    };
    let version_five = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 5,
    };
    let version_six = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 6,
    };
    let version_seven = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 7,
    };
    let version_eight = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 8,
    };
    let version_nine = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 9,
    };
    let version_ten = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 10,
    };
    let version_eleven = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 11,
    };
    let version_twelve = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 12,
    };
    let version_thirteen = PriorVersion {
        definition: rawr_iced::profile::Definition::new(&limits).unwrap(),
        version: 13,
    };
    assert_eq!(
        <rawr_iced::profile::Definition as profile::Definition<
            rawr_iced::profile::Checked,
        >>::version(&current),
        profile::Version::new(17).unwrap()
    );
    let current_contract = profile::classify(&current, &graph, &limits).unwrap();
    let version_two_contract = profile::classify(&version_two, &graph, &limits).unwrap();
    let version_three_contract = profile::classify(&version_three, &graph, &limits).unwrap();
    let version_four_contract = profile::classify(&version_four, &graph, &limits).unwrap();
    let version_five_contract = profile::classify(&version_five, &graph, &limits).unwrap();
    let version_six_contract = profile::classify(&version_six, &graph, &limits).unwrap();
    let version_seven_contract = profile::classify(&version_seven, &graph, &limits).unwrap();
    let version_eight_contract = profile::classify(&version_eight, &graph, &limits).unwrap();
    let version_nine_contract = profile::classify(&version_nine, &graph, &limits).unwrap();
    let version_ten_contract = profile::classify(&version_ten, &graph, &limits).unwrap();
    let version_eleven_contract = profile::classify(&version_eleven, &graph, &limits).unwrap();
    let version_twelve_contract = profile::classify(&version_twelve, &graph, &limits).unwrap();
    let version_thirteen_contract = profile::classify(&version_thirteen, &graph, &limits).unwrap();
    assert_ne!(
        current_contract.rules_digest(),
        version_two_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_three_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_four_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_five_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_six_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_seven_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_eight_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_nine_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_ten_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_eleven_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_twelve_contract.rules_digest()
    );
    assert_ne!(
        current_contract.rules_digest(),
        version_thirteen_contract.rules_digest()
    );
    let checked = rawr_iced::profile::Checked::from_fingerprint([0x62; 32]);
    for prior in [
        &version_two_contract,
        &version_three_contract,
        &version_four_contract,
        &version_five_contract,
        &version_six_contract,
        &version_seven_contract,
        &version_eight_contract,
        &version_nine_contract,
        &version_ten_contract,
        &version_eleven_contract,
        &version_twelve_contract,
        &version_thirteen_contract,
    ] {
        assert!(matches!(
            profile::demand(&current, prior, &checked, &limits),
            Err(profile::Error::DefinitionDrift)
        ));
    }
    let current_demand = profile::demand(&current, &current_contract, &checked, &limits).unwrap();
    let previous_demand =
        profile::demand(&version_three, &version_three_contract, &checked, &limits).unwrap();
    let current_proof = profile::Proof::generate(&current, &current_demand, &limits).unwrap();
    let previous_proof =
        profile::Proof::generate(&version_three, &previous_demand, &limits).unwrap();
    assert_ne!(current_proof.fingerprint(), previous_proof.fingerprint());
}

#[test]
fn phase_seven_equivalence_seed_is_machine_readable_and_complete() {
    let ledger: serde_json::Value =
        serde_json::from_str(include_str!("../../profile-equivalence.json")).unwrap();
    assert_eq!(ledger["schema"], "rawr.iced.profile-equivalence/v1");
    assert_eq!(ledger["phase"], 6);
    assert!(ledger["positive"].as_array().unwrap().len() >= 5);
    assert!(ledger["semantic_retain"].as_array().unwrap().len() >= 7);
    assert!(ledger["negative"].as_array().unwrap().len() >= 5);
}
