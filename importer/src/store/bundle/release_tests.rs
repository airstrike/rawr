//! Sealed bundles may release allocated blocks, never logical geometry.
//!
//! Allocation release is simulated through the store's measurement seam
//! (`geometry::seam`): a file reports extra allocated blocks when the seal is
//! measured, and the seam is cleared afterwards, exactly like APFS releasing
//! speculative preallocation after the writer closed the file.

use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::Sha256;

use super::geometry::seam;
use super::*;

static FIXTURE: AtomicU64 = AtomicU64::new(1);
const SEAL: &[u8] = b"release-seal";
const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const EXECUTIONS: u8 = 8;

/// Recorded (bytes, allocations, members) and whether the record is legacy.
pub(crate) fn recorded(root: &Path) -> ((u64, u64, u64), bool) {
    let record = Record::read_bundle(root).expect("the bundle record decodes");
    let charge = record.charge();
    (
        (charge.bytes, charge.allocations, charge.members),
        record.is_legacy_bundle(),
    )
}

/// The current measured (bytes, allocations, members) of a published bundle.
pub(crate) fn measured(root: &Path) -> (u64, u64, u64) {
    let charge = measure(root, false).expect("the bundle measures");
    (charge.bytes, charge.allocations, charge.members)
}

/// Every bundle root in a store.
pub(crate) fn bundle_roots(store: &Path) -> Vec<PathBuf> {
    bundle_paths(store).expect("the store bundles enumerate")
}

struct Fixture {
    root: PathBuf,
    store: super::super::Store,
    next: u8,
}

