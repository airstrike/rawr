use crate::p2;

use std::collections::{BTreeMap, BTreeSet};

use rawr_import::classify::representability::Representability;
use rawr_import::classify::Policy;

#[test]
fn real_exports_share_definition_plans_and_nest_associated_outcomes() {
    let _turn = rawr_import::store::test_turn();
    let graph = p2::graph();
    let export_count = graph.exports().len();
    let definition_count = graph.definitions().len();
    let rejected_associated = graph
        .definitions()
        .find(|definition| {
            definition.display_path().last() == Some("ZERO")
                && matches!(
                    definition.kind(),
                    rawr_import::api::model::Kind::Rejected {
                        class: rawr_import::api::model::RejectedClass::AssociatedConstant,
                        ..
                    }
                )
                && matches!(
                    definition.owner(),
                    rawr_import::api::model::Owner::Implementation(_)
                )
        })
        .expect("the rejected associated constant remains a total definition node");
    let rawr_import::api::model::Owner::Implementation(associated_owner) =
        rejected_associated.owner()
    else {
        unreachable!("the search above proves the implementation owner")
    };
    assert!(graph.implementations().any(|implementation| {
        implementation.id() == associated_owner
            && implementation
                .associated()
                .any(|id| id == rejected_associated.id())
    }));
    let contract = graph
        .classify(&Policy::standard(), &rawr_import::cargo::Limits::standard())
        .expect("the real extracted graph classifies");

    assert_eq!(contract.outcomes().len(), export_count);
    assert_eq!(
        contract.outcomes().len(),
        contract.raw_graph().exports().len()
    );
    let plan_ids = contract
        .definition_plans()
        .map(rawr_import::classify::DefinitionPlan::definition)
        .collect::<BTreeSet<_>>();
    assert_eq!(plan_ids.len(), contract.definition_plans().len());
    assert_eq!(contract.definition_plans().len(), definition_count);

    let top_level = p2::outcome(&contract, "facade_a", &["const_bound"]);
    assert!(matches!(
        top_level.identity().selector(),
        rawr_import::surface::identity::Selector::Function { item }
            if Some(item) == top_level.identity().definition().as_ref()
    ));

    let first = p2::outcome(&contract, "facade_a", &["RenamedNode"]);
    let second = p2::outcome(&contract, "facade_b", &["RenamedNode"]);
    assert_eq!(
        first.identity().definition(),
        second.identity().definition()
    );
    assert_ne!(first.identity().export(), second.identity().export());
    assert_ne!(first.identity().as_bytes(), second.identity().as_bytes());

    let mut associated_by_impl = BTreeMap::new();
    for implementation in p2::outcome(&contract, "facade_a", &["ImplSurface"]).implementations() {
        let associated = implementation.associated().collect::<Vec<_>>();
        assert!(!associated.is_empty());
        assert!(associated.iter().all(|outcome| outcome.origin().is_some()));
        assert!(associated_by_impl
            .insert(implementation.identity(), associated.len())
            .is_none());
    }
    assert!(!associated_by_impl.is_empty());

    let mut implementation_support = 0_usize;
    for outcome in contract.outcomes() {
        for implementation in outcome.implementations() {
            for support in contract
                .implementation_support(implementation.identity())
                .expect("the implementation key belongs to this contract")
            {
                implementation_support += 1;
                assert!(matches!(
                    support,
                    rawr_import::classify::SupportOutcome::Complete {
                        provenance:
                            rawr_import::classify::SupportProvenance::Implementation(identity),
                        ..
                    } | rawr_import::classify::SupportOutcome::Rejected {
                        provenance:
                            rawr_import::classify::SupportProvenance::Implementation(identity),
                        ..
                    } if identity == implementation.identity()
                ));
            }
        }
    }
    assert!(
        implementation_support > 0,
        "the real graph retains implementation-role recursive support"
    );

    let record = p2::outcome(&contract, "facade_a", &["Record"]);
    let (implementation, zero) = record
        .implementations()
        .find_map(|implementation| {
            implementation
                .associated()
                .find(|outcome| outcome.name() == "ZERO")
                .map(|zero| (implementation, zero))
        })
        .expect("Record retains its rejected associated constant");
    assert_eq!(
        zero.identity().implementation(),
        Some(implementation.identity())
    );
    assert!(matches!(
        zero.identity().selector(),
        rawr_import::surface::identity::Selector::AssociatedConstant { .. }
    ));
    assert!(matches!(
        zero.representability(),
        Representability::Rejected(reason)
            if reason.kind()
                == rawr_import::classify::representability::reject::Kind::Extraction
                && reason.extraction_reason().is_some()
    ));

    assert!(contract.definition_plans().any(|plan| {
        contract
            .definition_support(plan.definition())
            .is_some_and(|support| support.len() != 0)
    }));
    for plan in contract.definition_plans() {
        assert!(contract
            .definition_support(plan.definition())
            .expect("the definition key belongs to this contract")
            .all(|support| matches!(
                support,
                rawr_import::classify::SupportOutcome::Complete {
                    provenance: rawr_import::classify::SupportProvenance::Definition(identity),
                    ..
                } | rawr_import::classify::SupportOutcome::Rejected {
                    provenance: rawr_import::classify::SupportProvenance::Definition(identity),
                    ..
                } if identity == plan.definition()
            )));
    }
    assert_eq!(
        contract.support().len(),
        contract.raw_graph().support().len()
    );
    assert!(contract.outcomes().any(|outcome| matches!(
        outcome.representability(),
        Representability::FunctionItem(_)
            | Representability::Value(_)
            | Representability::Rejected(_)
    )));
}

