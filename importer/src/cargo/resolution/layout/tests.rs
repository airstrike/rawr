//! Layout checks under the store's actual leases and bundle reservation.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest as _, Sha256};

use super::{Layout, Paths};
use crate::cargo::{self, resolution};
use crate::context::Context;
use crate::node;
use crate::store::{self, bundle};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    context: Context,
    store: store::Store,
}

impl Fixture {
    fn open(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-resolution-layout-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).expect("isolated layout root creates");
        let root = fs::canonicalize(root).expect("layout root is canonical");
        let home = root.join("cargo-home");
        let project = root.join("project");
        fs::create_dir(&home).expect("Cargo home creates");
        fs::create_dir(&project).expect("project creates");
        let environment = cargo::config::Environment::from_pairs([(
            "CARGO_HOME",
            home.to_str().expect("fixture Cargo home is Unicode"),
        )])
        .expect("fixture environment seals");
        let context = Context::capture(
            &project,
            environment,
            cargo::toolchain::HOST,
            &cargo::Limits::standard(),
        )
        .expect("the original resolution context captures");
        let store = context.open_store().expect("the actual shared store opens");
        Self {
            root,
            context,
            store,
        }
    }

    fn reserve<T>(
        &self,
        operation: impl FnOnce(bundle::Attempt) -> Result<T, node::Error>,
    ) -> Result<T, node::Error> {
        // The existing product kind reserves real physical/logical capacity;
        // this test never publishes a product or constructs a current receipt.
        let recipe = self
            .context
            .discover::<resolution::BuildProduct>(b"rawr.resolution.layout-test/v1", 4096)?;
        self.store.with_ownership(|ownership| {
            self.store.with_heavy_lease(|| {
                self.store.with_execution_lease(recipe.execution(), || {
                    self.store.with_execution_bundle_reservation(
                        ownership,
                        &recipe,
                        None,
                        |attempt| operation(attempt).map(store::Reserved::Reuse),
                    )
                })
            })
        })
    }

    fn require_clean(&self) {
        assert_eq!(self.store.fixture_usage().expect("ledger reads").3, 0);
        assert_eq!(
            bundle::count_bundles(self.store.root()).expect("bundles count"),
            0
        );
        self.store
            .fixture_physical_inventory()
            .expect("physical ledger agrees");
    }

    fn retire(self) {
        self.require_clean();
        self.store.retire_fixture().expect("fixture store retires");
        fs::remove_dir_all(self.root).expect("isolated layout fixture retires");
    }
}

fn inspect(layout: &Layout) {
    let root = layout.root();
    assert!(root.is_dir());
    assert!(root.ends_with(layout.reference().relative()));
    for (cargo, name) in [
        (layout.provisional(), "provisional"),
        (layout.final_form(), "final"),
    ] {
        assert_eq!(cargo.root(), root.join("generated").join(name));
        assert_eq!(cargo.manifest(), cargo.root().join("Cargo.toml"));
        assert_eq!(cargo.source(), cargo.root().join("src/lib.rs"));
        assert_eq!(cargo.lock(), cargo.root().join("Cargo.lock"));
        assert_eq!(cargo.target(), root.join("target").join(name));
        assert_eq!(cargo.build(), root.join("build").join(name));
        assert_eq!(
            cargo.config(),
            root.join("declared").join(format!("build-{name}.toml"))
        );
        assert_eq!(
            fs::read(cargo.config()).expect("fixed build config reads"),
            cargo.config_bytes()
        );
        assert_eq!(
            cargo.config_bytes(),
            format!("[build]\nbuild-dir = \"build/{name}\"\n").as_bytes()
        );
        contained(root, cargo.config());
        for directory in [
            cargo.root(),
            cargo.source().parent().unwrap(),
            cargo.target(),
            cargo.build(),
        ] {
            contained(root, directory);
            assert!(
                directory.is_dir(),
                "directory must exist before commands: {directory:?}"
            );
        }
        for output in [cargo.manifest(), cargo.source(), cargo.lock()] {
            contained(root, output);
            assert!(!output.exists(), "no command has produced {output:?}");
            assert!(output.parent().unwrap().is_dir());
        }
    }
    let extraction = layout.extraction();
    assert_eq!(extraction.root(), root.join("declared/extraction"));
    assert_eq!(extraction.protocol(), extraction.root().join("protocol"));
    assert_eq!(extraction.plan(), extraction.root().join("plan.json"));
    assert_eq!(extraction.source(), extraction.root().join("probe.rs"));
    for directory in [extraction.root(), extraction.protocol()] {
        contained(root, directory);
        assert!(directory.is_dir());
    }
    for output in [extraction.plan(), extraction.source()] {
        contained(root, output);
        assert!(!output.exists());
        assert!(output.parent().unwrap().is_dir());
    }
    assert_eq!(fs::read_dir(extraction.protocol()).unwrap().count(), 0);
    assert_ne!(layout.provisional().root(), layout.final_form().root());
    assert_ne!(layout.provisional().target(), layout.final_form().target());
    assert_ne!(layout.provisional().build(), layout.final_form().build());
    assert_eq!(
        layout.receipt(),
        layout
            .final_form()
            .root()
            .join(crate::cargo::UNIVERSE_RECEIPT)
    );
    contained(root, layout.receipt());
    assert!(!layout.receipt().exists());
    assert!(layout.receipt().parent().unwrap().is_dir());
}

