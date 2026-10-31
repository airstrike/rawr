use std::path::{Path, PathBuf};

use super::{call, inventory, resolve, scope, Catalog};
use crate::api::model;
use crate::cargo::config::{Dependency, Environment};
use crate::cargo::{Limits, Request};
use crate::classify::representability::convert;
use crate::extract::Policy;
use crate::process::{test_launch, Native};

#[test]
fn independent_facts_prove_closed_obligations_without_any_child_process() {
    let _turn = crate::store::test_turn();
    crate::store::isolate_for_tests();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/tmp/independent-proof");
    std::fs::create_dir_all(&root).unwrap();
    let inventories = [
        capture(&root.join("ab"), &["a", "b"], &native, &limits),
        capture(&root.join("cb"), &["c", "b"], &native, &limits),
    ];
    for (name, inventory) in ["ab", "cb"].into_iter().zip(&inventories) {
        std::fs::write(
            root.join(format!("{name}-facts.json")),
            serde_json::to_vec_pretty(inventory.facts()).unwrap(),
        )
        .unwrap();
    }
    assert!(inventories[0].rejects_for_test(|facts| {
        facts.partitions[0].inherent = Box::new([]);
    }));
    assert!(inventories[0].rejects_for_test(|facts| {
        let id = facts.partitions[0].inherent[0].owner.clone();
        facts.partitions[0].unsupported = vec![id].into_boxed_slice();
    }));
    assert!(inventories[0].rejects_for_test(|facts| {
        facts.partitions[0].associated[0].declaration = None;
    }));
    let retained = inventories[0]
        .definitions()
        .find(|definition| {
            matches!(definition.kind(), model::Kind::Function { .. })
                && matches!(definition.owner(), model::Owner::Definition(_))
                && !inventories[0]
                    .graph()
                    .definitions()
                    .any(|ordinary| ordinary.id() == definition.id())
        })
        .expect("the independent facts retain an associated declaration outside the ordinary graph")
        .id()
        .clone();
    let original_inventory = *inventories[0].fingerprint();
    let original_graph = *inventories[0].graph().fingerprint().as_bytes();
    let original_catalog = *Catalog::join(&inventories).unwrap().fingerprint();
    let script = test_launch::Script::new(
        std::fs::canonicalize(&root).unwrap(),
        test_launch::FileLimits { count: 0, bytes: 0 },
        Vec::new(),
    )
    .unwrap();
    let original_proof = script
        .run(|| {
            let catalog = Catalog::join(&inventories).unwrap();
            let nominal = |root, name| {
                let id = catalog.export(root, &[name]).unwrap();
                model::Type::Definition {
                    id: catalog.identities[&id].clone(),
                    arguments: Box::new([]),
                }
            };
            let trait_id =
                |name| catalog.identities[&catalog.export("facts_c", &[name]).unwrap()].clone();
            let mut machine = resolve::Machine::over(&catalog);
            for (receiver, trait_name, accepted) in [
                ("LeftOnly", "Left", true),
                ("RightOnly", "Left", false),
                ("RightOnly", "Right", true),
                ("Neither", "Left", false),
                ("Both", "Left", true),
                ("Both", "Right", true),
                ("LeftOnly", "EqualityLeft", true),
                ("RightOnly", "EqualityLeft", false),
                ("RightOnly", "EqualityRight", true),
            ] {
                let result = machine.goal(
                    &trait_id(trait_name),
                    &[model::Argument::Type(nominal("facts_a", receiver))],
                    &resolve::State::default(),
                );
                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "{receiver}: {trait_name}: {result:?}"
                );
                if !accepted {
                    assert!(
                        matches!(result, Err(resolve::Failure::Absent)),
                        "{result:?}"
                    );
                }
            }
            for receiver in ["LeftOnly", "Alias"] {
                let extension = trait_id("Extension");
                let result = machine
                    .goal(
                        &extension,
                        &[
                            model::Argument::Type(nominal("facts_a", receiver)),
                            model::Argument::Type(model::Type::Parameter {
                                owner: extension.clone(),
                                index: 1,
                            }),
                        ],
                        &resolve::State::default(),
                    )
                    .unwrap();
                let arguments = result
                    .arguments(&extension)
                    .expect("the established relation retains closed substitutions");
                assert_eq!(
                    arguments[1],
                    model::Argument::Type(model::Type::Primitive("i64".into()))
                );
            }
            let assoc = catalog.identities[&catalog.export("facts_b", &["Assoc"]).unwrap()].clone();
            let associated = catalog
                .definitions
                .values()
                .find(|definition| {
                    matches!(definition.owner(), model::Owner::Definition(owner) if owner == &assoc)
                        && definition.display_path().last() == Some("Output")
                })
                .unwrap()
                .id()
                .clone();
            for (receiver, output) in [("LeftOnly", "i64"), ("RightOnly", "bool"), ("Alias", "i64")]
            {
                let mut state = resolve::State::default();
                let projected = machine
                    .normalize(
                        &model::Type::Projection {
                            trait_id: assoc.clone(),
                            associated: associated.clone(),
                            arguments: vec![model::Argument::Type(nominal("facts_a", receiver))]
                                .into_boxed_slice(),
                        },
                        &mut state,
                    )
                    .unwrap();
                assert_eq!(projected, model::Type::Primitive(output.into()));
            }
            let scoped = |traits: &[&str], lexical| {
                catalog
                    .scope(scope::Request {
                        source: [1; 32],
                        lexical: [lexical; 32],
                        edition: "2024".into(),
                        prelude: scope::Prelude::Active {
                            module: catalog.prelude("std", "2024").unwrap(),
                        },
                        traits: traits
                            .iter()
                            .map(|name| scope::Import {
                                definition: catalog.export("facts_c", &[name]).unwrap(),
                                path: vec!["facts_c".into(), (*name).into()].into_boxed_slice(),
                            })
                            .collect(),
                    })
                    .unwrap()
            };
            let named = |receiver: &str, name: &str| call::Request {
                receiver: convert::Type::Nominal {
                    definition: catalog.export("facts_a", &[receiver]).unwrap(),
                    arguments: Box::new([]),
                },
                mutable: false,
                lookup: call::Lookup::Method { name: name.into() },
                arguments: Box::new([]),
                type_arguments: Box::new([]),
            };
            let both = scoped(&["Left", "Right"], 2);
            for (receiver, output) in [
                ("LeftOnly", convert::Builtin::I64),
                ("RightOnly", convert::Builtin::Bool),
            ] {
                let proof = catalog.prove(named(receiver, "choose"), &both);
                let call::Outcome::Applicable(proof) = proof else {
                    panic!("{receiver}: {proof:?}");
                };
                assert_eq!(proof.output(), &convert::Type::Builtin(output));
            }
            assert!(matches!(
                catalog.prove(named("Both", "choose"), &both),
                call::Outcome::Ambiguous(_)
            ));
            assert!(matches!(
                catalog.prove(named("Neither", "choose"), &both),
                call::Outcome::NotApplicable(_)
            ));
            let same = scoped(&["LeftSame", "RightSame"], 3);
            assert!(matches!(
                catalog.prove(named("Both", "same"), &same),
                call::Outcome::Ambiguous(_)
            ));
            let left = scoped(&["Left"], 4);
            let right = scoped(&["Right"], 5);
            let call::Outcome::Applicable(left) = catalog.prove(named("Both", "choose"), &left)
            else {
                panic!("left scope");
            };
            let call::Outcome::Applicable(right) = catalog.prove(named("Both", "choose"), &right)
            else {
                panic!("right scope");
            };
            assert_ne!(left.fingerprint(), right.fingerprint());
            assert_ne!(left.output(), right.output());
            let mut explicit = named("Both", "choose");
            explicit.receiver = convert::Type::Borrowed {
                mutable: false,
                ty: Box::new(explicit.receiver),
            };
            explicit.lookup = call::Lookup::Ufcs {
                trait_id: catalog.export("facts_c", &["Left"]).unwrap(),
                path: vec!["facts_c".into(), "Left".into()].into_boxed_slice(),
                name: "choose".into(),
            };
            assert!(matches!(
                catalog.prove(explicit, &both),
                call::Outcome::Applicable(_)
            ));
            let projection = scoped(&["Projection", "Extension"], 6);
            for method in ["projected", "extension"] {
                let proof = catalog.prove(named("Alias", method), &projection);
                let call::Outcome::Applicable(proof) = proof else {
                    panic!("alias {method}: {proof:?}");
                };
                assert_eq!(
                    proof.output(),
                    &convert::Type::Builtin(convert::Builtin::I64)
                );
            }
            eprintln!(
                "INDEPENDENT_LOCAL_PROOF inventories=2 partitions=3 compiler_children=0 work={:?}",
                catalog.work()
            );
            chosen_fingerprint(&catalog)
        })
        .expect("composition has no permission to launch any child process");
    let [first, second] = inventories;
    let first = first
        .mutate_for_test(|facts| {
            let definition = facts
                .definitions
                .iter_mut()
                .find(|definition| definition.id() == &retained)
                .unwrap();
            let mut value = serde_json::to_value(&*definition).unwrap();
            value["kind"]["Function"]["signature"]["output"] =
                serde_json::json!({"Primitive":"bool"});
            *definition = serde_json::from_value(value).unwrap();
        })
        .unwrap();
    assert_eq!(first.graph().fingerprint().as_bytes(), &original_graph);
    assert_ne!(first.fingerprint(), &original_inventory);
    let mutated = [first, second];
    let mutated_catalog = Catalog::join(&mutated).unwrap();
    let mutated_proof = test_launch::Script::new(
        std::fs::canonicalize(&root).unwrap(),
        test_launch::FileLimits { count: 0, bytes: 0 },
        Vec::new(),
    )
    .unwrap()
    .run(|| chosen_fingerprint(&mutated_catalog))
    .unwrap();
    assert_ne!(
        mutated_proof, original_proof,
        "retained-only semantic changes invalidate the concrete call proof"
    );
    assert_ne!(
        mutated_catalog.fingerprint(),
        &original_catalog,
        "retained-only semantic facts participate in union invalidation"
    );
}