impl Fixture {
    fn new(label: &str) -> Self {
        seam::clear_all();
        seam::legacy_writer(false);
        let root = std::env::temp_dir().join(format!(
            "rawr-bundle-release-{label}-{}-{}",
            std::process::id(),
            FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).expect("the fixture root creates");
        let store =
            super::super::Store::open(&root, [0x38; 32]).expect("the fixture store opens");
        for value in 1..EXECUTIONS {
            let execution = ExecutionKey::from_bytes([value; 32]);
            super::super::lease::with_publication(
                store.root(),
                store.graph_budget(),
                |publication| prepare_execution(publication, store.root(), execution),
            )
            .expect("the execution parent is admitted");
            super::super::lease::with_execution(
                store.root(),
                store.graph_budget(),
                execution,
                || Ok(()),
            )
            .expect("the execution lock is admitted");
        }
        Self {
            root,
            store,
            next: 1,
        }
    }

    fn execution(&mut self) -> ExecutionKey {
        let execution = ExecutionKey::from_bytes([self.next; 32]);
        self.next += 1;
        assert!(
            self.next <= EXECUTIONS,
            "the fixture prepared enough executions"
        );
        execution
    }

    /// Allocate, populate, inflate the named members' allocation, and seal.
    fn sealed(&mut self, populate: fn(&Attempt), inflate: &[(&str, u64)]) -> (PathBuf, Pending) {
        let execution = self.execution();
        let attempt = Attempt::allocate(
            self.store.root(),
            execution,
            Budget {
                charge: Charge {
                    bytes: 64 * MIB,
                    allocations: 1,
                    members: 256,
                },
            },
        )
        .expect("the attempt allocates");
        let root = attempt.root();
        populate(&attempt);
        for (relative, bytes) in inflate {
            seam::set(&root.join(relative), *bytes);
        }
        let pending = attempt
            .seal(Box::from(SEAL))
            .expect("the attempt seals");
        (root, pending)
    }

    fn published(&mut self, inflate: &[(&str, u64)]) -> (PathBuf, Record) {
        let (root, mut pending) = self.sealed(populate, inflate);
        let key = Digest::from_bytes([self.next; 32]);
        let execution = pending.reference.execution();
        let record = pending
            .publish(
                Kind::Runner,
                2,
                key,
                LeaseKey::Execution(execution),
                Digest::from_bytes([0x5a; 32]),
            )
            .expect("the bundle publishes");
        pending.retain();
        (root, record)
    }

    fn retire(self) {
        seam::clear_all();
        seam::legacy_writer(false);
        drop(self.store);
        force_remove(&self.root);
    }
}

/// Generated outputs covering ordinary, block-aligned, zero-length, sparse,
/// executable and nested members.
fn populate(attempt: &Attempt) {
    let generated = attempt.generated();
    fs::write(generated.join("a"), [0x61; 10]).unwrap();
    fs::write(generated.join("b"), [0x62; 5000]).unwrap();
    fs::create_dir(generated.join("sub")).unwrap();
    fs::write(generated.join("sub/c"), [0x63; 4096]).unwrap();
    fs::write(attempt.build().join("zero"), []).unwrap();
    let sparse = File::create(attempt.target().join("sparse")).unwrap();
    sparse.set_len(MIB).unwrap();
    drop(sparse);
    fs::write(attempt.target().join("exe"), b"#!/bin/true").unwrap();
    fs::set_permissions(
        attempt.target().join("exe"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
}

/// The same tree created in a different order, through different handles.
fn populate_reversed(attempt: &Attempt) {
    fs::write(attempt.target().join("exe"), b"#!/bin/true").unwrap();
    fs::set_permissions(
        attempt.target().join("exe"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let sparse = File::create(attempt.target().join("sparse")).unwrap();
    sparse.set_len(MIB).unwrap();
    drop(sparse);
    fs::write(attempt.build().join("zero"), []).unwrap();
    let generated = attempt.generated();
    fs::create_dir(generated.join("sub")).unwrap();
    fs::write(generated.join("sub/c"), [0x63; 4096]).unwrap();
    fs::write(generated.join("b"), [0x62; 5000]).unwrap();
    fs::write(generated.join("a"), [0x61; 10]).unwrap();
}

fn populate_one_byte_longer(attempt: &Attempt) {
    populate(attempt);
    fs::write(attempt.generated().join("a"), [0x61; 11]).unwrap();
}

fn force_remove(path: &Path) {
    let mut pending = vec![path.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
            if let Ok(entries) = fs::read_dir(&path) {
                pending.extend(entries.flatten().map(|entry| entry.path()));
            }
        }
    }
    let _ = fs::remove_dir_all(path);
}

fn set_len(path: &Path, length: u64) {
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(length)
        .unwrap();
}

/// One geometry drift. `apply` runs on a thawed tree before it is refrozen;
/// `after` runs on the refrozen tree (mode changes and allocation growth).
struct Case {
    name: &'static str,
    inflate: &'static [(&'static str, u64)],
    apply: fn(&Path, &Path),
    after: fn(&Path),
}

const NOTHING: fn(&Path) = |_| {};

fn matrix() -> Vec<Case> {
    vec![
        Case {
            name: "one-byte truncation (within its block)",
            inflate: &[],
            apply: |root, _| set_len(&root.join("generated/b"), 4999),
            after: NOTHING,
        },
        Case {
            name: "whole-block truncation",
            inflate: &[],
            apply: |root, _| set_len(&root.join("generated/b"), 4000),
            after: NOTHING,
        },
        Case {
            name: "within-block truncation invisible to the charge",
            inflate: &[],
            apply: |root, _| set_len(&root.join("generated/b"), 4097),
            after: NOTHING,
        },
        Case {
            name: "one-byte extension",
            inflate: &[],
            apply: |root, _| set_len(&root.join("generated/a"), 11),
            after: NOTHING,
        },
        Case {
            name: "zero-length file extended",
            inflate: &[],
            apply: |root, _| set_len(&root.join("build/zero"), 1),
            after: NOTHING,
        },
        Case {
            name: "file truncated to zero length",
            inflate: &[],
            apply: |root, _| set_len(&root.join("generated/a"), 0),
            after: NOTHING,
        },
        Case {
            name: "sparse file truncated by one byte",
            inflate: &[],
            apply: |root, _| set_len(&root.join("target/sparse"), MIB - 1),
            after: NOTHING,
        },
        Case {
            name: "sparse file logically extended",
            inflate: &[],
            apply: |root, _| set_len(&root.join("target/sparse"), 2 * MIB),
            after: NOTHING,
        },
        Case {
            name: "equal-total compensating swap",
            inflate: &[],
            apply: |root, _| {
                set_len(&root.join("generated/a"), 11);
                set_len(&root.join("generated/b"), 4999);
            },
            after: NOTHING,
        },
        Case {
            name: "same-count rename",
            inflate: &[],
            apply: |root, _| {
                fs::rename(root.join("generated/a"), root.join("generated/a2")).unwrap()
            },
            after: NOTHING,
        },
        Case {
            name: "same-count move between partitions",
            inflate: &[],
            apply: |root, _| {
                fs::rename(root.join("generated/sub/c"), root.join("build/c")).unwrap()
            },
            after: NOTHING,
        },
        Case {
            name: "same-path replacement by a different file",
            inflate: &[],
            apply: |root, _| {
                fs::remove_file(root.join("generated/a")).unwrap();
                fs::write(root.join("generated/a"), [0x41; 12]).unwrap();
            },
            after: NOTHING,
        },
        Case {
            name: "growth hidden by another member's release",
            inflate: &[("generated/a", 64 * KIB)],
            apply: |root, _| set_len(&root.join("generated/b"), 5000 + 4096),
            after: NOTHING,
        },
        Case {
            name: "added member",
            inflate: &[],
            apply: |root, _| fs::write(root.join("generated/new"), b"new").unwrap(),
            after: NOTHING,
        },
        Case {
            name: "added directory",
            inflate: &[],
            apply: |root, _| fs::create_dir(root.join("declared/new")).unwrap(),
            after: NOTHING,
        },
        Case {
            name: "removed member",
            inflate: &[],
            apply: |root, _| fs::remove_file(root.join("generated/a")).unwrap(),
            after: NOTHING,
        },
        Case {
            name: "file replaced by a directory",
            inflate: &[],
            apply: |root, _| {
                fs::remove_file(root.join("generated/a")).unwrap();
                fs::create_dir(root.join("generated/a")).unwrap();
            },
            after: NOTHING,
        },
        Case {
            name: "file replaced by a symlink",
            inflate: &[],
            apply: |root, _| {
                fs::remove_file(root.join("generated/a")).unwrap();
                symlink("b", root.join("generated/a")).unwrap();
            },
            after: NOTHING,
        },
        Case {
            name: "added internal hard link",
            inflate: &[],
            apply: |root, _| {
                fs::hard_link(root.join("generated/a"), root.join("generated/a-link")).unwrap()
            },
            after: NOTHING,
        },
        Case {
            name: "external hard link at the same count",
            inflate: &[],
            apply: |root, fixture| {
                fs::hard_link(root.join("generated/a"), fixture.join("outside-link")).unwrap()
            },
            after: NOTHING,
        },
        Case {
            name: "special file",
            inflate: &[],
            apply: |root, _| {
                fs::remove_file(root.join("generated/a")).unwrap();
                nix::unistd::mkfifo(&root.join("generated/a"), nix::sys::stat::Mode::S_IRUSR)
                    .unwrap();
            },
            after: NOTHING,
        },
        Case {
            name: "mode change (executable bit added)",
            inflate: &[],
            apply: |_, _| {},
            after: |root| {
                fs::set_permissions(root.join("generated/a"), fs::Permissions::from_mode(0o555))
                    .unwrap()
            },
        },
        Case {
            name: "mode change (member made writable)",
            inflate: &[],
            apply: |_, _| {},
            after: |root| {
                fs::set_permissions(root.join("generated/a"), fs::Permissions::from_mode(0o644))
                    .unwrap()
            },
        },
        Case {
            name: "allocation growth above the recorded charge",
            inflate: &[],
            apply: |_, _| {},
            after: |root| seam::set(&root.join("generated/b"), 64 * KIB),
        },
    ]
}

fn mutate(fixture: &Path, root: &Path, case: &Case) {
    thaw(root).expect("the fixture thaws the bundle");
    (case.apply)(root, fixture);
    refreeze(root);
    (case.after)(root);
}

/// Restore frozen modes without following or rejecting unusual members.
fn refreeze(root: &Path) {
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
            fs::set_permissions(
                &path,
                fs::Permissions::from_mode(frozen_mode(&metadata, false)),
            )
            .unwrap();
        }
    }
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
    }
}