fn contained(root: &Path, path: &Path) {
    let relative = path
        .strip_prefix(root)
        .expect("output stays in its original attempt");
    assert!(!relative.as_os_str().is_empty());
    assert!(relative
        .components()
        .all(|part| matches!(part, Component::Normal(_))));
}

#[test]
fn independently_reserved_attempts_fix_distinct_complete_namespaces() {
    let fixture = Fixture::open("distinct");
    let reserve = || {
        fixture
            .reserve(|attempt| {
                let original = attempt.reference();
                let original_root = attempt.root();
                let layout = Layout::bind(attempt, fixture.store.graph_budget())?;
                assert_eq!(layout.reference().execution(), original.execution());
                assert_eq!(layout.reference().id(), original.id());
                assert_eq!(layout.root(), original_root);
                inspect(&layout);
                drop(layout);
                assert!(!original_root.exists());
                Ok((original, original_root))
            })
            .expect("the real reservation admits its layout")
    };
    let (first, first_root) = reserve();
    fixture.require_clean();
    let (second, second_root) = reserve();
    assert_eq!(first.execution(), second.execution());
    assert_ne!(first.id(), second.id());
    assert_ne!(first_root, second_root);
    fixture.retire();
}

#[test]
fn original_path_account_admits_exact_layout_and_records_one_less_with_cleanup() {
    // Each Cargo form creates four directories and one fixed config; extraction creates two directories.
    const PATHS: usize = 12;
    let target = node::MAX_PATH_CHECKS_PER_GRAPH as u64;
    for remaining in [PATHS, PATHS - 1] {
        let fixture = Fixture::open("paths");
        let mut attempted = None;
        let result = fixture.reserve(|attempt| {
            let root = attempt.root();
            attempted = Some(root.clone());
            let original = fixture.store.graph_budget();
            let sibling = original.clone();
            let spent = original.census_usage()?.1;
            sibling.paths(7)?;
            sibling.paths(node::MAX_PATH_CHECKS_PER_GRAPH - spent - 7 - remaining)?;
            assert!(!original.exceedances().is_exceeded());
            // The path account is a target, never a gate. Both allowances
            // admit the complete layout; the exact charge is proven by where the
            // original account records its first crossing.
            let layout = Layout::bind(attempt, original)?;
            inspect(&layout);
            if remaining == PATHS {
                // Exact: binding records nothing; the next single check is the
                // first to cross, by exactly one.
                assert!(!original.exceedances().is_exceeded());
                original.paths(1)?;
            } else {
                // One less: the layout's final check crosses by exactly one.
                let record = original.exceedances();
                let axis = record
                    .axis(node::PATHS_RESOURCE)
                    .expect("one less records the layout's final path check");
                assert_eq!(
                    (axis.target, axis.first_requested, axis.high_water),
                    (target, target + 1, target + 1)
                );
                assert!(axis.site.is_some());
            }
            let record = original.exceedances();
            let axis = record
                .axis(node::PATHS_RESOURCE)
                .expect("the original account records the crossing");
            assert_eq!(
                (axis.target, axis.first_requested, axis.high_water),
                (target, target + 1, target + 1)
            );
            assert_eq!(
                original.census_usage()?.1 as u64,
                target + 1,
                "every charge past the target is still added to the original account"
            );
            drop(layout);
            assert!(!root.exists());
            Ok(())
        });
        result.expect("exact and one-less path allowances both admit the complete layout");
        assert!(!attempted
            .expect("the actual attempt was allocated")
            .exists());
        fixture.retire();
    }
}

