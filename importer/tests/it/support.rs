use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_import::api::identity::Authority;
use rawr_import::api::{ledger, model};
use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;
use rawr_import::process::Native;
use sha2::{Digest as _, Sha256};

static TEST_ID: AtomicU64 = AtomicU64::new(1);

struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cross_package_support_and_duplicate_aliases_capture_once() {
    let _turn = rawr_import::store::test_turn();
    let fixture_digest_oracle = fixture_digests();
    let root = std::env::temp_dir().join(format!(
        "rawr-support-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("the unique support project can be created");
    let project = Project(fs::canonicalize(root).expect("the support project is canonical"));
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let graph = request(&project.0, &limits)
        .resolve(&native, &limits)
        .and_then(|resolution| rawr_import::extract::from_resolution(resolution, &native, &limits))
        .expect("the cross-package compiler capture canonicalizes");

    assert_eq!(
        graph.invocation_count(),
        1,
        "only the selected facade is probed"
    );
    for alias in ["facade_a", "facade_b"] {
        assert!(imported(&graph, alias, &["RenamedNode"]));
        assert!(imported(&graph, alias, &["BorrowConvert"]));
        assert!(imported(&graph, alias, &["BUILD_MARKER"]));
        assert!(imported(&graph, alias, &["OpaqueFields"]));
        assert!(imported(&graph, alias, &["opaque_private"]));
        assert!(imported(&graph, alias, &["capture"]));
        assert!(imported(&graph, alias, &["boxed_iterator"]));
        assert!(imported(&graph, alias, &["bound_only"]));
        assert!(imported(&graph, alias, &["repeated"]));
        assert!(imported(&graph, alias, &["ExternalAlias"]));
        assert!(imported(&graph, alias, &["normal_only"]));
        for path in [
            "FieldSurface",
            "VariantSurface",
            "SURFACE_CONST",
            "SURFACE_STATIC",
            "TraitSurface",
            "ImplSurface",
            "PrivateSurface",
        ] {
            assert!(imported(&graph, alias, &[path]));
        }
        assert!(!graph.exports().any(|outcome| {
            outcome.key().root_alias() == alias && outcome.key().path().eq(["doc_only"])
        }));
        for path in [
            "BlanketImpl",
            "DeclaredBounds",
            "ExternalImpl",
            "PrimitiveImpl",
            "ReferenceImpl",
        ] {
            assert!(imported(&graph, alias, &[path]));
        }
    }
    assert!(graph.exports().any(|outcome| {
        matches!(outcome, ledger::Path::Imported(export)
            if export.key().root_alias() == "facade_a"
                && export.key().path().eq(["RenamedNode"])
                && export.aliases().len() == 1
                && export.reexports().len() == 2
                && export.reexports().all(|hop| hop.item().is_some()))
    }));
    assert!(graph.exports().all(|outcome| {
        !outcome
            .key()
            .path()
            .any(|part| matches!(part, "private" | "Unrelated" | "unrelated"))
    }));
    assert!(graph.definitions().all(|definition| !matches!(
        definition.display_path().last(),
        Some("Unrelated") | Some("unrelated")
    )));

    let cargo_packages = graph
        .definitions()
        .filter_map(|definition| definition.id().unit().package())
        .map(|package| package.cargo_id().to_owned())
        .collect::<BTreeSet<_>>();
    assert!(cargo_packages
        .iter()
        .any(|package| package.contains("facade-api")));
    assert!(cargo_packages
        .iter()
        .any(|package| package.contains("support-api")));
    assert!(cargo_packages
        .iter()
        .any(|package| package.contains("external-api")));
    assert!(!cargo_packages
        .iter()
        .any(|package| package.contains("compiler-helper")));

    assert!(graph.definitions().any(|definition| {
        definition.display_path().last() == Some("BorrowConvert")
            && matches!(definition.kind(), model::Kind::Trait { .. })
    }));
    assert!(graph.definitions().any(|definition| {
        definition.display_path().last() == Some("Node")
            && matches!(definition.kind(), model::Kind::Struct { generics, .. }
                if generics.parameters().next().is_some()
                    && generics.predicates().next().is_some())
    }));
    assert!(graph.definitions().any(|definition| {
        definition.display_path().last() == Some("Output")
            && matches!(definition.kind(), model::Kind::AssociatedType { bounds, .. }
                if !bounds.is_empty())
    }));
    assert!(graph.definitions().any(|definition| {
        definition.display_path().last() == Some("DEFAULT")
            && matches!(
                definition.kind(),
                model::Kind::Const {
                    value: model::Default::Absent,
                    ..
                }
            )
    }));
    assert!(graph.definitions().any(|definition| {
        definition.display_path().last() == Some("map_node")
            && matches!(definition.kind(), model::Kind::Function { generics, .. }
            if generics.predicates().any(|predicate| matches!(
                predicate,
                model::Predicate::Bound {
                    bound: model::Bound::Trait { binders, .. },
                    ..
                } if binders.len() == 1
                    && binders[0].depth() == 0
                    && binders[0].index() == 0
            )))
    }));
    assert_impl_shapes(&graph);
    assert_declared_bounds(&graph);
    assert_semantic_edge_cases(&graph);
    assert_selected_alias_implementations(&graph);
    assert_selected_surface_implementations(&graph);
    assert_field_forms(&graph);
    assert_constant_alias_evidence(&graph);
    assert!(graph.definitions().all(|definition| {
        !matches!(
            definition.display_path().last(),
            Some("HiddenTrait") | Some("hidden")
        )
    }));
    assert!(graph.support().any(|outcome| {
        let id = match outcome {
            ledger::Support::Complete { id, .. } | ledger::Support::Rejected { id, .. } => id,
        };
        matches!(id.unit().authority(), Authority::Cargo(package) if package.name() == "support-api")
    }));
    assert_eq!(
        fixture_digest_oracle,
        fixture_digests(),
        "fixtures remain unchanged"
    );
}

#[test]
fn type_constants_schedule_exact_named_alias_identities() {
    let _turn = rawr_import::store::test_turn();
    let graph = constant_graph().expect("the pinned type-constant fixture canonicalizes");

    assert_projected_constant_alias(&graph, "projected_array", 0);
    assert_projected_constant_alias(&graph, "second_projected_array", 1);
    assert_inherent_constant_alias(&graph);
    let lengths = graph
        .definitions()
        .filter(|definition| definition.display_path().last() == Some("LENGTH"))
        .collect::<Vec<_>>();
    assert_eq!(lengths.len(), 3, "every LENGTH sibling has one identity");
    assert!(lengths
        .iter()
        .all(|definition| definition.id().namespace()
            == rawr_import::api::identity::Namespace::Value));
    let implementation_length = lengths
        .iter()
        .copied()
        .find(|definition| {
            matches!(
                definition.kind(),
                model::Kind::TypeConst {
                    rhs: model::Default::Present(model::Const::Literal { value, .. }),
                    ..
                } if value.as_ref() == "3_usize"
            )
        })
        .expect("the concrete projection retains its exact implementation member");
    let model::Owner::Implementation(implementation_owner) = implementation_length.owner() else {
        panic!("the concrete projected member retains its implementation owner");
    };
    let implementation = graph
        .implementations()
        .find(|implementation| implementation.id() == implementation_owner)
        .expect("the concrete projected implementation is retained exactly once");
    assert!(matches!(
        implementation.self_type(),
        model::Type::Definition { id, arguments }
            if definition_name(&graph, id) == Some("ConcreteLength") && arguments.is_empty()
    ));
    assert!(matches!(
        implementation.trait_ref(),
        model::Trait::Trait { id, arguments }
            if definition_name(&graph, id) == Some("ProjectedLength")
                && matches!(arguments.as_ref(),
                    [model::Argument::Type(model::Type::Definition { id, arguments })]
                        if definition_name(&graph, id) == Some("ConcreteLength")
                            && arguments.is_empty())
    ));
    assert_eq!(
        implementation
            .associated()
            .filter(|associated| *associated == implementation_length.id())
            .count(),
        1,
        "the exact selected implementation member is retained once",
    );
    assert!(graph.definitions().all(|definition| {
        !matches!(
            definition.kind(),
            model::Kind::TypeConst {
                rhs: model::Default::Present(model::Const::Literal { value, .. }),
                ..
            } if value.as_ref() == "9_usize"
        )
    }));
    let unrelated_implementation = graph
        .implementations()
        .find(|implementation| {
            matches!(
                implementation.self_type(),
                model::Type::Definition { id, arguments }
                    if definition_name(&graph, id) == Some("UnrelatedLength")
                        && arguments.is_empty()
            )
        })
        .expect("the unrelated public nominal retains only its implementation header");
    assert_eq!(
        unrelated_implementation.associated().len(),
        0,
        "a nominal surface does not eagerly admit unrelated associated members",
    );

    let required = type_constant(&graph, "REQUIRED");
    assert!(matches!(
        required,
        model::Kind::TypeConst {
            ty: model::Type::Primitive(ty),
            rhs: model::Default::Absent,
        } if ty.as_ref() == "usize"
    ));
    let defaulted = type_constant(&graph, "DEFAULTED");
    assert!(matches!(
        defaulted,
        model::Kind::TypeConst {
            rhs: model::Default::Present(model::Const::Literal { value, .. }),
            ..
        } if value.as_ref() == "5_usize"
    ));
    let model::Kind::TypeConst {
        rhs:
            model::Default::Present(model::Const::Unevaluated {
                arguments: forwarded,
                ..
            }),
        ..
    } = type_constant(&graph, "FORWARDED")
    else {
        panic!("the generic trait-default RHS remains identity-bearing");
    };
    assert!(matches!(
        forwarded.as_ref(),
        [model::Argument::Type(model::Type::SelfType(owner))]
            if definition_name(&graph, owner) == Some("DefaultedLength")
    ));
    assert!(matches!(
        type_constant(&graph, "FREE_LENGTH"),
        model::Kind::TypeConst {
            rhs: model::Default::Present(model::Const::Literal { value, .. }),
            ..
        } if value.as_ref() == "6_usize"
    ));

    assert!(matches!(
        array_length(&graph, "concrete_array"),
        model::Const::Unevaluated { arguments, .. }
            if matches!(arguments.as_ref(),
                [model::Argument::Type(model::Type::Definition { id, arguments })]
                    if definition_name(&graph, id) == Some("ConcreteLength")
                        && arguments.is_empty())
    ));
}

fn type_constant<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    name: &str,
) -> &'a model::Kind {
    graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some(name))
        .unwrap_or_else(|| panic!("type constant {name} is retained"))
        .kind()
}