fn chosen_fingerprint(catalog: &Catalog<'_>) -> [u8; 32] {
    let scope = catalog
        .scope(scope::Request {
            source: [7; 32],
            lexical: [8; 32],
            edition: "2024".into(),
            prelude: scope::Prelude::Disabled,
            traits: vec![scope::Import {
                definition: catalog.export("facts_c", &["Left"]).unwrap(),
                path: vec!["facts_c".into(), "Left".into()].into_boxed_slice(),
            }],
        })
        .unwrap();
    let outcome = catalog.prove(
        call::Request {
            receiver: convert::Type::Nominal {
                definition: catalog.export("facts_a", &["LeftOnly"]).unwrap(),
                arguments: Box::new([]),
            },
            mutable: false,
            lookup: call::Lookup::Method {
                name: "choose".into(),
            },
            arguments: Box::new([]),
            type_arguments: Box::new([]),
        },
        &scope,
    );
    let call::Outcome::Applicable(proof) = outcome else {
        panic!("{outcome:?}");
    };
    *proof.fingerprint()
}

pub(super) fn capture(
    root: &Path,
    selected: &[&str],
    native: &Native,
    limits: &Limits,
) -> inventory::Acquired {
    std::fs::create_dir_all(root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let environment = Environment::capture()
        .unwrap()
        .setting("CARGO_NET_OFFLINE", "true")
        .unwrap();
    let mut builder = Request::builder(&root, &root.join("target/rawr/cache"), limits)
        .unwrap()
        .inventory()
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .unwrap();
    for name in selected {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("applicability-{name}"));
        builder = builder
            .dependency(
                Dependency::path(&format!("facts_{name}"), &path, limits)
                    .unwrap()
                    .package(&format!("facts-{name}"))
                    .unwrap(),
            )
            .unwrap();
    }
    inventory::Acquired::capture(builder.build().unwrap(), native, limits)
        .expect("the independent intrinsic domain authenticates")
}

