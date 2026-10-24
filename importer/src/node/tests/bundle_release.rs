//! Sealed geometry at candidate-owned sites, retirement provenance, store-wide behaviour and
//! schema-4 migration. Allocation release is simulated through the store's
//! measurement seam rather than APFS timing.

use super::*;
use crate::store::bundle::release_tests::{measured, recorded};
use crate::store::bundle::geometry::seam;
use std::os::unix::fs::{symlink, PermissionsExt as _};

const COMPLETE: &[u8] = b"release completion";
const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

fn produce(
    attempt: crate::store::bundle::Attempt,
    inflate: &[(&str, u64)],
) -> Result<super::super::Bundled<BundleValue>, Error> {
    let io = |error: std::io::Error| Error::Transform(error.to_string().into());
    fs::write(attempt.generated().join("input"), b"generated").map_err(io)?;
    fs::write(attempt.generated().join("extra"), vec![0x65; 5000]).map_err(io)?;
    for (relative, bytes) in inflate {
        seam::set(&attempt.root().join(relative), *bytes);
    }
    let value = BundleValue::capture(attempt.reference());
    let pending = attempt.seal(Box::from(BundleValue::SEAL))?;
    super::super::Bundled::validate(value, pending, |value, pending| {
        value.validate_pending(pending)?;
        Ok(Box::from(COMPLETE))
    })
}

fn plain(
    attempt: crate::store::bundle::Attempt,
) -> Result<super::super::Bundled<BundleValue>, Error> {
    produce(attempt, &[])
}

/// Speculative preallocation at seal that the filesystem later releases.
fn inflated(
    attempt: crate::store::bundle::Attempt,
) -> Result<super::super::Bundled<BundleValue>, Error> {
    produce(attempt, &[("generated/extra", 256 * KIB)])
}

fn validate(value: &BundleValue, store: &crate::store::Store) -> Result<Box<[u8]>, Error> {
    value.validate_published(store)?;
    Ok(Box::from(COMPLETE))
}

struct Published {
    root: PathBuf,
    context: Context,
    store: crate::store::Store,
    value: super::super::Validated<BundleValue>,
    bundle: PathBuf,
}

fn publish(
    label: &str,
    execute: fn(crate::store::bundle::Attempt) -> Result<super::super::Bundled<BundleValue>, Error>,
) -> Published {
    seam::clear_all();
    let root = temporary(label);
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .discover::<BundleValue>(label.as_bytes(), 4096)
        .unwrap();
    let current = store.clone();
    let value = Evaluator::new(&store, &Counts::default())
        .discover_bundled(&recipe, move |value| validate(value, &current), execute)
        .unwrap();
    let bundle = store.root().join(value.value().reference.relative());
    Published {
        root,
        context,
        store,
        value,
        bundle,
    }
}

impl Published {
    fn commitment(&self) -> crate::store::bundle::BundleCommitment {
        crate::store::bundle::BundleCommitment::new(
            self.value.value().reference,
            self.value.value().seal_length,
            self.value.value().seal_digest,
        )
    }

    fn complete(&self) -> Result<(), Error> {
        let store = fixture_context(&self.root).open_store().unwrap();
        let (root, completion) = crate::store::bundle::Completion::authenticate(
            store.root(),
            self.commitment(),
            BundleValue::KIND,
            BundleValue::SCHEMA,
            self.value.content().digest(),
            store.graph_budget(),
        )?;
        completion.observe(&root, store.graph_budget(), |_, _| Ok(()))
    }

    fn validate_complete(&self) -> Result<Box<[u8]>, Error> {
        let store = fixture_context(&self.root).open_store().unwrap();
        self.value.value().validate_published(&store)
    }

    fn retire(self) {
        seam::clear_all();
        drop(self.value);
        self.store.retire_fixture().unwrap();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn thaw_tree(root: &Path) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        if metadata.is_dir() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            for entry in fs::read_dir(&path).unwrap() {
                pending.push(entry.unwrap().path());
            }
        } else if metadata.is_file() {
            let exec = metadata.permissions().mode() & 0o111;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644 | exec)).unwrap();
        }
    }
}

fn freeze_tree(root: &Path) {
    let mut directories = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).unwrap() {
                pending.push(entry.unwrap().path());
            }
            directories.push(path);
        } else if metadata.is_file() {
            let mode = if metadata.permissions().mode() & 0o111 != 0 {
                0o555
            } else {
                0o444
            };
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        }
    }
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
    }
}