#[test]
fn opaque_parameter_owners_follow_the_exact_compiler_generics_chain() {
    let _turn = rawr_import::store::test_turn();
    let root = std::env::temp_dir().join(format!(
        "rawr-opaque-ownership-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("the unique opaque ownership project can be created");
    let project = Project(fs::canonicalize(root).expect("the fixture project is canonical"));
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let graph = request(&project.0, &limits)
        .resolve(&native, &limits)
        .and_then(|resolution| rawr_import::extract::from_resolution(resolution, &native, &limits))
        .expect("the opaque ownership fixture canonicalizes");

    for alias in ["facade_a", "facade_b"] {
        for path in [
            "Projected",
            "opaque_projection",
            "sibling_opaque_projection",
            "opaque_higher_ranked",
            "RpititProjection",
            "RpititSurface",
            "VtableBase",
            "VtableContract",
            "vtable_contract",
            "opaque_outlives",
            "sibling_opaque_outlives",
            "opaque_static_outlives",
            "opaque_without_outlives",
            "opaque_hrtb_outlives",
        ] {
            assert!(imported(&graph, alias, &[path]));
        }
    }
    assert_opaque_parameter_ownership(&graph);
    assert_opaque_outlives_ownership(&graph);
    assert_anonymous_associated_support(&graph);
    assert_full_trait_contract(&graph);
}

#[test]
fn implementation_relevance_uses_exact_compiler_candidates() {
    let _turn = rawr_import::store::test_turn();
    let root = std::env::temp_dir().join(format!(
        "rawr-implementation-relevance-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("the unique implementation relevance project can be created");
    let project = Project(fs::canonicalize(root).expect("the fixture project is canonical"));
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let graph = request(&project.0, &limits)
        .resolve(&native, &limits)
        .and_then(|resolution| rawr_import::extract::from_resolution(resolution, &native, &limits))
        .expect("the implementation relevance fixture canonicalizes");

    assert_impl_shapes(&graph);
    assert_exact_implementation_relevance(&graph);
}

fn assert_field_forms(graph: &rawr_import::api::Graph<rawr_import::api::Extracted>) {
    let kind = |name| {
        graph
            .definitions()
            .find(|definition| definition.display_path().last() == Some(name))
            .map(model::Definition::kind)
            .unwrap_or_else(|| panic!("fixture retains `{name}`"))
    };

    assert!(matches!(
        kind("FieldLeaf"),
        model::Kind::Struct { fields, .. }
            if fields.form() == model::FieldForm::Unit
                && fields.public_complete()
                && fields.public_fields().is_empty()
    ));
    assert!(matches!(
        kind("AliasTarget"),
        model::Kind::Struct { fields, .. }
            if fields.form() == model::FieldForm::Tuple
                && fields.public_complete()
                && fields.public_fields().len() == 1
    ));
    assert!(matches!(
        kind("OpaqueFields"),
        model::Kind::Struct { fields, .. }
            if fields.form() == model::FieldForm::Record
                && !fields.public_complete()
                && fields.public_fields().len() == 1
                && fields.public_fields()[0].name() == "visible"
    ));
    assert!(matches!(
        kind("VariantSurface"),
        model::Kind::Enum { variants, .. }
            if variants.len() == 1
                && variants[0].field_set().form() == model::FieldForm::Tuple
                && variants[0].field_set().public_complete()
    ));
    let canonical =
        std::str::from_utf8(graph.canonical_bytes()).expect("canonical API evidence is UTF-8 JSON");
    assert!(!canonical.contains("PrivateField"));
    assert!(!canonical.contains("\"hidden\""));
}

#[test]
fn duplicate_crate_names_keep_distinct_artifact_and_compiler_identities() {
    let _turn = rawr_import::store::test_turn();
    let root = std::env::temp_dir().join(format!(
        "rawr-duplicate-crates-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("the duplicate-crate project can be created");
    let project = Project(fs::canonicalize(root).expect("the project is canonical"));
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the test environment is sealable");
    let request = Request::builder(&project.0, &project.0.join(".rawr/cache"), &limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(Policy::manifest(&limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("duplicates", &fixture("duplicate-facade"), &limits)
                .and_then(|dependency| dependency.package("duplicate-facade"))
                .expect("the duplicate facade selection is exact"),
        )
        .expect("the first duplicate dependency fits the request")
        .dependency(
            Dependency::path("duplicates_profile", &fixture("duplicate-facade"), &limits)
                .and_then(|dependency| dependency.package("duplicate-facade"))
                .expect("the duplicate facade profile selection is exact"),
        )
        .expect("the second duplicate dependency fits the request")
        .build()
        .expect("the duplicate fixture request seals");
    let graph = request
        .resolve(&native, &limits)
        .and_then(|resolution| rawr_import::extract::from_resolution(resolution, &native, &limits))
        .expect("both same-name metadata artifacts canonicalize");

    assert!(imported(&graph, "duplicates", &["VersionOne"]));
    assert!(imported(&graph, "duplicates", &["VersionTwo"]));
    assert!(graph.exports().any(|outcome| matches!(
        outcome,
        ledger::Path::Imported(export)
            if export.key().root_alias() == "duplicates"
                && export.key().path().eq(["VersionTwo"])
                && export.reexports().any(|hop| hop.kind() == ledger::ReexportKind::Glob)
    )));
    assert!(graph.exports().any(|outcome| matches!(
        outcome,
        ledger::Path::Imported(export)
            if export.key().root_alias() == "duplicates"
                && export.key().path().eq(["duplicate_macro"])
                && export.key().namespace().as_str() == "macro"
                && export.target().unit().package().is_some_and(|package|
                    package.name() == "duplicate-api"
                        && package.version() == "1.0.0"
                        && package.source().ends_with("/duplicate-v1"))
                && export.aliases().len() == 1
                && export.aliases().all(|alias|
                    alias.from().eq(["duplicate_identity", "duplicate_macro"])
                        && alias.to().eq(["duplicate_macro"]))
                && export.reexports().len() == 1
                && export.reexports().all(|hop|
                    hop.kind() == ledger::ReexportKind::Single && hop.item().is_some())
    )));
    let identities = graph
        .definitions()
        .filter(|definition| {
            matches!(
                definition.display_path().last(),
                Some("VersionOne") | Some("VersionTwo")
            )
        })
        .map(|definition| {
            let package = definition
                .id()
                .unit()
                .package()
                .expect("the duplicate identity fixture is Cargo-owned");
            (
                package.cargo_id().to_owned(),
                package.source().to_owned(),
                definition.id().unit().crate_name().to_owned(),
                definition.id().unit().stable_crate_id().to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(identities.len(), 2);
    assert!(identities
        .iter()
        .all(|(_, _, crate_name, _)| crate_name == "duplicate_identity"));
    assert_eq!(
        identities
            .iter()
            .map(|(package, _, _, _)| package.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        identities
            .iter()
            .map(|(_, source, _, _)| source.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
}

fn assert_impl_shapes(graph: &rawr_import::api::Graph<rawr_import::api::Extracted>) {
    let primitive = implementation(graph, "PrimitiveImpl");
    assert!(matches!(primitive.self_type(), model::Type::Primitive(name) if &**name == "u32"));
    assert_eq!(
        associated_names(graph, primitive),
        BTreeSet::new(),
        "nominal-independent trait evidence retains its impl header without unrelated members"
    );

    let blanket = implementation(graph, "BlanketImpl");
    assert!(matches!(blanket.self_type(), model::Type::Parameter { .. }));
    assert!(blanket.generics().predicates().any(|predicate| matches!(
        predicate,
        model::Predicate::Bound { bound, .. }
            if trait_bound_name(graph, bound) == Some("Clone")
    )));

    let reference = implementation(graph, "ReferenceImpl");
    assert!(matches!(
        reference.self_type(),
        model::Type::Reference { .. }
    ));

    let external = implementation(graph, "ExternalImpl");
    assert!(
        matches!(external.self_type(), model::Type::Definition { id, .. }
        if definition_name(graph, id) == Some("Option"))
    );

    let fundamental = implementation(graph, "ForeignTrait");
    assert!(
        matches!(fundamental.self_type(), model::Type::Definition { id, arguments }
        if definition_name(graph, id) == Some("Box")
            && arguments.iter().any(|argument| matches!(argument,
                model::Argument::Type(model::Type::Definition { id, .. })
                    if definition_name(graph, id) == Some("Node"))))
    );
    let package = fundamental
        .id()
        .unit()
        .package()
        .expect("the cross-crate implementation is Cargo-owned");
    assert_eq!(package.name(), "support-api");
    assert!(package.source().contains("support-api"));
    let relevance = fundamental
        .relevance()
        .filter_map(|id| definition_name(graph, id))
        .collect::<BTreeSet<_>>();
    assert!(relevance.contains("ForeignTrait"));
    assert!(relevance.contains("Node"));

    for implementation in graph.implementations() {
        for associated in implementation.associated() {
            let definition = graph
                .definitions()
                .find(|definition| definition.id() == associated)
                .expect("every associated item has one definition");
            assert!(
                matches!(
                    definition.owner(),
                    model::Owner::Implementation(owner) if owner == implementation.id()
                ) || matches!(
                    (definition.owner(), implementation.trait_ref()),
                    (model::Owner::Definition(owner), model::Trait::Trait { id, .. }) if owner == id
                )
            );
        }
    }
}

fn assert_selected_alias_implementations(
    graph: &rawr_import::api::Graph<rawr_import::api::Extracted>,
) {
    let alias_trait = implementation(graph, "AliasTrait");
    assert!(
        matches!(alias_trait.self_type(), model::Type::Definition { id, .. }
        if definition_name(graph, id) == Some("AliasTarget"))
    );
    assert!(
        associated_names(graph, alias_trait).is_empty(),
        "a demanded nominal admits its trait impl header without opening a sibling method"
    );

    let inherent = graph
        .implementations()
        .find(|implementation| {
            matches!(implementation.trait_ref(), model::Trait::Inherent)
                && matches!(implementation.self_type(), model::Type::Definition { id, .. }
                    if definition_name(graph, id) == Some("AliasTarget"))
        })
        .expect("the selected external alias expands relevant inherent implementations");
    assert!(associated_names(graph, inherent).contains("inherent_value"));
}

fn assert_selected_surface_implementations(
    graph: &rawr_import::api::Graph<rawr_import::api::Extracted>,
) {
    for (leaf, method) in [
        ("FieldLeaf", "field_leaf"),
        ("VariantLeaf", "variant_leaf"),
        ("ConstLeaf", "const_leaf"),
        ("StaticLeaf", "static_leaf"),
        ("TraitLeaf", "trait_leaf"),
        ("ImplLeaf", "impl_leaf"),
        ("ImplConstLeaf", "impl_const_leaf"),
    ] {
        let implementation = graph
            .implementations()
            .find(|implementation| {
                matches!(implementation.trait_ref(), model::Trait::Inherent)
                    && matches!(implementation.self_type(), model::Type::Definition { id, .. }
                        if definition_name(graph, id) == Some(leaf))
            })
            .unwrap_or_else(|| panic!("selected public surface expands `{leaf}` implementations"));
        assert!(associated_names(graph, implementation).contains(method));
    }
    assert!(graph.definitions().all(|definition| {
        !matches!(definition.display_path().last(), Some("PrivateSurfaceLeaf"))
    }));
    assert!(graph.implementations().all(|implementation| {
        !matches!(implementation.self_type(), model::Type::Definition { id, .. }
            if definition_name(graph, id) == Some("PrivateSurfaceLeaf"))
    }));
}

fn assert_semantic_edge_cases(graph: &rawr_import::api::Graph<rawr_import::api::Extracted>) {
    assert!(graph.exports().any(|outcome| matches!(
        outcome,
        ledger::Path::Imported(export)
            if export.key().path().eq(["private_return"])
                && export.key().namespace().as_str() == "value"
                && export.target().namespace().as_str() == "value"
                && export.target().def_path_hash() == "e856ca9d98a5ea406c438a55cad53d61"
                && export.target().unit().package().is_some_and(|package|
                    package.name() == "facade-api"
                        && package.version() == "0.1.0"
                        && package.source().ends_with("/facade-api"))
                && export.span().file().ends_with("/facade-api/src/lib.rs")
                && export.span().byte_range() == (986..1000)
    )));
    assert!(graph.exports().any(|outcome| matches!(
        outcome,
        ledger::Path::Rejected { key, reason, .. }
            if key.path().eq(["PrimitiveAlias"])
                && matches!(reason, ledger::Reason::UnsupportedPublicPath { .. })
    )));
    assert!(graph.exports().any(|outcome| matches!(
        outcome,
        ledger::Path::Imported(export)
            if export.key().root_alias() == "facade_a"
                && export.key().path().eq(["facade_macro"])
                && export.key().namespace().as_str() == "macro"
                && export.target().namespace().as_str() == "macro"
                && export.target().def_path_hash() == "e856ca9d98a5ea40f6985cfd770c62b5"
                && export.target().unit().package().is_some_and(|package|
                    package.name() == "facade-api"
                        && package.version() == "0.1.0"
                        && package.source().ends_with("/facade-api"))
                && export.aliases().next().is_none()
                && export.reexports().len() == 1
                && export.reexports().all(|hop|
                    hop.kind() == ledger::ReexportKind::MacroExport && hop.item().is_none())
                && export.span().file().ends_with("/facade-api/src/lib.rs")
                && export.span().byte_range() == (897..909)
    )));
    assert!(graph.implementations().all(|implementation| {
        !matches!(implementation.trait_ref(), model::Trait::Trait { id, .. }
            if definition_name(graph, id) == Some("BoundOnly"))
    }));

    let opaque = function_output(graph, "opaque_private");
    let capture = function_output(graph, "capture");
    assert!(matches!(opaque, model::Type::ImplTrait { captures, .. } if captures.is_empty()));
    assert!(matches!(capture, model::Type::ImplTrait { captures, .. } if !captures.is_empty()));
    assert!(contains_object_projection(function_output(
        graph,
        "boxed_iterator"
    )));

    let const_outcome = graph
        .exports()
        .find(|outcome| outcome.key().path().eq(["const_bound"]))
        .expect("the const-bound public path has one outcome");
    match const_outcome {
        ledger::Path::Imported(export) => {
            let definition = graph
                .definitions()
                .find(|definition| definition.id() == export.target())
                .expect("the imported const-bound definition is retained");
            let model::Kind::Function {
                signature,
                generics,
            } = definition.kind()
            else {
                panic!("const_bound retains exact function declaration evidence");
            };
            let parameter = generics
                .parameters()
                .find(|parameter| parameter.name() == "N")
                .expect("const_bound retains its declared const parameter");
            assert_eq!(parameter.owner(), definition.id());
            assert_eq!(parameter.index(), 0);
            assert!(matches!(
                parameter.kind(),
                model::ParameterKind::Const {
                    ty: model::Type::Primitive(name),
                    default: model::Default::Absent,
                } if name.as_ref() == "usize"
            ));
            let input = signature
                .inputs()
                .next()
                .expect("const_bound retains its array input");
            assert!(matches!(
                input,
                model::Type::Array {
                    length: model::Const::Parameter { owner, index },
                    ..
                } if owner == definition.id() && *index == 0
            ));
            assert!(matches!(
                signature.output(),
                model::Type::Array {
                    length: model::Const::Parameter { owner, index },
                    ..
                } if owner == definition.id() && *index == 0
            ));
            assert!(
                !generics
                    .predicates()
                    .any(|predicate| matches!(predicate, model::Predicate::ConstEvaluatable(_))),
                "rustc omits the redundant implicit ConstArgHasType predicate and emits no ConstEvaluatable predicate",
            );
        }
        ledger::Path::Rejected { .. } => {
            panic!("the compiler now resolves the const-bound public path")
        }
        ledger::Path::Excluded { .. } => panic!("the fixture excludes no const-bound path"),
    }

    let repeated = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some("repeated"))
        .expect("the repeated-bound function is retained");
    let model::Kind::Function { generics, .. } = repeated.kind() else {
        panic!("repeated must remain a function");
    };
    assert_eq!(
        generics
            .predicates()
            .filter(|predicate| matches!(predicate,
                model::Predicate::Bound { bound, .. }
                    if trait_bound_name(graph, bound) == Some("Clone")))
            .count(),
        1
    );
}

fn array_length<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    name: &str,
) -> &'a model::Const {
    let definition = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some(name))
        .expect("the const-array fixture function is retained");
    let model::Kind::Function { signature, .. } = definition.kind() else {
        panic!("the const-array fixture item remains a function");
    };
    let model::Type::Array { length, .. } = signature.output() else {
        panic!("the const-array fixture retains its array output");
    };
    length
}

fn assert_constant_alias_evidence(graph: &rawr_import::api::Graph<rawr_import::api::Extracted>) {
    assert!(matches!(
        array_length(graph, "four"),
        model::Const::Literal { ty, value } if ty.as_ref() == "const" && value.as_ref() == "4"
    ));
    assert!(matches!(
        array_length(graph, "eight"),
        model::Const::Literal { ty, value } if ty.as_ref() == "const" && value.as_ref() == "8"
    ));
    assert!(matches!(
        array_length(graph, "named_array"),
        model::Const::Literal { ty, value } if ty.as_ref() == "const" && value.as_ref() == "7"
    ));
    for name in ["four", "eight", "body_only"] {
        assert_eq!(
            graph
                .definitions()
                .filter(|definition| definition.display_path().last() == Some(name))
                .count(),
            1,
            "each public sibling has one exact semantic identity",
        );
    }
    assert_eq!(
        graph.fingerprint().as_bytes(),
        Sha256::digest(graph.canonical_bytes()).as_slice(),
    );
}

fn assert_projected_constant_alias(
    graph: &rawr_import::api::Graph<rawr_import::api::Extracted>,
    function_name: &str,
    expected_parameter_index: u32,
) {
    let function = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some(function_name))
        .expect("the named-constant fixture function is retained");
    let model::Kind::Function { signature, .. } = function.kind() else {
        panic!("the named-constant fixture remains a function");
    };
    let model::Type::Array { length, .. } = signature.output() else {
        panic!("the named-constant fixture retains its array output");
    };
    let model::Const::Unevaluated {
        definition,
        arguments,
    } = length
    else {
        panic!("the compiler-authentic named constant remains identity-bearing");
    };
    assert_eq!(
        definition.namespace(),
        rawr_import::api::identity::Namespace::Value,
    );
    let constant = graph
        .definitions()
        .find(|candidate| candidate.id() == definition)
        .expect("the projected constant identity is scheduled exactly once");
    assert_eq!(constant.display_path().last(), Some("LENGTH"));
    assert!(matches!(constant.kind(), model::Kind::TypeConst { .. }));
    assert!(matches!(
        constant.owner(),
        model::Owner::Definition(owner)
            if definition_name(graph, owner) == Some("ProjectedLength")
    ));
    assert!(matches!(
        arguments.as_ref(),
        [model::Argument::Type(model::Type::Parameter { owner, index })]
            if owner == function.id() && *index == expected_parameter_index
    ));
}

fn assert_inherent_constant_alias(graph: &rawr_import::api::Graph<rawr_import::api::Extracted>) {
    let function = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some("inherent_array"))
        .expect("the inherent-constant fixture function is retained");
    let model::Kind::Function { signature, .. } = function.kind() else {
        panic!("the inherent-constant fixture remains a function");
    };
    let model::Type::Array { length, .. } = signature.output() else {
        panic!("the inherent-constant fixture retains its array output");
    };
    let model::Const::Unevaluated {
        definition,
        arguments,
    } = length
    else {
        panic!("the inherent type constant remains identity-bearing");
    };
    let constant = graph
        .definitions()
        .find(|candidate| candidate.id() == definition)
        .expect("the inherent constant identity is scheduled exactly once");
    assert_eq!(constant.display_path().last(), Some("LENGTH"));
    assert!(matches!(constant.kind(), model::Kind::TypeConst { .. }));
    let model::Owner::Implementation(owner) = constant.owner() else {
        panic!("the inherent constant retains its implementation owner");
    };
    let implementation = graph
        .implementations()
        .find(|implementation| implementation.id() == owner)
        .expect("the inherent implementation is retained exactly once");
    let [model::Argument::Type(model::Type::Definition {
        id: outer,
        arguments: nested,
    })] = arguments.as_ref()
    else {
        panic!("the inherent alias has one exact applied Self type");
    };
    let outer_definition = graph
        .definitions()
        .find(|definition| definition.id() == outer)
        .expect("the inherent Self type is retained exactly once");
    assert!(outer_definition.display_path().eq(["InherentLength"]));
    assert!(matches!(
        outer_definition.kind(),
        model::Kind::Struct { .. }
    ));
    assert_eq!(outer_definition.visibility(), &model::Visibility::Public);
    assert!(matches!(
        nested.as_ref(),
        [model::Argument::Type(model::Type::Parameter { owner, index: 0 })]
            if owner == function.id()
    ));
    assert!(matches!(
        implementation.self_type(),
        model::Type::Definition { id, arguments }
            if id == outer
                && matches!(arguments.as_ref(),
                    [model::Argument::Type(model::Type::Parameter { index: 0, .. })])
    ));
}

fn assert_opaque_parameter_ownership(graph: &rawr_import::api::Graph<rawr_import::api::Extracted>) {
    let first = assert_projected_opaque(graph, "opaque_projection");
    let sibling = assert_projected_opaque(graph, "sibling_opaque_projection");
    assert_ne!(first, sibling, "sibling opaques retain distinct owners");

    let output = function_output(graph, "opaque_higher_ranked");
    let model::Type::ImplTrait { id, bounds, .. } = output else {
        panic!("opaque_higher_ranked retains its opaque output");
    };
    let binders = bounds.iter().find_map(|predicate| match predicate {
        model::Predicate::Bound {
            bound: model::Bound::Trait { binders, .. },
            ..
        } if !binders.is_empty() => Some(binders),
        _ => None,
    });
    let binders = binders.expect("the higher-ranked opaque bound retains binders");
    assert!(binders.iter().all(|binder| binder.owner() == id));

    let trait_method = graph
        .definitions()
        .find(|definition| {
            definition.display_path().last() == Some("rpitit_items")
                && matches!(definition.owner(), model::Owner::Definition(_))
        })
        .expect("the trait-owned RPITIT method is retained");
    let model::Kind::Function { signature, .. } = trait_method.kind() else {
        panic!("the trait-owned RPITIT method remains a function");
    };
    assert!(matches!(signature.output(), model::Type::Projection { .. }));
}

fn assert_anonymous_associated_support(
    graph: &rawr_import::api::Graph<rawr_import::api::Extracted>,
) {
    const PREFIX: &str = "{anonymous-associated-type:";
    let definitions = graph
        .definitions()
        .filter(|definition| definition.id().unit().crate_name() == "facade_api")
        .filter(|definition| {
            definition
                .display_path()
                .last()
                .is_some_and(|segment| segment.starts_with(PREFIX))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        definitions.len(),
        3,
        "the selected trait contract retains its RPITIT support without opening impl siblings"
    );
    assert_eq!(
        definitions
            .iter()
            .map(|definition| definition.id())
            .collect::<BTreeSet<_>>()
            .len(),
        definitions.len(),
        "anonymous associated semantic identities remain collision-free"
    );

    let segments = definitions
        .iter()
        .map(|definition| definition.display_path().last().expect("support segment"))
        .collect::<Vec<_>>();
    assert_eq!(
        segments
            .iter()
            .filter(|segment| **segment == "{anonymous-associated-type:rpitit_items#0}")
            .count(),
        1,
        "the trait contract retains the structured method anchor once"
    );
    assert_eq!(
        segments
            .iter()
            .filter(|segment| **segment == "{anonymous-associated-type:rpitit_pair#0}")
            .count(),
        1,
        "the first nested RPITIT uses the exact disambiguator"
    );
    assert_eq!(
        segments
            .iter()
            .filter(|segment| **segment == "{anonymous-associated-type:rpitit_pair#1}")
            .count(),
        1,
        "the sibling nested RPITIT uses a distinct disambiguator"
    );
    assert!(definitions.iter().all(|definition| {
        matches!(definition.owner(), model::Owner::Definition(_))
            && matches!(
                definition.origin(),
                model::Origin::Synthesized(mechanism)
                    if mechanism.as_ref() == "return-position-impl-trait-in-trait"
            )
    }));
    let ids = definitions
        .iter()
        .map(|definition| definition.id())
        .collect::<BTreeSet<_>>();
    assert!(graph.exports().all(|outcome| {
        !matches!(outcome, ledger::Path::Imported(export) if ids.contains(export.target()))
    }));
    assert!(graph.definitions().any(|definition| {
        definition.id().unit().crate_name() == "facade_api"
            && definition.display_path().last() == Some("Item")
            && !matches!(definition.origin(), model::Origin::Synthesized(_))
    }));
}

fn assert_full_trait_contract(graph: &rawr_import::api::Graph<rawr_import::api::Extracted>) {
    let contract = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some("VtableContract"))
        .expect("the selected object-safe trait is retained");
    let model::Kind::Trait {
        supertraits,
        associated,
        ..
    } = contract.kind()
    else {
        panic!("VtableContract remains a trait");
    };
    assert_eq!(
        associated
            .iter()
            .filter_map(|id| definition_name(graph, id))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["defaulted", "required"]),
        "the full selected contract retains required and defaulted members"
    );
    assert!(supertraits.iter().any(|predicate| matches!(
        predicate,
        model::Predicate::Bound { bound, .. }
            if trait_bound_name(graph, bound) == Some("VtableBase")
    )));

    let base = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some("VtableBase"))
        .expect("the selected supertrait is retained");
    let model::Kind::Trait { associated, .. } = base.kind() else {
        panic!("VtableBase remains a trait");
    };
    assert_eq!(
        associated
            .iter()
            .filter_map(|id| definition_name(graph, id))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["base"])
    );

    let function = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some("vtable_contract"))
        .expect("the trait-object surface is retained");
    let model::Kind::Function { signature, .. } = function.kind() else {
        panic!("vtable_contract remains a function");
    };
    let is_contract_object = |ty: &model::Type| {
        matches!(ty,
            model::Type::Reference { ty, .. }
                if matches!(&**ty,
                    model::Type::TraitObject { bounds, .. }
                        if bounds.iter().any(|bound| matches!(bound,
                            model::ObjectBound::Trait(model::Bound::Trait { trait_id, .. })
                                if definition_name(graph, trait_id) == Some("VtableContract")))))
    };
    assert!(signature.inputs().all(is_contract_object));
    assert!(is_contract_object(signature.output()));
}

fn assert_opaque_outlives_ownership(graph: &rawr_import::api::Graph<rawr_import::api::Extracted>) {
    let (first, first_lifetime) = opaque_outlives_bound(graph, "opaque_outlives");
    let (sibling, sibling_lifetime) = opaque_outlives_bound(graph, "sibling_opaque_outlives");
    assert_ne!(
        first, sibling,
        "sibling outlives opaques retain distinct owners"
    );
    assert!(matches!(
        first_lifetime,
        model::Lifetime::Early { owner, .. } if owner == first
    ));
    assert!(matches!(
        sibling_lifetime,
        model::Lifetime::Early { owner, .. } if owner == sibling
    ));

    let (static_owner, static_lifetime) = opaque_outlives_bound(graph, "opaque_static_outlives");
    assert!(matches!(static_lifetime, model::Lifetime::Static));
    assert_ne!(first, static_owner);

    let output = function_output(graph, "opaque_without_outlives");
    let model::Type::ImplTrait { bounds, .. } = output else {
        panic!("opaque_without_outlives retains its opaque output");
    };
    assert!(!bounds
        .iter()
        .any(|predicate| matches!(predicate, model::Predicate::TypeOutlives { .. })));

    let (hrtb, hrtb_lifetime) = opaque_outlives_bound(graph, "opaque_hrtb_outlives");
    assert!(matches!(
        hrtb_lifetime,
        model::Lifetime::Early { owner, .. } if owner == hrtb
    ));
    let model::Type::ImplTrait { bounds, .. } = function_output(graph, "opaque_hrtb_outlives")
    else {
        panic!("opaque_hrtb_outlives retains its opaque output");
    };
    assert!(bounds.iter().any(|predicate| matches!(
        predicate,
        model::Predicate::Bound {
            bound: model::Bound::Trait { binders, .. },
            ..
        } if !binders.is_empty() && binders.iter().all(|binder| binder.owner() == hrtb)
    )));
}

fn assert_exact_implementation_relevance(
    graph: &rawr_import::api::Graph<rawr_import::api::Extracted>,
) {
    let implementation_keys = graph
        .implementations()
        .map(|implementation| implementation.key())
        .collect::<Vec<_>>();
    assert_eq!(
        implementation_keys
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len(),
        implementation_keys.len(),
        "implementation identities remain unique while canonical candidate order is retained"
    );
    for implementation in graph.implementations() {
        let relevance = implementation.relevance().collect::<Vec<_>>();
        assert!(
            relevance.windows(2).all(|pair| pair[0] < pair[1]),
            "each implementation relevance set is strict and canonical"
        );
    }
    for trait_name in [
        "PrimitiveImpl",
        "BlanketImpl",
        "ReferenceImpl",
        "ExternalImpl",
        "ForeignTrait",
        "DemandRelevant",
        "ImplOwnerBound",
        "ImplOwnerReachability",
    ] {
        assert_eq!(
            graph
                .implementations()
                .filter(|implementation| matches!(
                    implementation.trait_ref(),
                    model::Trait::Trait { id, .. }
                        if definition_name(graph, id) == Some(trait_name)
                ))
                .count(),
            1,
            "the exact compiler candidate cardinality is retained for {trait_name}"
        );
    }

    let implementations = graph
        .implementations()
        .filter(|implementation| {
            matches!(implementation.trait_ref(), model::Trait::Trait { id, .. }
                if definition_name(graph, id) == Some("DemandRelevant"))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        implementations.len(),
        1,
        "the unrelated private receiver implementation has no demand provenance"
    );
    let demand_relevant = implementations[0];
    assert!(matches!(
        demand_relevant.self_type(),
        model::Type::Definition { id, .. }
            if definition_name(graph, id) == Some("RelevantWrapper")
    ));
    assert_eq!(
        associated_names(graph, demand_relevant),
        BTreeSet::from(["Output"]),
        "the projection-selected associated type excludes its unrelated method sibling"
    );
    let relevance = demand_relevant
        .relevance()
        .filter_map(|id| definition_name(graph, id))
        .collect::<BTreeSet<_>>();
    assert!(relevance.contains("RelevantWrapper"));
    assert!(relevance.contains("relevant_projection"));
    assert!(relevance.contains("Output"));
    assert!(graph
        .definitions()
        .all(|definition| definition.display_path().last() != Some("UnrelatedWrapper")));

    let owner_bound = implementation(graph, "ImplOwnerBound");
    assert!(matches!(
        owner_bound.self_type(),
        model::Type::Definition { id, .. }
            if definition_name(graph, id) == Some("ImplOwnerSurface")
    ));
    assert_eq!(
        associated_names(graph, owner_bound),
        BTreeSet::from(["Evidence"])
    );
    let owner_relevance = owner_bound
        .relevance()
        .filter_map(|id| definition_name(graph, id))
        .collect::<BTreeSet<_>>();
    assert!(owner_relevance.contains("ImplOwnerSurface"));
    assert!(owner_relevance.contains("ImplOwnerReachability"));
    assert!(owner_relevance.contains("ImplOwnerBound"));
    assert!(owner_relevance.contains("Evidence"));

    let owner_surface = implementation(graph, "ImplOwnerReachability");
    assert_eq!(
        associated_names(graph, owner_surface),
        BTreeSet::new(),
        "a bound admits the trait implementation header without selecting a sibling method"
    );
    assert!(graph
        .definitions()
        .all(|definition| definition.display_path().last() != Some("PrivateImplOwner")));
    assert!(
        graph.implementations().all(|implementation| {
            graph
                .definitions()
                .all(|definition| definition.id() != implementation.id())
        }),
        "anonymous impl identities are never emitted as definitions"
    );

    let inherent = graph
        .implementations()
        .find(|implementation| {
            matches!(implementation.trait_ref(), model::Trait::Inherent)
                && matches!(implementation.self_type(), model::Type::Definition { id, .. }
                    if definition_name(graph, id) == Some("ImplSurface"))
        })
        .expect("the demanded public nominal retains its inherent implementation");
    assert_eq!(
        associated_names(graph, inherent),
        BTreeSet::from(["LEAF", "leaf"]),
        "the demanded nominal retains exactly its public inherent surface"
    );
    assert!(graph
        .definitions()
        .all(|definition| definition.display_path().last() != Some("hidden_impl_detail")));
}

fn opaque_outlives_bound<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    function_name: &str,
) -> (
    &'a rawr_import::api::identity::Definition,
    &'a model::Lifetime,
) {
    let output = function_output(graph, function_name);
    let model::Type::ImplTrait { id, bounds, .. } = output else {
        panic!("{function_name} retains its opaque output");
    };
    let (subject, lifetime) = bounds
        .iter()
        .find_map(|predicate| match predicate {
            model::Predicate::TypeOutlives { ty, lifetime } => Some((ty, lifetime)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{function_name} retains its outlives bound"));
    assert!(matches!(subject, model::Type::SelfType(owner) if owner == id));
    (id, lifetime)
}

fn assert_projected_opaque<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    function_name: &str,
) -> &'a rawr_import::api::identity::Definition {
    let function = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some(function_name))
        .unwrap_or_else(|| panic!("missing function {function_name}"));
    let model::Kind::Function { signature, .. } = function.kind() else {
        panic!("{function_name} must remain a function");
    };
    let model::Type::ImplTrait {
        id,
        bounds,
        captures,
    } = signature.output()
    else {
        panic!("{function_name} retains its opaque output");
    };
    let (projection, projected) = bounds
        .iter()
        .find_map(|predicate| match predicate {
            model::Predicate::ProjectionEquality { projection, ty } => Some((projection, ty)),
            _ => None,
        })
        .expect("the associated projection bound is retained");
    let model::Type::Projection { arguments, .. } = projection else {
        panic!("the associated bound retains projection identity");
    };
    assert!(arguments
        .iter()
        .any(|argument| matches!(argument, model::Argument::Type(model::Type::SelfType(owner)) if owner == id)));

    let model::Type::Reference {
        lifetime,
        ty: projected,
        ..
    } = projected
    else {
        panic!("the projected item retains its reference type");
    };
    assert!(matches!(lifetime, model::Lifetime::Early { owner, .. } if owner == id));
    let model::Type::Array { element, length } = projected.as_ref() else {
        panic!("the projected item retains its array type");
    };
    assert!(
        matches!(element.as_ref(), model::Type::Parameter { owner, .. } if owner == function.id())
    );
    assert!(matches!(length, model::Const::Parameter { owner, .. } if owner == function.id()));

    assert!(captures.iter().any(|argument| matches!(
        argument,
        model::Argument::Lifetime(model::Lifetime::Early { owner, .. }) if owner == function.id()
    )));
    assert!(captures.iter().any(|argument| matches!(
        argument,
        model::Argument::Type(model::Type::Parameter { owner, .. }) if owner == function.id()
    )));
    assert!(captures.iter().any(|argument| matches!(
        argument,
        model::Argument::Const(model::Const::Parameter { owner, .. }) if owner == function.id()
    )));
    assert!(!captures.iter().any(|argument| matches!(
        argument,
        model::Argument::Lifetime(model::Lifetime::Early { owner, .. }) if owner == id
    )));
    id
}

fn function_output<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    name: &str,
) -> &'a model::Type {
    let definition = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some(name))
        .unwrap_or_else(|| panic!("missing function {name}"));
    let model::Kind::Function { signature, .. } = definition.kind() else {
        panic!("{name} must remain a function");
    };
    signature.output()
}

fn contains_object_projection(ty: &model::Type) -> bool {
    match ty {
        model::Type::TraitObject { bounds, .. } => bounds
            .iter()
            .any(|bound| matches!(bound, model::ObjectBound::ProjectionEquality { .. })),
        model::Type::Definition { arguments, .. } => arguments.iter().any(|argument| {
            matches!(argument, model::Argument::Type(inner) if contains_object_projection(inner))
        }),
        model::Type::Reference { ty, .. }
        | model::Type::RawPointer { ty, .. }
        | model::Type::Slice(ty) => contains_object_projection(ty),
        model::Type::Array { element, .. } => contains_object_projection(element),
        model::Type::Tuple(elements) => elements.iter().any(contains_object_projection),
        model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::FunctionPointer(_)
        | model::Type::ImplTrait { .. }
        | model::Type::Projection { .. }
        | model::Type::Never => false,
    }
}

fn assert_declared_bounds(graph: &rawr_import::api::Graph<rawr_import::api::Extracted>) {
    let definition = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some("DeclaredBounds"))
        .expect("the declared-bounds fixture type is retained");
    let model::Kind::Struct { generics, .. } = definition.kind() else {
        panic!("DeclaredBounds must remain a struct");
    };
    let lifetime = generics
        .parameters()
        .find(|parameter| parameter.name() == "'a")
        .expect("the bounded lifetime parameter is retained");
    assert!(matches!(
        lifetime.kind(),
        model::ParameterKind::Lifetime { bounds } if bounds.is_empty()
    ));
    assert!(generics
        .predicates()
        .any(|predicate| matches!(predicate, model::Predicate::LifetimeOutlives { .. })));

    let inline = generics
        .parameters()
        .find(|parameter| parameter.name() == "T")
        .expect("the inline-bounded type parameter is retained");
    assert!(
        matches!(inline.kind(), model::ParameterKind::Type { bounds, .. }
        if bounds.iter().any(|bound| matches!(bound, model::Bound::MaybeSized)))
    );

    let where_parameter = generics
        .parameters()
        .find(|parameter| parameter.name() == "U")
        .expect("the where-bounded type parameter is retained");
    assert!(
        matches!(where_parameter.kind(), model::ParameterKind::Type { bounds, .. }
        if bounds.is_empty())
    );
    assert!(generics.predicates().any(|predicate| matches!(predicate,
        model::Predicate::Bound { bound, .. }
            if trait_bound_name(graph, bound) == Some("Copy"))));
    assert!(generics.predicates().any(|predicate| matches!(predicate,
        model::Predicate::Bound { bound, .. }
            if trait_bound_name(graph, bound) == Some("Clone"))));
}