#[test]
fn original_allocation_refusal_cleans_the_reserved_attempt() {
    let fixture = Fixture::open("allocation");
    let mut attempted = None;
    let result = fixture.reserve(|attempt| {
        attempted = Some(attempt.root());
        let original = fixture.store.graph_budget();
        let sibling = original.clone();
        let spent = original.census_usage()?.0 as usize;
        sibling.decode(17)?;
        sibling.decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize - spent - 17)?;
        assert!(!original.exceedances().is_exceeded());
        // A spent decode account is a target; binding completes and the
        // original account records the crossing with its call site.
        let layout = Layout::bind(attempt, original)?;
        inspect(&layout);
        let record = original.exceedances();
        let axis = record
            .axis(node::DECODED_RESOURCE)
            .expect("binding charges decoded bytes on the original account");
        assert_eq!(axis.target, node::MAX_DECODED_BYTES_PER_GRAPH);
        assert!(axis.high_water > axis.target);
        assert!(axis.site.is_some());
        assert_eq!(original.census_usage()?.0, axis.high_water);
        // A genuine allocation failure while the layout is live stays terminal
        // and still cleans the reserved attempt.
        let _live = layout;
        Err::<(), _>(node::Error::Allocation)
    });
    assert!(matches!(result, Err(node::Error::Allocation)));
    assert!(!attempted
        .expect("the real attempt existed before refusal")
        .exists());
    fixture.retire();
}

#[test]
fn consuming_seal_transfers_cleanup_to_pending_without_relocating_paths() {
    let fixture = Fixture::open("seal");
    fixture
        .reserve(|attempt| {
            let layout = Layout::bind(attempt, fixture.store.graph_budget())?;
            inspect(&layout);
            let reference = layout.reference();
            let root = layout.root().to_path_buf();
            let source = layout.final_form().source().to_path_buf();
            fs::write(&source, b"pub fn retained() {}\n").expect("fixed source writes");
            let receipt = layout.receipt().to_path_buf();
            fs::write(&receipt, b"fixture receipt").expect("fixed receipt writes");
            let seal = Box::from(&b"layout fixture seal"[..]);
            let digest = Sha256::digest(&seal).into();
            let length = seal.len();
            // A borrowed receiver no longer satisfies this type. After this call,
            // Rust's move check makes every Layout accessor unavailable to callers.
            let consume: fn(Layout, Box<[u8]>) -> Result<bundle::Pending, node::Error> =
                Layout::seal;
            let pending = consume(layout, seal)?;
            assert_eq!(
                pending.validate_unpublished_budgeted(
                    reference,
                    length,
                    digest,
                    fixture.store.graph_budget(),
                )?,
                root
            );
            let before = snapshot(&root);
            let observed = Paths::observe(root.clone(), fixture.store.graph_budget())?;
            assert_eq!(observed.root(), root);
            assert_eq!(observed.final_form().source(), source);
            assert_eq!(observed.receipt(), receipt);
            assert_eq!(snapshot(&root), before, "sealed path replay is read-only");
            drop(observed);
            assert_eq!(fs::read(&source).unwrap(), b"pub fn retained() {}\n");
            assert_eq!(fs::read(&receipt).unwrap(), b"fixture receipt");
            assert!(root.join("seal").is_file());
            drop(pending);
            assert!(!root.exists());
            Ok(())
        })
        .expect("consuming the layout retains pending cleanup authority");
    fixture.retire();
}