#[test]
fn release_with_identical_geometry_passes_every_store_site() {
    let mut fixture = Fixture::new("release");
    let inflate = [
        ("generated/a", 64 * KIB),
        ("generated/b", 128 * KIB),
        ("target/sparse", 2 * MIB),
    ];
    let (root, mut pending) = fixture.sealed(populate, &inflate);
    let sealed = pending.charge;
    let reference = pending.reference;
    let digest: [u8; 32] = Sha256::digest(SEAL).into();
    seam::clear_all();
    let released = measure(&root, true).unwrap();
    assert_eq!(released.members, sealed.members);
    assert!(
        released.bytes < sealed.bytes,
        "the seam models a real release"
    );

    // Pending seal recheck and complete-scope recheck.
    pending
        .validate_unpublished(reference, SEAL.len(), digest)
        .expect("the pending seal recheck admits a release");
    let graph = crate::node::GraphBudget::new();
    pending
        .validate_unpublished_budgeted(reference, SEAL.len(), digest, &graph)
        .expect("the complete-scope pending recheck admits a release");
    // Pending Completion.
    let commitment = BundleCommitment::new(reference, SEAL.len(), digest);
    let (_, completion) = pending.complete_replay(commitment, &graph).unwrap();
    completion
        .observe(&root, &graph, |_, _| Ok(()))
        .expect("the pending completion admits a release");

    // Publication runs Record::validate_bundle on the post-freeze walk.
    let record = pending
        .publish(
            Kind::Runner,
            2,
            Digest::from_bytes([0x71; 32]),
            LeaseKey::Execution(reference.execution()),
            Digest::from_bytes([0x72; 32]),
        )
        .expect("publication admits a release between seal and publication");
    pending.retain();
    assert_eq!(
        record.charge(),
        sealed,
        "the seal charge is recorded unchanged"
    );
    assert!(
        record.sealed_geometry().is_some(),
        "a schema-5 record is written"
    );
    assert_eq!(
        fs::metadata(root.join("allocation")).unwrap().len(),
        accounting::BUNDLE_RECORD_BYTES as u64
    );

    // Registered inventory and retirement.
    let measured = validate_physical(&root, record.allocation())
        .expect("inventory admits a released allocation");
    assert_eq!(measured.members, sealed.members);
    assert!(measured.bytes < record.charge().bytes);
    validate_physical_retiring(&root, record.allocation())
        .expect("the retiring check admits a released allocation");
    thaw_root(&root).unwrap();
    validate_physical_retiring(&root, record.allocation())
        .expect("the thawed-root retiring check admits a released allocation");
    assert_eq!(
        Record::read_bundle(&root).unwrap().charge(),
        sealed,
        "nothing re-records the charge downward"
    );

    // Repair::validate.
    freeze(&root).unwrap();
    let mut recovery = super::super::recovery::Budget::new();
    let eviction = prepare_eviction_at(root.clone(), record.allocation(), &mut recovery)
        .unwrap()
        .unwrap();
    let repair = eviction
        .into_repair(record.allocation(), &mut recovery)
        .unwrap();
    let excess = repair
        .validate(&root, record.allocation())
        .expect("repair admits a released allocation");
    assert_eq!(excess.bytes, 0, "a release is never an excess");
    drop(repair);
    fixture.retire();
}