fn implementation<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    trait_name: &str,
) -> &'a model::Implementation {
    graph
        .implementations()
        .find(|implementation| {
            matches!(implementation.trait_ref(), model::Trait::Trait { id, .. }
            if definition_name(graph, id) == Some(trait_name))
        })
        .unwrap_or_else(|| panic!("missing public implementation of {trait_name}"))
}

fn associated_names<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    implementation: &'a model::Implementation,
) -> BTreeSet<&'a str> {
    implementation
        .associated()
        .filter_map(|id| definition_name(graph, id))
        .collect()
}

fn trait_bound_name<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    bound: &'a model::Bound,
) -> Option<&'a str> {
    match bound {
        model::Bound::Trait { trait_id, .. } => definition_name(graph, trait_id),
        model::Bound::MaybeSized => None,
    }
}

fn definition_name<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    id: &rawr_import::api::identity::Definition,
) -> Option<&'a str> {
    graph
        .definitions()
        .find(|definition| definition.id() == id)
        .and_then(|definition| definition.display_path().last())
}

fn imported<P: rawr_import::api::Phase>(
    graph: &rawr_import::api::Graph<P>,
    alias: &str,
    path: &[&str],
) -> bool {
    graph.exports().any(|outcome| {
        matches!(outcome, ledger::Path::Imported(export)
            if export.key().root_alias() == alias && export.key().path().eq(path.iter().copied()))
    })
}

