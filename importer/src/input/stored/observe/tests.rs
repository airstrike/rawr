use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{entry, Error};
use crate::cargo;
use crate::input::stored::{self, Budget};
use crate::input::{proof, Allocation, Usage};
use crate::node;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-stored-observe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("fixture directory");
        Self(fs::canonicalize(root).expect("canonical fixture directory"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn recipe(kind: u8, path: &Path) -> proof::Recipe {
    match kind {
        0 => proof::Recipe::File {
            role: proof::Role::DependencySource,
            package_id: Some("pkg".into()),
            path: path.into(),
        },
        1 => proof::Recipe::Path {
            role: proof::Role::DeclaredInput,
            package_id: None,
            path: path.into(),
        },
        2 => proof::Recipe::Directory {
            role: proof::Role::OutDirectory,
            package_id: Some("pkg".into()),
            path: path.into(),
        },
        _ => panic!("fixture recipe kind"),
    }
}

fn observe(recipe: &proof::Recipe, base: &Path) -> stored::Result<proof::Entry> {
    let limits = cargo::Limits::standard();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    entry(
        recipe.view(),
        base,
        &mut Budget::borrow(&limits, &mut allocation, &node::GraphBudget::new()),
    )
}

fn reference(recipe: proof::Recipe) -> proof::Entry {
    let limits = cargo::Limits::standard();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let proof = proof::evaluate(&[recipe], false, &limits, &mut allocation)
        .expect("legacy entry semantics");
    let (_, entries, _) = proof.into_stored_parts();
    entries[0].clone()
}

#[test]
fn real_file_path_and_complete_directory_entries_match_legacy() {
    let fixture = Fixture::new();
    let file = fixture.0.join("é.rs");
    fs::write(&file, b"one").expect("source fixture");
    fs::hard_link(&file, fixture.0.join("hard.rs")).expect("legacy-supported hard link");
    fs::create_dir(fixture.0.join("empty")).expect("empty directory");
    fs::create_dir(fixture.0.join("nested")).expect("nested directory");
    fs::write(fixture.0.join("nested/zero"), b"").expect("empty file");
    let missing = fixture.0.join("missing");
    for (kind, path) in [
        (0, &file),
        (1, &file),
        (1, &fixture.0),
        (1, &missing),
        (2, &fixture.0),
    ] {
        let actual = observe(&recipe(kind, path), &fixture.0).expect("current filesystem data");
        let expected = reference(recipe(kind, path));
        assert_eq!(
            serde_json::to_value(actual).expect("actual entry"),
            serde_json::to_value(expected).expect("legacy entry")
        );
    }
}

#[test]
fn relative_paths_use_only_the_explicit_owner_base_and_preserve_normalization() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("source"), b"x").expect("source");
    fs::create_dir(fixture.0.join("nested")).expect("nested");
    let actual = observe(&recipe(0, Path::new("nested/../source")), &fixture.0)
        .expect("bound relative source");
    let expected = reference(recipe(0, &fixture.0.join("source")));
    assert!(actual == expected);
    assert!(matches!(
        observe(&recipe(1, Path::new("source")), Path::new("relative")),
        Err(stored::Error::Observation(Error::Policy(_)))
    ));
    assert!(matches!(
        observe(&recipe(1, Path::new("/../source")), &fixture.0),
        Err(stored::Error::Observation(Error::Policy("path-escape")))
    ));
}

#[test]
fn content_membership_and_missing_type_drift_change_observation_data() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source");
    fs::write(&source, b"one").expect("source");
    let before = observe(&recipe(0, &source), &fixture.0).expect("before");
    fs::write(&source, b"two").expect("same-length change");
    assert!(before != observe(&recipe(0, &source), &fixture.0).expect("after"));
    let before = observe(&recipe(2, &fixture.0), &fixture.0).expect("directory before");
    fs::create_dir(fixture.0.join("empty")).expect("empty membership addition");
    assert!(before != observe(&recipe(2, &fixture.0), &fixture.0).expect("directory after"));
    fs::remove_file(&source).expect("remove source");
    assert!(matches!(
        observe(&recipe(1, &source), &fixture.0),
        Ok(proof::Entry::Missing { .. })
    ));
    fs::create_dir(&source).expect("replace by directory");
    assert!(matches!(
        observe(&recipe(1, &source), &fixture.0),
        Ok(proof::Entry::Directory { .. })
    ));
    assert!(matches!(
        observe(&recipe(0, &source), &fixture.0),
        Err(stored::Error::Observation(Error::Policy(_)))
    ));
}