#[test]
fn lowering_calls_prove_from_their_lexical_import_paths() {
    use crate::surface::application::Call;

    let _turn = crate::store::test_turn();
    crate::store::isolate_for_tests();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/tmp/lexical-calls");
    std::fs::create_dir_all(&root).unwrap();
    let inventories = [
        capture(&root.join("ab"), &["a", "b"], &native, &limits),
        capture(&root.join("cb"), &["c", "b"], &native, &limits),
    ];
    let script = test_launch::Script::new(
        std::fs::canonicalize(&root).unwrap(),
        test_launch::FileLimits { count: 0, bytes: 0 },
        Vec::new(),
    )
    .unwrap();
    script
        .run(|| {
            let catalog = Catalog::join(&inventories).unwrap();
            let call = |receiver: &str, scope: &[&[&str]]| {
                Call {
                    receiver: catalog.export("facts_a", &[receiver]).unwrap(),
                    receiver_arguments: Box::new([]),
                    name: "choose".into(),
                    mutable: false,
                    arguments: Box::new([]),
                    scope: scope
                        .iter()
                        .map(|path| path.iter().map(|segment| (*segment).into()).collect())
                        .collect(),
                }
                .canonical()
            };
            let left = catalog
                .prove_call(&call("Both", &[&["facts_c", "Left"]]), "2024")
                .unwrap();
            assert_eq!(
                left.signature().output(),
                &convert::Type::Builtin(convert::Builtin::I64)
            );
            let right = catalog
                .prove_call(&call("Both", &[&["facts_c", "Right"]]), "2024")
                .unwrap();
            assert_eq!(
                right.signature().output(),
                &convert::Type::Builtin(convert::Builtin::Bool)
            );
            assert_ne!(left.identity(), right.identity());
            assert_ne!(left.call().key(), right.call().key());
            // A type, a module path and the source's own items cannot
            // change method lookup; the proof keeps its selection.
            let wider = catalog
                .prove_call(
                    &call(
                        "Both",
                        &[
                            &["facts_c", "Left"],
                            &["facts_a", "Both"],
                            &["crate", "local"],
                        ],
                    ),
                    "2024",
                )
                .unwrap();
            assert_eq!(wider.signature().output(), left.signature().output());
            assert_ne!(wider.call().key(), left.call().key());
            // A glob brings every trait its module exports.
            assert!(matches!(
                catalog.prove_call(&call("Both", &[&["facts_c", "*"]]), "2024"),
                Err(super::Rejection::Ambiguous(_))
            ));
            let glob = catalog
                .prove_call(&call("LeftOnly", &[&["facts_c", "*"]]), "2024")
                .unwrap();
            assert_eq!(
                glob.signature().output(),
                &convert::Type::Builtin(convert::Builtin::I64)
            );
            assert!(matches!(
                catalog.prove_call(&call("Neither", &[&["facts_c", "*"]]), "2024"),
                Err(super::Rejection::NotApplicable)
            ));
            // An import from a crate without facts here may name a trait the
            // proof cannot see: undecided, never silently dropped.
            assert!(matches!(
                catalog.prove_call(
                    &call("Both", &[&["facts_c", "Left"], &["std", "fmt", "Write"]]),
                    "2024"
                ),
                Err(super::Rejection::Unknown(_))
            ));
        })
        .expect("proving lowering calls launches no child process");
}