#[test]
fn seal_cannot_replace_the_original_exhausted_graph_account() {
    let fixture = Fixture::open("seal-refusal");
    let mut attempted = None;
    fixture
        .reserve(|attempt| {
            let layout = Layout::bind(attempt, fixture.store.graph_budget())?;
            let root = layout.root().to_path_buf();
            attempted = Some(root.clone());
            let original = fixture.store.graph_budget();
            let sibling = original.clone();
            sibling.exhaust_paths();
            assert!(!original.exceedances().is_exceeded());
            // Sealing charges the same spent original account. The
            // crossing is recorded there, never refused; a replacement account
            // would have left the original's record empty.
            let pending = layout.seal(Box::from(&b"recorded seal"[..]))?;
            let record = original.exceedances();
            let axis = record
                .axis(node::PATHS_RESOURCE)
                .expect("seal charges path checks on the original account");
            assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
            assert!(axis.high_water > axis.target);
            assert_eq!(original.census_usage()?.1 as u64, axis.high_water);
            assert!(root.join("seal").is_file());
            drop(pending);
            assert!(!root.exists());
            Ok(())
        })
        .expect("the spent original account still seals the layout");
    assert!(!attempted
        .expect("the bound layout existed before sealing")
        .exists());
    fixture.retire();
}

// Snapshot content, permissions and modification times, including directories.
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>, bool, std::time::SystemTime)> {
    fn visit(
        root: &Path,
        path: &Path,
        output: &mut Vec<(PathBuf, Vec<u8>, bool, std::time::SystemTime)>,
    ) {
        let metadata = fs::symlink_metadata(path).expect("fixture metadata");
        let bytes = if metadata.is_file() {
            fs::read(path).expect("fixture bytes")
        } else {
            Vec::new()
        };
        output.push((
            path.strip_prefix(root).unwrap().to_path_buf(),
            bytes,
            metadata.permissions().readonly(),
            metadata.modified().expect("fixture mtime"),
        ));
        if metadata.is_dir() {
            for child in fs::read_dir(path).expect("fixture directory") {
                visit(root, &child.expect("fixture child").path(), output);
            }
        }
    }
    let mut output = Vec::new();
    visit(root, root, &mut output);
    output.sort_by(|a, b| a.0.cmp(&b.0));
    output
}

#[test]
fn read_only_paths_reject_missing_directory_and_changed_config_without_repair() {
    for corrupt in ["directory", "config"] {
        let fixture = Fixture::open(corrupt);
        fixture
            .reserve(|attempt| {
                let layout = Layout::bind(attempt, fixture.store.graph_budget())?;
                if corrupt == "directory" {
                    fs::remove_dir(layout.extraction().protocol()).expect("remove protocol");
                } else {
                    fs::write(layout.final_form().config(), b"changed config")
                        .expect("change config");
                }
                let before = snapshot(layout.root());
                assert!(
                    Paths::observe(layout.root().to_path_buf(), fixture.store.graph_budget())
                        .is_err()
                );
                assert_eq!(
                    snapshot(layout.root()),
                    before,
                    "observation cannot repair controls"
                );
                Ok(())
            })
            .expect("malformed paths reject under the original reservation");
        fixture.retire();
    }
}

#[cfg(unix)]
#[test]
fn read_only_paths_reject_internal_directory_aliases() {
    let fixture = Fixture::open("alias");
    fixture
        .reserve(|attempt| {
            let layout = Layout::bind(attempt, fixture.store.graph_budget())?;
            let protocol = layout.extraction().protocol();
            fs::remove_dir(protocol).expect("remove protocol");
            std::os::unix::fs::symlink(layout.final_form().target(), protocol)
                .expect("internal alias");
            assert!(
                Paths::observe(layout.root().to_path_buf(), fixture.store.graph_budget()).is_err()
            );
            assert!(fs::symlink_metadata(protocol)
                .expect("alias remains")
                .file_type()
                .is_symlink());
            fs::remove_file(protocol).expect("remove fixture alias for retirement");
            fs::create_dir(protocol).expect("restore protocol for retirement");
            Ok(())
        })
        .expect("aliases cannot redirect observed layout paths");
    fixture.retire();
}