#[test]
fn release_never_falls_below_a_files_rounded_logical_length() {
    let mut fixture = Fixture::new("rounding");
    // 4096 bytes is exactly one block; an inflated report of one more byte
    // rounds to two blocks at seal.
    let (root, record) = fixture.published(&[("generated/sub/c", 4097), ("generated/b", 3 * 4096)]);
    seam::clear_all();
    // The filesystem now reports no allocation at all for these files: the
    // per-file charge stays at its rounded logical length.
    seam::set(&root.join("generated/sub/c"), 0);
    seam::set(&root.join("generated/b"), 0);
    seam::set(&root.join("generated/a"), 0);
    let measured = validate_physical(&root, record.allocation())
        .expect("allocation released below the logical length is still accepted");
    let floor = measure(&root, false).unwrap();
    assert_eq!(measured, floor);
    seam::clear_all();
    let ordinary = measure(&root, false).unwrap();
    assert!(
        floor.bytes <= ordinary.bytes,
        "the logical floor never undercuts an ordinary measurement"
    );
    // Overflow and limit edges are refusals, never acceptance.
    seam::set(&root.join("generated/b"), u64::MAX);
    assert!(matches!(
        validate_physical(&root, record.allocation()),
        Err(Error::Allocation)
    ));
    seam::set(&root.join("generated/b"), MAX_BYTES);
    assert!(matches!(
        validate_physical(&root, record.allocation()),
        Err(Error::Limit { .. })
    ));
    seam::clear_all();
    validate_physical(&root, record.allocation()).expect("the bundle is otherwise intact");
    fixture.retire();
}