#[test]
fn symlink_and_special_file_policy_remains_explicit() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source");
    fs::write(&source, b"one").expect("source");
    std::os::unix::fs::symlink(&source, fixture.0.join("link")).expect("symlink");
    for kind in [0, 1] {
        assert!(matches!(
            observe(&recipe(kind, &fixture.0.join("link")), &fixture.0),
            Err(stored::Error::Observation(Error::Policy(_)))
        ));
    }
    assert!(matches!(
        observe(&recipe(2, &fixture.0), &fixture.0),
        Err(stored::Error::Observation(Error::Policy("symlink-input")))
    ));
    assert!(matches!(
        observe(&recipe(1, Path::new("/dev/null")), &fixture.0),
        Err(stored::Error::Observation(Error::Policy(
            "unsupported-input-shape"
        )))
    ));
}

#[test]
fn every_other_recipe_rejects_without_filesystem_or_account_work() {
    let baseline = stored::tests::baseline();
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let prior = Usage {
        items: 7,
        bytes: 31,
    };
    let mut allocation = Allocation::for_proof(prior, &limits);
    for recipe in &baseline.recipes[3..] {
        assert!(matches!(
            entry(
                recipe.view(),
                Path::new("/"),
                &mut Budget::borrow(&limits, &mut allocation, &graph)
            ),
            Err(stored::Error::Observation(Error::Policy(
                "recipe is not a filesystem observation"
            )))
        ));
    }
    assert_eq!(allocation.usage(), prior);
}

#[test]
fn actual_directory_ownership_obeys_exact_prior_usage_and_terminal_refusal() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("source"), b"one").expect("source");
    fs::create_dir(fixture.0.join("empty")).expect("empty directory");
    let recipe = recipe(2, &fixture.0);
    let limits = cargo::Limits::standard();
    let prior = Usage {
        items: 7,
        bytes: 31,
    };
    let mut measured = Allocation::for_proof(prior, &limits);
    entry(
        recipe.view(),
        &fixture.0,
        &mut Budget::borrow(&limits, &mut measured, &node::GraphBudget::new()),
    )
    .expect("measured real ownership");
    let usage = measured.usage();
    let mut allocation = Allocation::for_proof(prior, &limits);
    let graph = node::GraphBudget::new();
    let result = entry(
        recipe.view(),
        &fixture.0,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    );
    // The directory's ownership is charged in full.
    result.expect("directory ownership is owned");
    assert_eq!(allocation.usage(), usage);
    // A genuine failure latches the account terminally.
    allocation.refuse();
    let spent = allocation.usage();
    assert!(matches!(
        entry(
            recipe.view(),
            &fixture.0,
            &mut Budget::borrow(&limits, &mut allocation, &node::GraphBudget::new())
        ),
        Err(stored::Error::Refused)
    ));
    assert_eq!(allocation.usage(), spent);
    assert!(matches!(
        allocation.bytes(0),
        Err(crate::Error::InputAccountRefused)
    ));
}

#[test]
fn file_graph_paths_and_hashing_are_exact_per_new_identity_and_metadata_storage_adds_no_decode() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source");
    fs::write(&source, b"one").expect("source");
    let recipe = recipe(1, &source);
    // File's direct view has eight admitted operations; use no package payload.
    let proof::Recipe::Path {
        role,
        package_id,
        path,
    } = recipe
    else {
        panic!("path fixture")
    };
    let recipe = proof::Recipe::File {
        role,
        package_id,
        path,
    };
    let length = source.as_os_str().len();
    assert!(length < 384, "fixture exercises stack CStr input path");
    // Path copies and the canonical output are owned once by the original
    // allocation; the graph's decode axis measures decoding, not copies.
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    entry(
        recipe.view(),
        &fixture.0,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("file entry");
}

#[test]
fn initial_refusal_prevents_io_and_read_length_changes_cannot_succeed() {
    let fixture = Fixture::new();
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let prior = Usage {
        items: 7,
        bytes: 31,
    };
    let mut allocation = Allocation::for_proof(prior, &limits);
    // A genuine failure latches the account; a latched account does no
    // filesystem work at all.
    allocation.refuse();
    assert!(matches!(
        entry(
            recipe(0, &fixture.0.join("absent")).view(),
            &fixture.0,
            &mut Budget::borrow(&limits, &mut allocation, &graph)
        ),
        Err(stored::Error::Refused)
    ));
    assert_eq!(allocation.usage(), prior);
    let source = fixture.0.join("source");
    fs::write(&source, b"one").expect("source");
    for expected in [0, 2, 4] {
        let mut allocation = Allocation::for_proof(prior, &limits);
        let graph = node::GraphBudget::new();
        let result = super::read::digest(
            &source,
            expected,
            &mut Budget::borrow(&limits, &mut allocation, &graph),
        );
        if expected < 3 {
            assert!(
                matches!(
                    result,
                    Err(stored::Error::Input(crate::Error::Limit {
                        resource: "freshness file",
                        limit,
                        actual: 3,
                    })) if limit == expected
                ),
                "expected {expected}"
            );
            assert!(matches!(
                allocation.bytes(0),
                Err(crate::Error::InputAccountRefused)
            ));
        } else {
            assert!(
                matches!(result, Err(stored::Error::Observation(Error::Drift(_)))),
                "expected {expected}"
            );
        }
    }
}