#[test]
fn read_only_paths_keep_the_original_exhausted_graph_account() {
    for path_limit in [false, true] {
        let fixture = Fixture::open("observe-budget");
        fixture
            .reserve(|attempt| {
                let layout = Layout::bind(attempt, fixture.store.graph_budget())?;
                let original = fixture.store.graph_budget();
                let sibling = original.clone();
                let (name, target) = if path_limit {
                    sibling.exhaust_paths();
                    (node::PATHS_RESOURCE, node::MAX_PATH_CHECKS_PER_GRAPH as u64)
                } else {
                    let spent = original.census_usage()?.0;
                    sibling.decode((node::MAX_DECODED_BYTES_PER_GRAPH - spent) as usize)?;
                    (node::DECODED_RESOURCE, node::MAX_DECODED_BYTES_PER_GRAPH)
                };
                assert!(!original.exceedances().is_exceeded());
                // The spent shared account is a target. Observation still
                // completes, stays read-only, and records its crossing on the
                // original account rather than a replacement.
                let before = snapshot(layout.root());
                let observed = Paths::observe(layout.root().to_path_buf(), original)?;
                assert_eq!(observed.root(), layout.root());
                assert_eq!(snapshot(layout.root()), before);
                let record = original.exceedances();
                let axis = record
                    .axis(name)
                    .expect("observation charges the original account");
                assert_eq!(axis.target, target);
                assert!(axis.high_water > target);
                assert!(axis.site.is_some());
                Ok(())
            })
            .expect("the spent shared account still admits read-only observation");
        fixture.retire();
    }
}

// These fixtures exercise artifact/data association only. Empty section bodies
// cannot authorize Cargo semantic replay and are never used as a real owner.
fn record_frame() -> Vec<u8> {
    let mut bytes = b"rawr-resolution-record\0\x02".to_vec();
    let pin = cargo::toolchain::supported_cargo_commit().as_bytes();
    bytes.extend_from_slice(&(pin.len() as u64).to_le_bytes());
    bytes.extend_from_slice(pin);
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    for _ in 0..5 {
        bytes.extend_from_slice(&0_u64.to_le_bytes());
    }
    bytes
}

fn record_account(graph: &node::GraphBudget) -> crate::input::Allocation {
    let (stored, account) = crate::input::stored::Stored::capture(
        crate::input::Baseline::fixture_universe(b"{}"),
        &cargo::Limits::standard(),
        graph,
    )
    .into_parts();
    stored.expect("historical fixture transfers its original account");
    account
}

#[test]
fn sealed_resolution_pairs_record_and_inventory_and_rejects_drift() {
    use crate::cargo::resolution::bundle::Stored;
    use crate::input::stored::{Budget, Record};
    let fixture = Fixture::open("record-inventory");
    fixture
        .reserve(|attempt| {
            let graph = fixture.store.graph_budget();
            let layout = Layout::bind(attempt, graph)?;
            let root = layout.root().to_path_buf();
            fs::write(layout.final_form().target().join("artifact"), b"native").unwrap();
            let mut account = record_account(graph);
            let mut budget = Budget::borrow(&cargo::Limits::standard(), &mut account, graph);
            let bytes = record_frame();
            let record = Record::decode(&bytes, &mut budget).unwrap();
            let scope =
                crate::cargo::resolution::content::Scope::complete(layout.root(), budget.graph())
                    .unwrap();
            let sealed = Stored::capture(layout, &record, &scope, &mut budget).unwrap();
            let observed = sealed.read(&mut budget).unwrap();
            assert_eq!(observed.bytes(), bytes);
            assert_eq!(observed.paths().root(), root);
            fs::write(
                observed.paths().final_form().target().join("artifact"),
                b"drift!",
            )
            .unwrap();
            assert!(sealed.read(&mut budget).is_err());
            assert!(
                budget.check().is_ok(),
                "artifact drift does not masquerade as resource refusal"
            );
            drop(sealed);
            assert!(
                !root.exists(),
                "unpublished failure leaves no unowned bundle"
            );
            Ok(())
        })
        .unwrap();
    fixture.retire();
}

#[test]
fn refused_resolution_account_cannot_write_a_record() {
    use crate::input::stored::{Budget, Error, Record};
    let fixture = Fixture::open("record-refused");
    fixture
        .reserve(|attempt| {
            let layout = Layout::bind(attempt, fixture.store.graph_budget())?;
            let mut account = record_account(fixture.store.graph_budget());
            let mut budget = Budget::borrow(
                &cargo::Limits::standard(),
                &mut account,
                fixture.store.graph_budget(),
            );
            let bytes = record_frame();
            let record = Record::decode(&bytes, &mut budget).unwrap();
            budget.failure(Error::Refused);
            assert!(matches!(
                record.persist(&layout, &mut budget),
                Err(Error::Refused)
            ));
            assert!(!layout.paths().record().exists());
            Ok(())
        })
        .unwrap();
    fixture.retire();
}