fn set_len(path: &Path, length: u64) {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(length)
        .unwrap();
}

fn mutate(bundle: &Path, apply: impl FnOnce(&Path)) {
    thaw_tree(bundle);
    apply(bundle);
    freeze_tree(bundle);
}

fn fact(result: &Result<impl std::fmt::Debug, Error>) -> Option<String> {
    match result {
        Err(Error::CandidateCorrupt(corruption)) => {
            let debug = format!("{corruption:?}");
            ["Geometry", "Census", "Mode", "Seal", "Member"]
                .into_iter()
                .find(|fact| debug.contains(&format!("fact: {fact}")))
                .map(str::to_owned)
        }
        _ => None,
    }
}

#[test]
fn release_passes_candidate_sites_and_the_ledger_keeps_the_recorded_charge() {
    let published = publish("release-release", inflated);
    let sealed = recorded(&published.bundle);
    let ledger = published.store.fixture_ledger_total().unwrap();
    seam::clear_all();
    let released = measured(&published.bundle);
    assert_eq!((released.1, released.2), (sealed.1, sealed.2));
    assert!(released.0 < sealed.0, "the allocated component fell");

    published
        .validate_complete()
        .expect("validate_complete admits a release");
    published
        .complete()
        .expect("published Completion admits a release");
    assert_eq!(
        published.store.fixture_ledger_total().unwrap(),
        ledger,
        "the ledger keeps the recorded charge: no refund and no warm-path write"
    );
    assert_eq!(recorded(&published.bundle), sealed);

    // A warm lookup hits the released bundle: no rejection, no retirement.
    let store = fixture_context(&published.root).open_store().unwrap();
    let recipe = published
        .context
        .discover::<BundleValue>(b"release-release", 4096)
        .unwrap();
    let counts = Counts::default();
    let current = store.clone();
    let hit = Evaluator::new(&store, &counts)
        .discover_bundled(
            &recipe,
            move |value| validate(value, &current),
            |_| panic!("a released bundle must be reused"),
        )
        .unwrap();
    assert_eq!(hit.content().digest(), published.value.content().digest());
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 0);
    assert!(published.bundle.exists());
    assert_eq!(store.fixture_ledger_total().unwrap(), ledger);
    published.retire();
}

type Mutation = (&'static str, &'static str, fn(&Path));

fn candidate_matrix() -> Vec<Mutation> {
    vec![
        ("one-byte truncation", "Geometry", |b| {
            set_len(&b.join("generated/extra"), 4999)
        }),
        ("whole-block truncation", "Geometry", |b| {
            set_len(&b.join("generated/extra"), 4000)
        }),
        ("within-block truncation", "Geometry", |b| {
            set_len(&b.join("generated/extra"), 4097)
        }),
        ("extension", "Geometry", |b| {
            set_len(&b.join("generated/input"), 10)
        }),
        ("equal-total swap", "Geometry", |b| {
            set_len(&b.join("generated/input"), 10);
            set_len(&b.join("generated/extra"), 4999);
        }),
        ("rename", "Geometry", |b| {
            fs::rename(b.join("generated/input"), b.join("generated/input2")).unwrap()
        }),
        ("move", "Geometry", |b| {
            fs::rename(b.join("generated/input"), b.join("build/input")).unwrap()
        }),
        ("replacement", "Geometry", |b| {
            fs::remove_file(b.join("generated/input")).unwrap();
            fs::write(b.join("generated/input"), b"replaced!!").unwrap();
        }),
        ("added member", "Census", |b| {
            fs::write(b.join("generated/new"), b"new").unwrap()
        }),
        ("removed member", "Census", |b| {
            fs::remove_file(b.join("generated/input")).unwrap()
        }),
        ("zero-length truncation", "Geometry", |b| {
            set_len(&b.join("generated/input"), 0)
        }),
        ("sparse extension", "Geometry", |b| {
            set_len(&b.join("generated/extra"), 4 * MIB)
        }),
        ("file to directory", "Geometry", |b| {
            fs::remove_file(b.join("generated/input")).unwrap();
            fs::create_dir(b.join("generated/input")).unwrap();
        }),
        ("symlink", "Geometry", |b| {
            fs::remove_file(b.join("generated/input")).unwrap();
            symlink("extra", b.join("generated/input")).unwrap();
        }),
        ("hard link", "Geometry", |b| {
            fs::hard_link(b.join("generated/input"), b.join("generated/link")).unwrap()
        }),
        ("special file", "Geometry", |b| {
            fs::remove_file(b.join("generated/input")).unwrap();
            nix::unistd::mkfifo(&b.join("generated/input"), nix::sys::stat::Mode::S_IRUSR).unwrap();
        }),
    ]
}