#[test]
fn real_grammar_and_private_constructor_proofs_are_closed() {
    let _turn = rawr_import::store::test_turn();
    let graph = p2::graph();
    let opaque_definition = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some("OpaqueFields"))
        .expect("the real graph retains OpaqueFields");
    assert!(matches!(
        opaque_definition.kind(),
        rawr_import::api::model::Kind::Struct { fields, .. }
            if fields.form() == rawr_import::api::model::FieldForm::Record
                && !fields.public_complete()
                && fields.public_fields().len() == 1
                && fields.public_fields()[0].name() == "visible"
    ));
    let alias_target = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some("AliasTarget"))
        .expect("the real graph retains the tuple support definition");
    assert!(matches!(
        alias_target.kind(),
        rawr_import::api::model::Kind::Struct { fields, .. }
            if fields.form() == rawr_import::api::model::FieldForm::Tuple
                && fields.public_complete()
                && fields.public_fields().len() == 1
    ));
    let variant_surface = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some("VariantSurface"))
        .expect("the real graph retains the tuple enum variant");
    assert!(matches!(
        variant_surface.kind(),
        rawr_import::api::model::Kind::Enum { variants, .. }
            if variants.len() == 1
                && variants[0].field_set().form()
                    == rawr_import::api::model::FieldForm::Tuple
    ));
    let contract = graph
        .classify(&Policy::standard(), &rawr_import::cargo::Limits::standard())
        .expect("the real extracted graph classifies");

    let opaque = p2::outcome(&contract, "facade_a", &["OpaqueFields"]);
    assert!(matches!(
        opaque.representability(),
        Representability::Rejected(reason)
            if reason.kind()
                == rawr_import::classify::representability::reject::Kind::UnsupportedPrimitive
                && reason.span().file().ends_with("support-api/src/lib.rs")
    ));
    assert!(matches!(
        p2::outcome(&contract, "facade_a", &["ImplSurface"]).representability(),
        Representability::Struct(plan)
            if plan.form() == rawr_import::api::model::FieldForm::Unit
                && plan.constructible()
                && plan.constructor().is_some()
    ));
    let external_alias = p2::outcome(&contract, "facade_a", &["ExternalAlias"]);
    assert!(matches!(
        external_alias.representability(),
        Representability::Rejected(reason)
            if reason.kind()
                == rawr_import::classify::representability::reject::Kind::UnnameableType
                && reason.span().file().ends_with("facade-api/src/lib.rs")
    ));
    assert!(matches!(
        p2::outcome(&contract, "facade_a", &["VariantSurface"]).representability(),
        Representability::Rejected(reason)
            if reason.kind()
                == rawr_import::classify::representability::reject::Kind::UnnameableType
    ));
}