#[test]
fn rejection_matrix_at_inventory_and_retirement() {
    for case in matrix() {
        let mut fixture = Fixture::new("inventory-matrix");
        let (root, record) = fixture.published(case.inflate);
        seam::clear_all();
        validate_physical(&root, record.allocation())
            .unwrap_or_else(|error| panic!("{}: the intact bundle fails: {error:?}", case.name));
        mutate(&fixture.root, &root, &case);
        let inventory = validate_physical(&root, record.allocation());
        assert!(
            matches!(inventory, Err(Error::CleanupRequired(_))),
            "{}: inventory must refuse with CleanupRequired, got {inventory:?}",
            case.name
        );
        let retiring = validate_physical_retiring(&root, record.allocation());
        assert!(
            matches!(retiring, Err(Error::CleanupRequired(_))),
            "{}: retirement must refuse with CleanupRequired, got {retiring:?}",
            case.name
        );
        fixture.retire();
    }
}

#[test]
fn rejection_matrix_at_pending_seal_completion_and_publication() {
    for case in matrix() {
        let mut fixture = Fixture::new("pending-matrix");
        let (root, mut pending) = fixture.sealed(populate, case.inflate);
        seam::clear_all();
        let reference = pending.reference;
        let digest: [u8; 32] = Sha256::digest(SEAL).into();
        // Pending trees are not frozen yet: apply the logical change directly
        // and model mode changes on the frozen mode the seal will apply.
        if case.name == "mode change (member made writable)" {
            // Writability is not observable before the freeze; the
            // published matrix covers it.
            drop(pending);
            fixture.retire();
            continue;
        }
        (case.apply)(&root, &fixture.root);
        if case.name == "mode change (executable bit added)" {
            fs::set_permissions(root.join("generated/a"), fs::Permissions::from_mode(0o755))
                .unwrap();
        } else {
            (case.after)(&root);
        }
        let recheck = pending.validate_unpublished(reference, SEAL.len(), digest);
        assert!(
            matches!(recheck, Err(Error::Corrupt { .. } | Error::Shape(_))),
            "{}: pending seal recheck must reject, got {recheck:?}",
            case.name
        );
        let graph = crate::node::GraphBudget::new();
        let complete = pending.validate_unpublished_budgeted(reference, SEAL.len(), digest, &graph);
        assert!(
            matches!(complete, Err(Error::Corrupt { .. } | Error::Shape(_))),
            "{}: pending complete scope must reject, got {complete:?}",
            case.name
        );
        let commitment = BundleCommitment::new(reference, SEAL.len(), digest);
        let (_, completion) = pending.complete_replay(commitment, &graph).unwrap();
        let observed = completion.observe(&root, &graph, |_, _| Ok(()));
        assert!(
            matches!(observed, Err(Error::Corrupt { .. } | Error::Shape(_))),
            "{}: pending completion must reject, got {observed:?}",
            case.name
        );
        if !matches!(
            case.name,
            "file replaced by a symlink" | "special file" | "added internal hard link"
        ) {
            let published = pending.publish(
                Kind::Runner,
                2,
                Digest::from_bytes([0x73; 32]),
                LeaseKey::Execution(reference.execution()),
                Digest::from_bytes([0x74; 32]),
            );
            assert!(
                published.is_err(),
                "{}: publication must reject a drifted seal",
                case.name
            );
        }
        drop(pending);
        fixture.retire();
    }
}

#[test]
fn geometry_commitment_is_independent_of_enumeration_and_creation_order() {
    let mut fixture = Fixture::new("determinism");
    let (first_root, first) = fixture.sealed(populate, &[]);
    let (second_root, second) = fixture.sealed(populate_reversed, &[("generated/b", MIB)]);
    let (_, third) = fixture.sealed(populate_one_byte_longer, &[]);
    assert_ne!(first_root, second_root);
    assert_eq!(
        first.geometry, second.geometry,
        "the commitment depends on the tree, not on creation order, root or allocation"
    );
    assert_ne!(first.geometry, third.geometry);
    let walked = survey(&first_root, View::Pending, None, false, Error::Shape).unwrap();
    assert_eq!(
        walked.geometry, first.geometry,
        "the commitment is reproducible"
    );
    // The traversal reads every directory in sorted order.
    let names: Vec<_> = read_sorted(&first_root.join("generated"))
        .unwrap()
        .into_iter()
        .map(|entry| entry.file_name())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    drop((first, second, third));
    fixture.retire();
}

#[test]
fn published_geometry_matches_the_sealed_commitment() {
    let mut fixture = Fixture::new("published-geometry");
    let (root, record) = fixture.published(&[]);
    let published = survey(&root, View::Published, None, true, Error::Shape).unwrap();
    assert_eq!(Some(published.geometry), record.sealed_geometry());
    thaw_root(&root).unwrap();
    let retiring = survey(&root, View::Retiring, None, false, Error::Shape).unwrap();
    assert_eq!(Some(retiring.geometry), record.sealed_geometry());
    let exact = survey(&root, View::Published, None, false, Error::Shape).unwrap();
    assert_ne!(
        Some(exact.geometry),
        record.sealed_geometry(),
        "a thawed root is a mode change outside retirement"
    );
    fixture.retire();
}

