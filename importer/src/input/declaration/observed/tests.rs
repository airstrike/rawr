use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{acquire, read};
use crate::cargo::{self, toolchain};
use crate::input::build;
use crate::input::declaration::Declarations;
use crate::input::stored::{self, observe, Budget};
use crate::input::{Allocation, Usage};
use crate::node;

const PACKAGE: &str = "path+file:///fixture#package@0.1.0";
const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

struct Fixture {
    root: PathBuf,
    out: PathBuf,
    output: PathBuf,
    build: build::Retained,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-saved-observation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("build/package-hash/out")).expect("fixture layout");
        let root = fs::canonicalize(root).expect("canonical fixture");
        let out = root.join("build/package-hash/out");
        let output = root.join("build/package-hash/output");
        let build = retained(&root, &out);
        Self {
            root,
            out,
            output,
            build,
        }
    }

    fn write(&self, bytes: &[u8]) {
        fs::write(&self.output, bytes).expect("saved output");
    }

    fn observe(
        &self,
        allocation: &mut Allocation,
        graph: &node::GraphBudget,
    ) -> stored::Result<Declarations> {
        read(
            PACKAGE,
            &self.out,
            &self.build,
            &self.root,
            &mut Budget::borrow(&cargo::Limits::standard(), allocation, graph),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn retained(root: &Path, out: &Path) -> build::Retained {
    build::Retained {
        build: build::Build {
            evidence: build::Evidence {
                usage: Usage {
                    items: 123,
                    bytes: 456,
                },
                packages: BTreeMap::from([(
                    PACKAGE.into(),
                    build::Package {
                        manifest: root.join("Cargo.toml"),
                        root: root.into(),
                        build_script: Some(root.join("build.rs")),
                    },
                )])
                .into(),
                scripts: Box::from([build::Script {
                    compiler: None,
                    package_id: PACKAGE.into(),
                    out_dir: out.into(),
                    cfg: Box::from([]),
                    environment: Box::from([]),
                }]),
                artifacts: Box::from([]),
                environment: Box::from([]),
            },
            package_files: BTreeMap::new(),
        },
    }
}

fn account() -> Allocation {
    Allocation::for_proof(PRIOR, &cargo::Limits::standard())
}

fn same(actual: &Declarations, expected: &Declarations) {
    assert_eq!(actual.package_id, expected.package_id);
    assert_eq!(actual.output, expected.output);
    assert_eq!(actual.directives, expected.directives);
    assert_eq!(actual.paths, expected.paths);
    assert_eq!(actual.environment, expected.environment);
}

#[test]
fn actual_observation_matches_legacy_empty_ordered_duplicate_and_opaque_data() {
    let fixture = Fixture::new();
    for bytes in [
        &b""[..],
        &b"ignored\ncargo:rerun-if-changed=src\ncargo::rerun-if-env-changed=KEY\ncargo:rerun-if-changed=src\ncargo:rustc-cfg=value\r\ncargo::opaque=raw\0data\n"[..],
    ] {
        fixture.write(bytes);
        let actual = fixture.observe(&mut account(), &node::GraphBudget::new()).expect("current data");
        let expected = Declarations::read_accounted(
            toolchain::supported_cargo_commit(),
            PACKAGE,
            &fixture.out,
            &mut account(),
        ).expect("legacy parser data");
        same(&actual, &expected);
        fs::remove_file(&fixture.output).expect("remove source after acquisition");
        same(&actual, &expected);
    }
}

#[test]
fn exact_package_and_native_out_dir_binding_precede_io_and_ownership() {
    let fixture = Fixture::new();
    // No saved-output file exists. Even with exhausted graph accounts, every
    // association mismatch must reject before a filesystem or storage attempt.
    let graph = node::GraphBudget::new();
    for (package, out) in [
        ("other-package", fixture.out.clone()),
        (PACKAGE, fixture.root.join("build/other-hash/out")),
        (PACKAGE, fixture.out.join(".")),
    ] {
        let mut allocation = account();
        let result = read(
            package,
            &out,
            &fixture.build,
            &fixture.root,
            &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
        );
        assert!(matches!(
            result,
            Err(stored::Error::Observation(observe::Error::Policy(
                "saved-output Build execution is not associated"
            )))
        ));
        assert_eq!(allocation.usage(), PRIOR);
        allocation
            .items(0)
            .expect("semantic mismatch did not refuse account");
    }
    let mut unassociated = retained(&fixture.root, &fixture.out);
    unassociated.build.evidence.packages = Default::default();
    let mut allocation = account();
    assert!(matches!(
        read(
            PACKAGE,
            &fixture.out,
            &unassociated,
            &fixture.root,
            &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
        ),
        Err(stored::Error::Observation(observe::Error::Policy(_)))
    ));
    assert_eq!(allocation.usage(), PRIOR);
}

#[test]
fn bound_execution_still_requires_existing_output_and_canonical_layout() {
    let fixture = Fixture::new();
    assert!(matches!(
        fixture.observe(&mut account(), &node::GraphBudget::new()),
        Err(stored::Error::Observation(observe::Error::Io { .. }))
    ));
    // The saved output is read through a link, as Cargo reads it.
    let elsewhere = fixture.root.join("elsewhere");
    fs::write(&elsewhere, b"cargo:x=y\n").expect("other file");
    std::os::unix::fs::symlink(&elsewhere, &fixture.output).expect("output symlink");
    let linked = fixture
        .observe(&mut account(), &node::GraphBudget::new())
        .expect("an output through a link is its target");
    assert_eq!(linked.output, elsewhere);
    fs::remove_file(&fixture.output).expect("remove output symlink");
    fixture.write(b"");
    // An OUT_DIR that resolves outside Cargo's build layout is not one.
    let original = fixture.out.with_file_name("old-out");
    fs::rename(&fixture.out, &original).expect("rename OUT_DIR");
    std::os::unix::fs::symlink(&original, &fixture.out).expect("OUT_DIR symlink");
    assert!(matches!(
        fixture.observe(&mut account(), &node::GraphBudget::new()),
        Err(stored::Error::Observation(observe::Error::Policy(
            "saved-output-layout"
        )))
    ));

    let wrong = fixture.root.join("wrong");
    fs::create_dir(&wrong).expect("wrong layout directory");
    assert!(matches!(
        read(
            PACKAGE,
            &wrong,
            &retained(&fixture.root, &wrong),
            &fixture.root,
            &mut Budget::borrow(
                &cargo::Limits::standard(),
                &mut account(),
                &node::GraphBudget::new()
            ),
        ),
        Err(stored::Error::Observation(observe::Error::Policy(
            "saved-output-layout"
        )))
    ));
}

#[test]
fn shorter_opened_saved_output_is_allowed_and_growth_above_initial_length_refuses() {
    let fixture = Fixture::new();
    for bytes in [&b"cargo:x=y\n"[..], &b""[..]] {
        fixture.write(b"cargo:rustc-cfg=long-initial-value\n");
        let mut allocation = account();
        let actual = acquire(
            PACKAGE,
            &fixture.out,
            &fixture.build,
            &fixture.root,
            &mut Budget::borrow(
                &cargo::Limits::standard(),
                &mut allocation,
                &node::GraphBudget::new(),
            ),
            |output| {
                fs::write(output, bytes).expect("shorten after pathname metadata");
                Ok(())
            },
        )
        .expect("shorter opened source is legacy-supported");
        assert_eq!(actual.directive_count(), usize::from(!bytes.is_empty()));
    }
    fixture.write(b"x\n");
    let mut allocation = account();
    let result = acquire(
        PACKAGE,
        &fixture.out,
        &fixture.build,
        &fixture.root,
        &mut Budget::borrow(
            &cargo::Limits::standard(),
            &mut allocation,
            &node::GraphBudget::new(),
        ),
        |output| {
            fs::write(output, b"cargo:x=y\n").expect("grow after pathname metadata");
            Ok(())
        },
    );
    // The buffer was admitted at the pathname-metadata length; a source that
    // grew past it is a physical read bound, not an accounting target:
    // it still refuses with a physical `Error::Limit`, latches the account and
    // records no exceedance.
    assert!(matches!(
        result,
        Err(stored::Error::Input(crate::Error::Limit {
            resource: "freshness file",
            limit: 2,
            actual: 10,
        }))
    ));
}

#[test]
fn actual_builder_preserves_completion_utf8_and_late_grammar_order() {
    let fixture = Fixture::new();
    for (bytes, tag) in [
        (&b"cargo:x=y\n\xff"[..], "truncated-saved-output"),
        (&b"cargo:x=y\n\xff\n"[..], "saved-output-utf8"),
        (&b"cargo:rerun-if-changed=\n"[..], "malformed-saved-output"),
        (
            &b"cargo:x=y\ncargo:rerun-if-env-changed=A=B\n"[..],
            "malformed-saved-output",
        ),
    ] {
        fixture.write(bytes);
        let mut allocation = account();
        let result = fixture.observe(&mut allocation, &node::GraphBudget::new());
        assert!(
            matches!(result, Err(stored::Error::Observation(observe::Error::Policy(actual))) if actual == tag)
        );
        allocation
            .items(0)
            .expect("grammar failure stays distinct from refusal");
    }
    fixture.write(b"cargo:rerun-if-changed=\n");
    let mut full = account();
    let _ = fixture.observe(&mut full, &node::GraphBudget::new());
}

#[test]
fn complete_growth_and_boxing_obey_prior_original_account_and_refusal() {
    let fixture = Fixture::new();
    fixture.write(
        b"cargo:rerun-if-changed=a\ncargo:rerun-if-changed=b\ncargo:rerun-if-env-changed=KEY\n",
    );
    let mut allocation = account();
    fixture
        .observe(&mut allocation, &node::GraphBudget::new())
        .expect("measure actual ownership");
    let usage = allocation.usage();
    assert!(usage.items > PRIOR.items && usage.bytes > PRIOR.bytes);
    // A genuine failure latches the original account, and a latched account
    // does no further work.
    allocation.refuse();
    assert!(matches!(
        fixture.observe(&mut allocation, &node::GraphBudget::new()),
        Err(stored::Error::Refused)
    ));
    assert_eq!(allocation.usage(), usage);
    assert!(matches!(
        allocation.bytes(0),
        Err(crate::Error::InputAccountRefused)
    ));
}

#[test]
fn graph_charges_only_actual_observations_not_retained_metadata() {
    let fixture = Fixture::new();
    fixture.write(b"cargo:x=y\n");
    assert!(fixture.output.as_os_str().len() < 384, "stack CStr fixture");
    // Retained metadata (package id, path copies, canonical outputs, directive
    // text) is owned once by the original allocation; the graph's decode axis
    // measures decoding, not copies of admitted bytes.
    for account_kind in 0..2 {
        for floor in [0, 1] {
            if account_kind == 1 && floor == 1 {
                // No decode charge exists here for a decode floor to refuse.
                continue;
            }
            let graph = node::GraphBudget::new();
            let mut allocation = account();
            let result = fixture.observe(&mut allocation, &graph);
            // Hashing and comparisons sit exactly at their targets: any such
            // charge by the observation would now record an exceedance.
            assert!(result.is_ok(), "graph targets are recorded, never refused");
        }
    }
}