#[test]
fn candidate_sites_reject_the_matrix_as_corruption_and_inventory_checks_placement() {
    let mut cases: Vec<(&str, &str, Box<dyn Fn(&Path)>)> = candidate_matrix()
        .into_iter()
        .map(|(name, fact, apply)| {
            let apply: Box<dyn Fn(&Path)> = Box::new(move |bundle: &Path| mutate(bundle, apply));
            (name, fact, apply)
        })
        .collect();
    cases.push((
        "mode change",
        "Geometry",
        Box::new(|bundle: &Path| {
            fs::set_permissions(
                bundle.join("generated/input"),
                fs::Permissions::from_mode(0o555),
            )
            .unwrap()
        }),
    ));
    cases.push((
        "writable member",
        "Mode",
        Box::new(|bundle: &Path| {
            fs::set_permissions(
                bundle.join("generated/input"),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap()
        }),
    ));
    cases.push((
        "allocation growth",
        "Census",
        Box::new(|bundle: &Path| seam::set(&bundle.join("generated/extra"), MIB)),
    ));
    for (name, expected, apply) in cases {
        let published = publish("release-candidate", plain);
        published.validate_complete().unwrap();
        apply(&published.bundle);
        let complete = published.validate_complete();
        assert_eq!(
            fact(&complete).as_deref(),
            Some(expected),
            "{name}: validate_complete must return CandidateCorrupt, got {complete:?}"
        );
        let completion = published.complete();
        assert_eq!(
            fact(&completion).as_deref(),
            Some(expected),
            "{name}: Completion must return CandidateCorrupt, got {completion:?}"
        );
        // The inventory checks a bundle's placement only; its consumers check
        // its tree.
        let inventory = published.store.fixture_ledger_total();
        assert!(
            inventory.is_ok(),
            "{name}: inventory checks placement only, got {inventory:?}"
        );
        if name != "symlink" && name != "special file" {
            seam::clear_all();
            thaw_tree(&published.bundle);
            freeze_tree(&published.bundle);
        }
        published.retire();
    }
}

#[test]
fn growth_hidden_by_another_members_release_is_geometry_corruption() {
    let published = publish("release-hidden-growth", |attempt| {
        produce(attempt, &[("generated/input", 256 * KIB)])
    });
    seam::clear_all();
    published.validate_complete().unwrap();
    let sealed = recorded(&published.bundle);
    mutate(&published.bundle, |b| {
        set_len(&b.join("generated/extra"), 5000 + 4096)
    });
    assert!(
        measured(&published.bundle).0 < sealed.0,
        "the total still fell"
    );
    let complete = published.validate_complete();
    assert_eq!(fact(&complete).as_deref(), Some("Geometry"), "{complete:?}");
    assert_eq!(fact(&published.complete()).as_deref(), Some("Geometry"));
    published.store.fixture_ledger_total().unwrap();
    published.retire();
}

#[test]
fn geometry_mismatch_drives_retirement_but_release_and_refusals_do_not() {
    // Geometry mismatch: positive retirement evidence, retired once and rebuilt.
    let published = publish("release-provenance", plain);
    mutate(&published.bundle, |b| {
        set_len(&b.join("generated/extra"), 4999)
    });
    let store = fixture_context(&published.root).open_store().unwrap();
    let recipe = published
        .context
        .discover::<BundleValue>(b"release-provenance", 4096)
        .unwrap();
    let counts = Counts::default();
    let current = store.clone();
    let repaired = Evaluator::new(&store, &counts)
        .discover_bundled(&recipe, move |value| validate(value, &current), plain)
        .unwrap();
    assert!(!published.bundle.exists(), "the drifted bundle is retired");
    assert_ne!(
        repaired.content().digest(),
        published.value.content().digest()
    );
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 1);
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).unwrap(),
        1
    );
    store
        .fixture_ledger_total()
        .expect("the repaired store inventories");
    published.retire();

    // Checked-arithmetic overflow is a refusal, never corruption evidence.
    let published = publish("release-overflow", plain);
    seam::set(&published.bundle.join("generated/extra"), u64::MAX);
    let complete = published.validate_complete();
    assert!(matches!(complete, Err(Error::Allocation)), "{complete:?}");
    assert!(matches!(published.complete(), Err(Error::Allocation)));
    seam::clear_all();
    assert!(published.bundle.exists());
    published.retire();
}