#[test]
fn retirement_race_completes_when_allocation_falls_after_measurement() {
    let mut fixture = Fixture::new("retirement-race");
    let (root, record) = fixture.published(&[]);
    let allocation = record.allocation();
    // The retirement measurement sees more allocation than recorded.
    seam::set(&root.join("generated/b"), MIB);
    let mut larger = super::super::recovery::tree_charge(&root, 1024).unwrap();
    larger.allocations = 1;
    assert!(larger.bytes > allocation.charge.bytes);
    let mut recovery = super::super::recovery::Budget::new();
    let repair = prepare_eviction_at(root.clone(), allocation, &mut recovery)
        .unwrap()
        .unwrap()
        .into_repair(allocation, &mut recovery)
        .unwrap();
    // Allocation falls before the recheck.
    seam::clear_all();
    let excess = repair
        .validate(&root, allocation)
        .expect("a fall between measurement and recheck completes retirement");
    assert_eq!(
        excess.bytes,
        larger.bytes - allocation.charge.bytes,
        "excess is charged from the larger measurement"
    );
    // A logical change between measurement and recheck still rejects.
    thaw(&root).unwrap();
    set_len(&root.join("generated/a"), 9);
    refreeze(&root);
    assert!(matches!(
        repair.validate(&root, allocation),
        Err(Error::CleanupRequired(
            "bundle changed after retirement measurement"
        ))
    ));
    drop(repair);
    fixture.retire();
}

#[test]
fn legacy_schema_four_bundle_is_judged_by_the_labelled_legacy_rule() {
    let mut fixture = Fixture::new("legacy");
    seam::legacy_writer(true);
    let (root, record) = fixture.published(&[("generated/b", 256 * KIB)]);
    seam::legacy_writer(false);
    assert!(record.is_legacy_bundle());
    let before = fs::read(root.join("allocation")).unwrap();
    assert_eq!(before.len(), accounting::RECORD_BYTES);
    assert_eq!(u16::from_le_bytes([before[8], before[9]]), 4);
    seam::clear_all();
    // Release: same counts, measured <= recorded.
    validate_physical(&root, record.allocation()).expect("the legacy rule admits a release");
    validate_physical_retiring(&root, record.allocation())
        .expect("the legacy retiring rule admits a release");
    // Growth rejects.
    seam::set(&root.join("generated/a"), MIB);
    assert!(matches!(
        validate_physical(&root, record.allocation()),
        Err(Error::CleanupRequired(_))
    ));
    seam::clear_all();
    // A count change rejects.
    thaw(&root).unwrap();
    fs::write(root.join("generated/new"), b"new").unwrap();
    refreeze(&root);
    assert!(matches!(
        validate_physical(&root, record.allocation()),
        Err(Error::CleanupRequired(_))
    ));
    thaw(&root).unwrap();
    fs::remove_file(root.join("generated/new")).unwrap();
    // A writable member still rejects under the existing mode check.
    assert!(validate_physical(&root, record.allocation()).is_err());
    refreeze(&root);
    validate_physical(&root, record.allocation()).unwrap();
    // Nothing is upgraded in place.
    assert_eq!(fs::read(root.join("allocation")).unwrap(), before);
    assert!(Record::read_bundle(&root).unwrap().is_legacy_bundle());
    fixture.retire();
}

#[test]
fn bundle_record_schema_leaves_the_ledger_fence_and_typed_records_alone() {
    let mut fixture = Fixture::new("fence");
    let (root, _record) = fixture.published(&[]);
    let bytes = fs::read(root.join("allocation")).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 5);
    // An older reader, which accepts only 184-byte schema-4 records, fails
    // closed on a schema-5 bundle record rather than reinterpreting it.
    assert!(bytes.len() != accounting::RECORD_BYTES);
    // The installation ledger header schema is untouched by the bundle bump.
    let ledger = fs::read(fixture.store.root().join("ledgers/installation")).unwrap();
    assert_eq!(&ledger[..8], b"RAWRLEDR");
    assert_eq!(u16::from_le_bytes([ledger[8], ledger[9]]), 5);
    fixture.retire();
}
