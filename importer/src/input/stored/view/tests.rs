use std::path::Path;

use super::View;
use crate::cargo;
use crate::input::stored::{self, Stored};
use crate::input::{package, proof, Allocation, Usage};
use crate::node;

#[derive(Debug, Eq, PartialEq)]
struct Borrow {
    pointer: *const u8,
    length: usize,
}

impl Borrow {
    fn text(text: &str) -> Self {
        Self {
            pointer: text.as_ptr(),
            length: text.len(),
        }
    }

    fn path(path: &Path) -> Self {
        let bytes = path.as_os_str().as_encoded_bytes();
        Self {
            pointer: bytes.as_ptr(),
            length: bytes.len(),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    tag: u8,
    role: Option<proof::Role>,
    scalar: usize,
    payloads: [Option<Borrow>; 6],
}

fn snapshot<Package>(view: View<'_, Package>) -> Snapshot {
    match view {
        View::Workspace { roots } => Snapshot {
            tag: 11,
            role: None,
            scalar: roots.len(),
            payloads: [
                Some(Borrow {
                    pointer: roots.as_ptr().cast(),
                    length: std::mem::size_of_val(roots),
                }),
                roots.first().map(|path| Borrow::path(path)),
                None,
                None,
                None,
                None,
            ],
        },
        View::File {
            role,
            package_id,
            path,
        } => Snapshot {
            tag: 0,
            role: Some(role),
            scalar: 0,
            payloads: [
                package_id.map(Borrow::text),
                Some(Borrow::path(path)),
                None,
                None,
                None,
                None,
            ],
        },
        View::Path {
            role,
            package_id,
            path,
        } => Snapshot {
            tag: 1,
            role: Some(role),
            scalar: 0,
            payloads: [
                package_id.map(Borrow::text),
                Some(Borrow::path(path)),
                None,
                None,
                None,
                None,
            ],
        },
        View::Directory {
            role,
            package_id,
            path,
        } => Snapshot {
            tag: 2,
            role: Some(role),
            scalar: 0,
            payloads: [
                package_id.map(Borrow::text),
                Some(Borrow::path(path)),
                None,
                None,
                None,
                None,
            ],
        },
        View::DepInfo {
            package_id,
            path,
            package_root,
            out_directories,
        } => {
            assert_eq!(
                out_directories.len(),
                2,
                "complete fixture has two OUT_DIR paths"
            );
            Snapshot {
                tag: 3,
                role: None,
                scalar: 0,
                payloads: [
                    Some(Borrow::text(package_id)),
                    Some(Borrow::path(path)),
                    Some(Borrow::path(package_root)),
                    Some(Borrow {
                        pointer: out_directories.as_ptr().cast(),
                        length: out_directories.len(),
                    }),
                    Some(Borrow::path(&out_directories[0])),
                    Some(Borrow::path(&out_directories[1])),
                ],
            }
        }
        View::SavedOutput {
            package_id,
            out_dir,
        } => Snapshot {
            tag: 4,
            role: None,
            scalar: 0,
            payloads: [
                Some(Borrow::text(package_id)),
                Some(Borrow::path(out_dir)),
                None,
                None,
                None,
                None,
            ],
        },
        // Live and retained package enums occupy different slots; their borrowed
        // referents are checked independently below, without a payload conversion.
        View::Package(_) => Snapshot {
            tag: 5,
            role: None,
            scalar: 0,
            payloads: [None, None, None, None, None, None],
        },
        View::Environment {
            package_id,
            execution,
            key,
            value,
        } => Snapshot {
            tag: 6,
            role: None,
            scalar: 0,
            payloads: [
                Some(Borrow::text(package_id)),
                Some(Borrow::text(execution)),
                Some(Borrow::text(key)),
                value.map(Borrow::text),
                None,
                None,
            ],
        },
        View::EnvironmentProjection { schema, key, value } => Snapshot {
            tag: 7,
            role: None,
            scalar: schema as usize,
            payloads: [
                Some(Borrow::text(key)),
                value.map(Borrow::text),
                None,
                None,
                None,
                None,
            ],
        },
        View::Fact {
            role,
            package_id,
            name,
            value,
        } => Snapshot {
            tag: 8,
            role: Some(role),
            scalar: 0,
            payloads: [
                package_id.map(Borrow::text),
                Some(Borrow::text(name)),
                Some(Borrow::text(value)),
                None,
                None,
                None,
            ],
        },
        View::BuildMessage {
            package_id,
            execution,
            name,
            value,
        } => Snapshot {
            tag: 9,
            role: None,
            scalar: 0,
            payloads: [
                Some(Borrow::text(package_id)),
                Some(Borrow::text(execution)),
                Some(Borrow::text(name)),
                Some(Borrow::text(value)),
                None,
                None,
            ],
        },
        View::Directive {
            package_id,
            execution,
            index,
            value,
        } => Snapshot {
            tag: 10,
            role: None,
            scalar: index,
            payloads: [
                Some(Borrow::text(package_id)),
                Some(Borrow::text(execution)),
                Some(Borrow::text(value)),
                None,
                None,
                None,
            ],
        },
    }
}

#[test]
fn every_recipe_borrows_the_same_payloads_after_consuming_capture() {
    let limits = cargo::Limits::standard();
    for alternate_options in [false, true] {
        let mut baseline = stored::tests::baseline();
        if alternate_options {
            let proof::Recipe::Environment { value, .. } = &mut baseline.recipes[6] else {
                panic!("environment fixture")
            };
            *value = None;
            let proof::Recipe::EnvironmentProjection { value, .. } = &mut baseline.recipes[7]
            else {
                panic!("projection fixture")
            };
            *value = Some("é\n".into());
            let proof::Recipe::Fact { package_id, .. } = &mut baseline.recipes[8] else {
                panic!("fact fixture")
            };
            *package_id = Some("owner".into());
        }
        assert_eq!(baseline.recipes.len(), 12);
        let before: [Snapshot; 12] =
            std::array::from_fn(|index| snapshot(baseline.recipes[index].view()));
        let proof::Recipe::Package(owner) = &baseline.recipes[5] else {
            panic!("live package fixture")
        };
        let View::Package(borrowed) = baseline.recipes[5].view() else {
            panic!("live package view")
        };
        assert!(std::ptr::eq(owner, borrowed));
        assert!(matches!(borrowed, package::Set::Registry(_)));
        let prior = baseline.allocation.usage();
        let (value, allocation) =
            Stored::capture(baseline, &limits, &node::GraphBudget::new()).into_parts();
        let value = value.expect("complete retained data");
        for (index, recipe) in value.recipes().iter().enumerate() {
            assert_eq!(before[index].tag, index as u8);
            assert_eq!(before[index], snapshot(recipe.view()));
        }
        let stored::Recipe::Package(owner) = &value.recipes()[5] else {
            panic!("stored package fixture")
        };
        let View::Package(borrowed) = value.recipes()[5].view() else {
            panic!("stored package view")
        };
        assert!(std::ptr::eq(owner, borrowed));
        let stored::Package::Registry(registry) = borrowed else {
            panic!("stored registry fixture")
        };
        assert_eq!(registry.package_id.as_ref(), "registry+fixture#pkg@1.2.3");
        assert_eq!(registry.source.as_ref(), "registry+fixture");
        assert_eq!(registry.checksum, [7; 32]);
        assert_eq!(registry.marker_digest, [8; 32]);
        assert_eq!(
            allocation.usage(),
            Usage {
                items: prior.items + 12,
                bytes: prior.bytes + 12 * std::mem::size_of::<stored::Recipe>(),
            }
        );
    }
}

#[test]
fn views_have_no_owned_drop_or_account_effect_even_after_refusal() {
    assert!(!std::mem::needs_drop::<View<'_, package::Set>>());
    assert!(!std::mem::needs_drop::<View<'_, stored::Package>>());
    let limits = cargo::Limits::standard();
    let mut baseline = stored::tests::baseline();
    let prior = baseline.allocation.usage();
    baseline.allocation.byte_limit = prior.bytes;
    // One byte over the target is charged and recorded, never refused.
    baseline
        .allocation
        .bytes(1)
        .expect("one over the byte target is recorded");
    let prior = baseline.allocation.usage();
    // A genuine failure latches the account; views still cost nothing.
    baseline.allocation.refuse();
    for recipe in &baseline.recipes {
        assert_eq!(snapshot(recipe.view()), snapshot(recipe.view()));
    }
    assert_eq!(baseline.allocation.usage(), prior);
    assert!(matches!(
        baseline.allocation.bytes(0),
        Err(crate::Error::InputAccountRefused)
    ));

    let (stored, mut allocation) = Stored::capture(
        stored::tests::baseline(),
        &limits,
        &node::GraphBudget::new(),
    )
    .into_parts();
    let stored = stored.expect("stored fixture");
    let prior = allocation.usage();
    allocation.item_limit = prior.items;
    allocation
        .items(1)
        .expect("one over the item target is recorded");
    let prior = allocation.usage();
    allocation.refuse();
    for recipe in stored.recipes() {
        assert_eq!(snapshot(recipe.view()), snapshot(recipe.view()));
    }
    assert_eq!(allocation.usage(), prior);
    assert!(matches!(
        allocation.items(0),
        Err(crate::Error::InputAccountRefused)
    ));
}

#[test]
fn a_live_package_tree_is_stored_as_its_package_and_root() {
    let recipe = proof::Recipe::Package(package::Set::fixture_stored_tree());
    let proof::Recipe::Package(owner) = &recipe else {
        panic!("live package")
    };
    let View::Package(borrowed) = recipe.view() else {
        panic!("package view")
    };
    assert!(std::ptr::eq(owner, borrowed));
    let Ok(stored::Recipe::Package(stored::Package::Tree { package_id, root })) =
        stored::Recipe::capture(recipe)
    else {
        panic!("a stored package tree")
    };
    assert_eq!(&*package_id, "path+file:///unobserved/package#pkg@1.2.3");
    assert_eq!(root, Path::new("/unobserved/package"));
}

fn scalar_entries() -> Box<[proof::Entry]> {
    let mut entries = vec![
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
            role: proof::Role::Profile,
            package_id: None,
            name: "profile".into(),
            value: "dev".into(),
        },
        proof::Entry::BuildMessage {
            package_id: "pkg".into(),
            execution: "out".into(),
            name: "cfg".into(),
            source: None,
            value: "feature=\"value\"".into(),
        },
        proof::Entry::Directive {
            package_id: "pkg".into(),
            execution: "out".into(),
            index: 4,
            value: "cargo::rerun-if-changed=src".into(),
        },
    ];
    entries.sort_unstable();
    entries.into_boxed_slice()
}

#[test]
fn live_evaluation_preserves_entries_fingerprint_and_exact_prior_accounting() {
    let baseline = stored::tests::baseline();
    let limits = cargo::Limits::standard();
    let prior = Usage {
        items: 7,
        bytes: 31,
    };
    let mut allocation = Allocation::for_proof(prior, &limits);
    allocation.item_limit = prior.items + 19;
    allocation.byte_limit = prior.bytes + 82;
    let actual = proof::evaluate(&baseline.recipes[6..11], &limits, &mut allocation)
        .expect("exact remaining scalar proof allowance");
    assert_eq!(
        allocation.usage(),
        Usage {
            items: 26,
            bytes: 113
        }
    );
    let expected = proof::Proof::fixture_stored(scalar_entries());
    assert_eq!(actual.fingerprint(), expected.fingerprint());
    assert!(actual.stored_parts().1 == expected.stored_parts().1);
}

/// Live evaluation is owned in full on top of prior usage and never
/// latches; a genuine failure then latches the account terminally.
#[test]
fn live_evaluation_charges_in_full_and_keeps_refusal_terminal() {
    let baseline = stored::tests::baseline();
    let limits = cargo::Limits::standard();
    let prior = Usage {
        items: 7,
        bytes: 31,
    };
    let mut allocation = Allocation::for_proof(prior, &limits);
    proof::evaluate(&baseline.recipes[6..11], &limits, &mut allocation)
        .expect("live evaluation is owned, not gated");
    let spent = allocation.usage();
    assert_eq!(
        spent,
        Usage {
            items: 26,
            bytes: 113
        }
    );
    assert!(allocation.bytes(0).is_ok(), "a charge never latches");
    allocation.refuse();
    assert!(matches!(
        proof::evaluate(&baseline.recipes[6..7], &limits, &mut allocation),
        Err(crate::Error::InputAccountRefused)
    ));
    assert_eq!(allocation.usage(), spent);
}