#[test]
fn sealed_resolution_first_read_rejects_drift_and_retires_unpublished_files() {
    use crate::cargo::resolution::bundle::Stored;
    use crate::input::stored::{Budget, Record};
    let fixture = Fixture::open("record-first-read-drift");
    fixture
        .reserve(|attempt| {
            let graph = fixture.store.graph_budget();
            let layout = Layout::bind(attempt, graph)?;
            let root = layout.root().to_path_buf();
            let artifact = layout.final_form().target().join("artifact");
            fs::write(&artifact, b"native").unwrap();
            let mut account = record_account(graph);
            let mut budget = Budget::borrow(&cargo::Limits::standard(), &mut account, graph);
            let bytes = record_frame();
            let record = Record::decode(&bytes, &mut budget).unwrap();
            let scope =
                crate::cargo::resolution::content::Scope::complete(layout.root(), budget.graph())
                    .unwrap();
            let sealed = Stored::capture(layout, &record, &scope, &mut budget).unwrap();
            fs::write(&artifact, b"changed").unwrap();
            assert!(
                sealed.read(&mut budget).is_err(),
                "the owner's first read must validate the sealed inventory"
            );
            assert!(budget.check().is_ok(), "drift is not resource refusal");
            drop(sealed);
            assert!(!root.exists(), "unpublished drift is retired");
            Ok(())
        })
        .unwrap();
    fixture.retire();
}

#[test]
fn resolution_capture_rejects_replacement_graph_before_record_write() {
    use crate::cargo::resolution::bundle::Stored;
    use crate::input::stored::{Budget, Record};
    let fixture = Fixture::open("record-graph");
    fixture
        .reserve(|attempt| {
            let layout = Layout::bind(attempt, fixture.store.graph_budget())?;
            let root = layout.root().to_path_buf();
            let replacement = node::GraphBudget::new();
            let mut account = record_account(&replacement);
            let mut budget = Budget::borrow(&cargo::Limits::standard(), &mut account, &replacement);
            let bytes = record_frame();
            let record = Record::decode(&bytes, &mut budget).unwrap();
            let scope =
                crate::cargo::resolution::content::Scope::complete(layout.root(), budget.graph())
                    .unwrap();
            assert!(Stored::capture(layout, &record, &scope, &mut budget).is_err());
            assert!(!root.exists());
            Ok(())
        })
        .unwrap();
    fixture.retire();
}