fn constant_graph() -> rawr_import::Result<rawr_import::api::Graph<rawr_import::api::Extracted>> {
    let root = std::env::temp_dir().join(format!(
        "rawr-constant-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("the unique constant project can be created");
    let project = Project(fs::canonicalize(root).expect("the constant project is canonical"));
    let limits = Limits::standard();
    let native = Native::pinned()?;
    constant_request(&project.0, &limits)
        .resolve(&native, &limits)
        .and_then(|resolution| rawr_import::extract::from_resolution(resolution, &native, &limits))
}

fn constant_request(root: &Path, limits: &Limits) -> rawr_import::cargo::Request {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the constant environment is sealable");
    let fixture = fixture("constant-api");
    let dependency = Dependency::path("constant_api", &fixture, limits)
        .and_then(|dependency| dependency.package("constant-api"))
        .expect("the constant fixture dependency is exact");
    Request::builder(root, &root.join(".rawr/cache"), limits)
        .expect("the constant request builder seals")
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .expect("the constant manifest policy fits the request")
        .dependency(dependency)
        .expect("the constant dependency fits the request")
        .build()
        .expect("the constant request seals")
}

fn request(root: &Path, limits: &Limits) -> rawr_import::cargo::Request {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the support environment is sealable");
    let fixture = fixture("facade-api");
    Request::builder(root, &root.join(".rawr/cache"), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("facade_a", &fixture, limits)
                .and_then(|dependency| dependency.package("facade-api"))
                .expect("the first facade alias is exact"),
        )
        .expect("the first facade dependency fits the request")
        .dependency(
            Dependency::path("facade_b", &fixture, limits)
                .and_then(|dependency| dependency.package("facade-api"))
                .expect("the second facade alias is exact"),
        )
        .expect("the second facade dependency fits the request")
        .build()
        .expect("the support request seals")
}

fn fixture(name: &str) -> PathBuf {
    fs::canonicalize(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("the support fixture exists")
}

fn fixture_digests() -> BTreeSet<(PathBuf, [u8; 32])> {
    [
        "compiler-helper/Cargo.toml",
        "compiler-helper/src/lib.rs",
        "support-api/Cargo.toml",
        "support-api/build.rs",
        "support-api/src/lib.rs",
        "external-api/Cargo.toml",
        "external-api/src/lib.rs",
        "facade-api/Cargo.toml",
        "facade-api/src/lib.rs",
    ]
    .into_iter()
    .map(|relative| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(relative);
        let bytes = fs::read(&path).expect("the support fixture is readable");
        (path, Sha256::digest(bytes).into())
    })
    .collect()
}