#[test]
fn reserved_entry_slot_is_not_charged_again_by_push() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let mut slots = super::slots::Slots::<u64>::new();
    slots.reserve(&mut budget).expect("next slot admitted");
    let before = budget.usage();
    slots.push(42, &mut budget).expect("reserved slot used");
    assert_eq!(budget.usage(), before);
    assert_eq!(slots.as_slice(), &[42]);
}

#[test]
fn file_refusal_latches_and_optional_config_rejects_directories() {
    let fixture = Fixture::new();
    let file = fixture.0.join("config");
    fs::write(&file, b"12345").expect("fixture input");
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let entry = super::file(proof::Role::Config, None, &file, &fixture.0, &mut budget)
        .expect("the file is observed");
    assert!(matches!(entry, proof::Entry::File { bytes: 5, .. }));
    // A genuine failure latches; the latched account observes nothing more.
    budget.refuse();
    assert!(matches!(
        super::file(proof::Role::Config, None, &file, &fixture.0, &mut budget),
        Err(stored::Error::Refused)
    ));
    assert!(matches!(budget.check(), Err(stored::Error::Refused)));

    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    assert!(matches!(
        super::optional_file(
            proof::Role::Config,
            None,
            &fixture.0,
            &fixture.0,
            &mut budget
        ),
        Err(stored::Error::Observation(Error::Policy(
            "config-input-shape"
        )))
    ));
}

#[test]
fn an_unchanged_freshness_file_is_hashed_once_and_a_rewrite_is_hashed_again() {
    let fixture = Fixture::new();
    let limits = cargo::Limits::standard();
    let source = fixture.0.join("memoized");
    fs::write(&source, b"first").expect("source");
    let digest = |expected: usize| {
        let mut allocation = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
        let graph = node::GraphBudget::new();
        super::read::digest(
            &source,
            expected,
            &mut Budget::borrow(&limits, &mut allocation, &graph),
        )
        .expect("the freshness file digests")
    };
    let first = digest(5);
    assert_eq!(digest(5), first, "a later consumer reuses the same digest");
    fs::write(&source, b"other").expect("same-length rewrite");
    let changed = digest(5);
    assert_ne!(
        changed, first,
        "a changed identity is read and hashed again"
    );
}

/// #520: a config candidate under a symlinked `.cargo` keeps the path Cargo
/// reads and is read through the link as it is now. Other inputs still refuse
/// symlinks.
#[cfg(unix)]
#[test]
fn config_candidates_follow_symlinks_and_other_inputs_refuse_them() {
    use sha2::{Digest as _, Sha256};
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let shared = fixture.0.join("shared");
    fs::create_dir(&shared).expect("shared configs");
    fs::write(shared.join("first.toml"), b"first").expect("first config");
    fs::write(shared.join("second.toml"), b"second!").expect("second config");
    fs::create_dir(fixture.0.join("real")).expect("real .cargo");
    symlink(
        shared.join("first.toml"),
        fixture.0.join("real/config.toml"),
    )
    .expect("file link");
    symlink(fixture.0.join("real"), fixture.0.join(".cargo")).expect("directory link");
    let spelled = fixture.0.join(".cargo/config.toml");

    let limits = cargo::Limits::standard();
    let observe = |role| {
        let graph = node::GraphBudget::new();
        let mut allocation = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let optional = super::optional_file(role, None, &spelled, &fixture.0, &mut budget);
        let path = entry::<()>(
            super::super::view::View::Path {
                role,
                package_id: None,
                path: &spelled,
            },
            &fixture.0,
            &mut budget,
        );
        (optional, path)
    };
    let expected = |content: &[u8]| proof::Entry::File {
        role: proof::Role::Config,
        package_id: None,
        path: spelled.to_str().unwrap().into(),
        bytes: content.len(),
        digest: Sha256::digest(content).into(),
    };
    let (optional, path) = observe(proof::Role::Config);
    assert!(optional.ok() == Some(expected(b"first")));
    assert!(path.ok() == Some(expected(b"first")));

    // Retargeting the file link changes the entry.
    fs::remove_file(fixture.0.join("real/config.toml")).expect("file link is removed");
    symlink(
        shared.join("second.toml"),
        fixture.0.join("real/config.toml"),
    )
    .expect("relink");
    let (optional, path) = observe(proof::Role::Config);
    assert!(optional.ok() == Some(expected(b"second!")));
    assert!(path.ok() == Some(expected(b"second!")));

    let (_, path) = observe(proof::Role::DeclaredInput);
    assert!(matches!(
        path,
        Err(stored::Error::Observation(Error::Policy("symlink-input")))
    ));
}