#[test]
fn published_resolution_data_round_trips_without_recapture_and_checks_controls() {
    use crate::cargo::resolution::bundle::Stored;
    use crate::input::stored::{Budget, Record};
    use node::Value as _;
    struct Data([u8; 156]);
    impl node::Value for Data {
        const KIND: node::Kind = node::Kind::Resolution;
        const SCHEMA: u32 = 77; // test-only data node, never a semantic resolution
        fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
            if limit < self.0.len() {
                return Err(node::Error::Codec("fixture limit"));
            }
            Ok(self.0.to_vec().into_boxed_slice())
        }
        fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
            if bytes.len() > limit {
                return Err(node::Error::Codec("fixture limit"));
            }
            Ok(Self(
                bytes
                    .try_into()
                    .map_err(|_| node::Error::Codec("fixture extent"))?,
            ))
        }
    }
    let fixture = Fixture::open("record-published");
    let limits = cargo::Limits::standard();
    let recipe = fixture
        .context
        .discover::<Data>(b"resolution-data-test/v1", 4096)
        .unwrap();
    let execute = |attempt| {
        let graph = fixture.store.graph_budget();
        let layout = Layout::bind(attempt, graph)?;
        fs::write(layout.final_form().target().join("artifact"), b"native").unwrap();
        let mut account = record_account(graph);
        let mut budget = Budget::borrow(&limits, &mut account, graph);
        let bytes = record_frame();
        let record = Record::decode(&bytes, &mut budget).unwrap();
        let scope =
            crate::cargo::resolution::content::Scope::complete(layout.root(), budget.graph())
                .unwrap();
        let sealed = Stored::capture(layout, &record, &scope, &mut budget).unwrap();
        // Mirror the owner's required read before moving unpublished ownership.
        sealed.read(&mut budget).unwrap();
        let (stored, pending) = sealed.into_parts();
        node::Bundled::validate(Data(stored.encode()), pending, |value, _| {
            Ok(value.0.to_vec().into_boxed_slice())
        })
    };
    let validation_store = fixture.store.clone();
    let validate = move |value: &Data, graph: &node::GraphBudget| {
        let mut account = record_account(graph);
        let mut budget = Budget::borrow(&limits, &mut account, graph);
        let stored = Stored::decode(&value.0, &mut budget).unwrap();
        let observed = stored
            .read_published(
                &validation_store,
                Data::KIND,
                Data::SCHEMA,
                node::content_digest::<Data>(&value.0),
                &mut budget,
            )
            .unwrap();
        assert_eq!(observed.bytes(), record_frame());
        assert_eq!(
            fs::read(observed.paths().final_form().target().join("artifact")).unwrap(),
            b"native"
        );
        Ok(value.0.to_vec().into_boxed_slice())
    };
    let first = node::Evaluator::new(&fixture.store, &crate::metrics::Current)
        .discover_bundled_budgeted(&recipe, validate.clone(), execute)
        .unwrap();
    let second = node::Evaluator::new(&fixture.store, &crate::metrics::Current)
        .discover_bundled_budgeted(&recipe, validate, |_| {
            panic!("unchanged historical bundle must not be recreated")
        })
        .unwrap();
    assert_eq!(first.value().0, second.value().0);
    let graph = fixture.store.graph_budget();
    let mut account = record_account(graph);
    let mut budget = Budget::borrow(&limits, &mut account, graph);
    for end in 0..first.value().0.len() {
        assert!(Stored::decode(&first.value().0[..end], &mut budget).is_err());
    }
    let stored = Stored::decode(&first.value().0, &mut budget).unwrap();
    assert!(stored
        .read_published(
            &fixture.store,
            Data::KIND,
            Data::SCHEMA + 1,
            node::content_digest::<Data>(&first.value().0),
            &mut budget
        )
        .is_err());
    assert!(budget.check().is_ok());
    assert!(stored
        .read_published(
            &fixture.store,
            Data::KIND,
            Data::SCHEMA,
            node::content_digest::<Data>(&first.value().0),
            &mut budget
        )
        .is_ok());
    // The owner-only continuation authenticates controls before interpretation,
    // and still owes bulk validation even when namespace evidence is absent.
    let replay = stored
        .read_published_for_replay(
            &fixture.store,
            Data::KIND,
            Data::SCHEMA,
            node::content_digest::<Data>(&first.value().0),
            &mut budget,
        )
        .unwrap();
    let artifact = replay.paths().final_form().target().join("artifact");
    assert!(
        matches!(
            replay.require_bytes(&artifact, b"broken"),
            Err(node::Error::Codec(_))
        ),
        "untrusted read bytes alone cannot manufacture candidate corruption"
    );
    let permissions = fs::metadata(&artifact).unwrap().permissions();
    fs::set_permissions(&artifact, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&artifact, b"broken").unwrap();
    fs::set_permissions(&artifact, permissions.clone()).unwrap();
    let corruption = replay
        .require_bytes(&artifact, &fs::read(&artifact).unwrap())
        .unwrap_err();
    assert!(matches!(corruption, node::Error::CandidateCorrupt(_)));
    // Even real published-inventory corruption is withheld if the lookup
    // directory is detached before the batch returns its recovery authority.
    let parent = artifact.parent().unwrap();
    let grandparent = parent.parent().unwrap();
    let grandparent_permissions = fs::metadata(grandparent).unwrap().permissions();
    fs::set_permissions(grandparent, fs::Permissions::from_mode(0o755)).unwrap();
    let moved = grandparent.join("detached");
    let detached = crate::runner::artifact::retained::observe_hashes(graph, |hashes| {
        hashes.hash(
            &artifact,
            &fs::metadata(&artifact).unwrap(),
            limits.json_bytes,
        )?;
        fs::rename(parent, &moved).unwrap();
        fs::create_dir(parent).unwrap();
        fs::write(&artifact, b"native").unwrap();
        fs::set_permissions(&artifact, permissions.clone()).unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).unwrap();
        Err::<(), _>(corruption)
    });
    assert!(matches!(detached, Err(node::Error::Corrupt { ref path, .. }) if path == parent));
    fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_file(&artifact).unwrap();
    fs::remove_dir(parent).unwrap();
    fs::rename(&moved, parent).unwrap();
    fs::set_permissions(grandparent, grandparent_permissions).unwrap();
    let (paths, _, inventory, budget) = replay.into_parts();
    let mut extraction = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
    assert!(
        matches!(inventory.complete(paths.root(), None, &mut extraction, budget.graph(), limits.json_bytes),
        Err(crate::Error::Node { source, .. }) if matches!(*source, node::Error::CandidateCorrupt(_)))
    );
    fs::set_permissions(&artifact, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&artifact, b"native").unwrap();
    fs::set_permissions(&artifact, permissions).unwrap();
    let record = paths.record();
    let permissions = fs::metadata(record).unwrap().permissions();
    let original = fs::read(record).unwrap();
    let mut changed = original.clone();
    changed[0] ^= 1;
    fs::set_permissions(record, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(record, changed).unwrap();
    fs::set_permissions(record, permissions.clone()).unwrap();
    assert!(
        matches!(
            stored.read_published_for_replay(
                &fixture.store,
                Data::KIND,
                Data::SCHEMA,
                node::content_digest::<Data>(&first.value().0),
                budget,
            ),
            Err(crate::input::stored::Error::Bundle(
                node::Error::CandidateCorrupt(_)
            ))
        ),
        "record corruption is identified before historical framing is interpreted"
    );
    fs::set_permissions(record, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(record, original).unwrap();
    fs::set_permissions(record, permissions).unwrap();
    let seal = paths.root().join("seal");
    let permissions = fs::metadata(&seal).unwrap().permissions();
    let original = fs::read(&seal).unwrap();
    let mut changed = original.clone();
    changed[0] ^= 1;
    fs::set_permissions(&seal, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&seal, changed).unwrap();
    fs::set_permissions(&seal, permissions.clone()).unwrap();
    assert!(
        matches!(
            stored.read_published_scoped_for_replay(
                &fixture.store,
                Data::KIND,
                Data::SCHEMA,
                node::content_digest::<Data>(&first.value().0),
                budget,
                node::read_set::Scope::Consumer,
            ),
            Err(crate::input::stored::Error::Bundle(
                node::Error::CandidateCorrupt(_)
            ))
        ),
        "consumer seal corruption retains exact retirement provenance"
    );
    fs::set_permissions(&seal, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&seal, original).unwrap();
    fs::set_permissions(&seal, permissions).unwrap();
    // The retained record length is measured against the
    // `input_proof_bytes` accounting target. Exactly the record admits
    // silently; one byte less records a truthful high water on the original
    // input account, without latching it, and the read still completes.
    const RECORD: &str = "retained resolution record bytes";
    let length = record_frame().len();
    for (target, exceeded) in [(length, false), (length - 1, true)] {
        let mut tiny = limits;
        tiny.input_proof_bytes = target;
        let mut tiny_account = record_account(graph);
        let mut tiny_budget = Budget::borrow(&tiny, &mut tiny_account, graph);
        let stored = Stored::decode(&first.value().0, &mut tiny_budget).unwrap();
        let observed = stored
            .read_published(
                &fixture.store,
                Data::KIND,
                Data::SCHEMA,
                node::content_digest::<Data>(&first.value().0),
                &mut tiny_budget,
            )
            .unwrap();
        assert_eq!(observed.bytes(), record_frame());
        drop(observed);
        assert!(tiny_budget.check().is_ok());
        let record = tiny_account.exceedances();
        match record.axis(RECORD) {
            Some(axis) => {
                assert!(exceeded, "an exact record length admits silently");
                assert_eq!(axis.target, target as u64);
                assert_eq!(axis.first_requested, length as u64);
                assert_eq!(axis.high_water, length as u64);
            }
            None => assert!(!exceeded, "one byte over the target is recorded"),
        }
    }
    drop((first, second));
    fixture.store.retire_fixture().unwrap();
    fs::remove_dir_all(fixture.root).unwrap();
}