fn project_context(root: &Path, project: &str) -> Context {
    fs::create_dir(root.join(project)).unwrap();
    let cargo_home = fs::canonicalize(root.join("cargo-home")).unwrap();
    let environment =
        cargo::config::Environment::from_pairs([("CARGO_HOME", cargo_home.to_str().unwrap())])
            .unwrap();
    Context::capture(
        &root.join(project),
        environment,
        "test-target",
        &cargo::Limits::standard(),
    )
    .unwrap()
}

#[test]
fn one_released_bundle_among_several_admits_imports_enrollment_and_eviction() {
    let root = temporary("release-store-wide");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    seam::clear_all();
    let mut bundles = Vec::new();
    for (label, execute) in [
        (&b"release-wide-a"[..], inflated as fn(_) -> _),
        (b"release-wide-b", plain),
        (b"release-wide-c", plain),
    ] {
        let recipe = context.discover::<BundleValue>(label, 4096).unwrap();
        let current = store.clone();
        let value = Evaluator::new(&store, &Counts::default())
            .discover_bundled(&recipe, move |value| validate(value, &current), execute)
            .unwrap();
        bundles.push(store.root().join(value.value().reference.relative()));
    }
    seam::clear_all();
    assert!(measured(&bundles[0]).0 < recorded(&bundles[0]).0);

    // Store opens and inventories.
    let fresh = fixture_context(&root).open_store().unwrap();
    fresh.fixture_ledger_total().expect("the store inventories");
    // A new import publishes.
    let recipe = context
        .discover::<BundleValue>(b"release-wide-d", 4096)
        .unwrap();
    let current = fresh.clone();
    Evaluator::new(&fresh, &Counts::default())
        .discover_bundled(&recipe, move |value| validate(value, &current), plain)
        .expect("a new import succeeds beside a released bundle");
    // A previously unseen project enrolls and imports.
    let other = project_context(&root, "project-two");
    let other_store = other.open_store().unwrap();
    let recipe = other.discover::<BundleValue>(b"release-wide-e", 4096).unwrap();
    let current = other_store.clone();
    Evaluator::new(&other_store, &Counts::default())
        .discover_bundled(&recipe, move |value| validate(value, &current), plain)
        .expect("new-project enrollment succeeds beside a released bundle");
    // Eviction succeeds.
    let evict = fixture_context(&root).open_store().unwrap();
    assert!(evict.fixture_evict_one(false).expect("eviction succeeds"));
    evict.fixture_ledger_total().unwrap();
    store.retire_fixture().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn one_truncated_bundle_does_not_refuse_the_store() {
    let root = temporary("release-store-truncated");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    seam::clear_all();
    let mut bundles = Vec::new();
    for label in [&b"release-trunc-a"[..], b"release-trunc-b"] {
        let recipe = context.discover::<BundleValue>(label, 4096).unwrap();
        let current = store.clone();
        let value = Evaluator::new(&store, &Counts::default())
            .discover_bundled(&recipe, move |value| validate(value, &current), plain)
            .unwrap();
        bundles.push(store.root().join(value.value().reference.relative()));
    }
    // A within-block truncation leaves the charge identical.
    let before = measured(&bundles[0]);
    mutate(&bundles[0], |b| set_len(&b.join("generated/extra"), 4097));
    assert_eq!(measured(&bundles[0]), before);
    // The inventory checks placement only; the truncated bundle is its
    // consumers' to refuse, so an unrelated import proceeds.
    let fresh = fixture_context(&root).open_store().unwrap();
    fresh.fixture_ledger_total().unwrap();
    let recipe = context
        .discover::<BundleValue>(b"release-trunc-c", 4096)
        .unwrap();
    let current = fresh.clone();
    Evaluator::new(&fresh, &Counts::default())
        .discover_bundled(&recipe, move |value| validate(value, &current), plain)
        .expect("an unrelated import proceeds beside a truncated bundle");
    assert!(
        bundles.iter().all(|bundle| bundle.exists()),
        "an unrelated import deletes nothing"
    );
    store.retire_fixture().unwrap();
    let _ = fs::remove_dir_all(root);
}
