//! Preparation through real reservations, without executing a Cargo child.

use std::fs;
use std::path::PathBuf;

use super::{Plan, ACTIVE_DIRECTORY, DIRECTORY, PROBE_SOURCE, TARGET_DIRECTORY};
use crate::cargo::{self, resolution, toolchain, unit};
use crate::context::Context;
use crate::node;
use crate::store::{self, bundle};
use crate::{Error, Result};

pub(super) struct Fixture {
    root: PathBuf,
    context: Context,
    store: store::Store,
    _project: store::TestProject,
}

impl Fixture {
    pub(super) fn open(label: &str) -> Self {
        let guard = store::TestProject::new(&format!("rawr-plan-preparation-{label}"));
        let root = guard.path().to_path_buf();
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
            toolchain::HOST,
            &cargo::Limits::standard(),
        )
        .expect("the original resolution context captures");
        toolchain::Attestation::fixture_current(&context, &root.join("toolchain"));
        let store = context.open_store().expect("the actual shared store opens");
        Self {
            root,
            context,
            store,
            _project: guard,
        }
    }

    pub(super) fn reserve<T>(
        &self,
        operation: impl FnOnce(&resolution::layout::Layout, &toolchain::Proof) -> Result<T>,
    ) -> std::result::Result<T, node::Error> {
        let toolchain = self
            .context
            .cached_toolchain()
            .expect("fixture proof is retained");
        let recipe = self
            .context
            .discover::<resolution::BuildProduct>(b"rawr.extraction.preparation-test/v1", 4096)?;
        // Exercise the actual owner, project lease, capacity reservation, and
        // Attempt cleanup. No fixture publishes a Product or current authority.
        self.store.with_ownership(|ownership| {
            self.store.with_heavy_lease(|| {
                self.store.with_execution_lease(recipe.execution(), || {
                    self.store.with_execution_bundle_reservation(
                        ownership,
                        &recipe,
                        None,
                        |attempt| {
                            let layout = resolution::layout::Layout::bind(
                                attempt,
                                self.store.graph_budget(),
                            )?;
                            operation(&layout, toolchain.value().proof())
                                .map(store::Reserved::Reuse)
                                .map_err(node::Error::from)
                        },
                    )
                })
            })
        })
    }

    pub(super) fn retire(self) {
        assert_eq!(self.store.fixture_usage().expect("ledger reads").3, 0);
        assert_eq!(
            bundle::count_bundles(self.store.root()).expect("bundles count"),
            0
        );
        self.store
            .fixture_physical_inventory()
            .expect("physical ledger agrees");
        self.store.retire_fixture().expect("fixture store retires");
        fs::remove_dir_all(self.root).expect("isolated fixture retires");
    }
}

fn output(crate_type: &str) -> unit::Output {
    unit::Output::fixture(
        "path+file:///fixture#dependency@0.1.0",
        "/fixture/libdependency.rlib",
        "dependency",
        "lib",
        crate_type,
    )
}

fn graph() -> unit::BuildGraph {
    unit::BuildGraph::fixture_artifacts(Box::from([output("rlib")]))
}

fn code(error: Error) -> &'static str {
    match error {
        Error::Extraction { code, .. } => code,
        error => panic!("expected extraction rejection, got {error:?}"),
    }
}

#[test]
fn reserved_preparation_preserves_outputs_and_uses_fixed_controls() {
    let fixture = Fixture::open("fixed");
    fixture
        .reserve(|layout, proof| {
            let sentinels = [
                layout.final_form().target().join("existing-artifact"),
                layout.provisional().target().join("existing-artifact"),
                layout.final_form().build().join("existing-build-output"),
                layout.provisional().build().join("existing-build-output"),
                layout.extraction().protocol().join("existing-sidecar"),
                layout.extraction().plan().to_path_buf(),
            ];
            for path in &sentinels {
                fs::write(path, b"retained output").expect("sentinel publishes");
            }
            let mut plan = Plan::prepare_in(
                layout,
                &graph(),
                proof,
                toolchain::HOST,
                &cargo::Limits::standard(),
            )?;
            assert_eq!(plan.target_directory(), layout.final_form().target());
            assert_eq!(plan.command_target()?, layout.final_form().target());
            assert_eq!(plan.artifact_directory, layout.final_form().build());
            assert_eq!(plan.protocol_directory(), layout.extraction().protocol());
            assert_eq!(plan.plan_file, layout.extraction().plan());
            assert_eq!(plan.probe_source, layout.extraction().source());
            assert_eq!(
                fs::read(&plan.probe_source).expect("probe reads"),
                PROBE_SOURCE
            );
            for path in &sentinels {
                assert_eq!(fs::read(path).expect("output survives"), b"retained output");
            }
            assert!(!layout.root().join(DIRECTORY).exists());
            assert!(!layout.root().join(TARGET_DIRECTORY).exists());
            Ok(())
        })
        .expect("the reserved attempt prepares");
    fixture.retire();
}

#[test]
fn legacy_refresh_and_reserved_preparation_share_exact_entries() {
    let fixture = Fixture::open("shared");
    let legacy = fixture.root.join("legacy");
    let active = legacy.join(DIRECTORY).join(ACTIVE_DIRECTORY);
    let target = legacy.join(TARGET_DIRECTORY);
    fs::create_dir_all(&active).expect("legacy controls create");
    fs::create_dir(&target).expect("legacy target creates");
    fs::write(active.join("stale-control"), b"stale").expect("old control publishes");
    fs::write(target.join("stale-artifact"), b"stale").expect("old artifact publishes");
    fixture
        .reserve(|layout, proof| {
            let graph = unit::BuildGraph::fixture_artifacts(Box::from([
                output("rlib"),
                unit::Output::fixture(
                    "path+file:///other#other@0.1.0",
                    "/other.rlib",
                    "other",
                    "lib",
                    "rlib",
                ),
            ]));
            let limits = cargo::Limits::standard();
            let legacy_plan = Plan::prepare(&legacy, &graph, proof, toolchain::HOST, &limits)?;
            let reserved = Plan::prepare_in(layout, &graph, proof, toolchain::HOST, &limits)?;
            assert_eq!(legacy_plan.artifact_directory, legacy_plan.target_directory);
            assert_eq!(reserved.artifact_directory, layout.final_form().build());
            assert!(!active.join("stale-control").exists());
            assert!(!target.join("stale-artifact").exists());
            assert_eq!(legacy_plan.entries, reserved.entries);
            assert_eq!(legacy_plan.compiler_commit, reserved.compiler_commit);
            assert_eq!(legacy_plan.host, reserved.host);
            assert_eq!(legacy_plan.selected_target, reserved.selected_target);
            assert_eq!(
                fs::read(legacy_plan.probe_source).expect("legacy probe reads"),
                PROBE_SOURCE
            );
            assert!(reserved.entries.windows(2).all(|pair| pair[0] < pair[1]));
            Ok(())
        })
        .expect("both preparations accept the same graph");
    fixture.retire();
}

#[test]
fn both_preparations_reject_repeated_entries_and_conflicting_correlations() {
    let fixture = Fixture::open("correlation");
    let legacy = fixture.root.join("legacy");
    fs::create_dir(&legacy).expect("legacy harness creates");
    for (second_type, expected) in [
        ("rlib", "ambiguous-package-unit-correlation"),
        ("dylib", "duplicate-unit-correlation"),
    ] {
        fixture
            .reserve(|layout, proof| {
                let graph = unit::BuildGraph::fixture_artifacts(Box::from([
                    output("rlib"),
                    output(second_type),
                ]));
                let limits = cargo::Limits::standard();
                let legacy_error = Plan::prepare(&legacy, &graph, proof, toolchain::HOST, &limits)
                    .err()
                    .expect("legacy preparation rejects ambiguity");
                let sentinel = layout.final_form().target().join("keep");
                fs::write(&sentinel, b"keep").expect("sentinel publishes");
                let reserved_error =
                    Plan::prepare_in(layout, &graph, proof, toolchain::HOST, &limits)
                        .err()
                        .expect("reserved preparation rejects ambiguity");
                assert_eq!(code(legacy_error), expected);
                assert_eq!(code(reserved_error), expected);
                assert_eq!(
                    fs::read(&sentinel).expect("target survives rejection"),
                    b"keep"
                );
                Ok(())
            })
            .expect("the rejected plan leaves reservation cleanup intact");
    }
    fixture.retire();
}

#[test]
fn missing_precreated_directories_are_never_recreated() {
    let fixture = Fixture::open("missing");
    for directory in ["protocol", "target", "build"] {
        fixture
            .reserve(|layout, proof| {
                let missing = match directory {
                    "protocol" => layout.extraction().protocol(),
                    "target" => layout.final_form().target(),
                    _ => layout.final_form().build(),
                };
                fs::remove_dir(missing).expect("owned directory removes");
                let error = Plan::prepare_in(
                    layout,
                    &graph(),
                    proof,
                    toolchain::HOST,
                    &cargo::Limits::standard(),
                )
                .err()
                .expect("missing reserved directory is rejected");
                assert!(matches!(error, Error::Io { source, .. }
                if source.kind() == std::io::ErrorKind::NotFound));
                assert!(!missing.exists());
                assert!(!layout.extraction().source().exists());
                Ok(())
            })
            .expect("missing directory rejection cleans the attempt");
    }
    fixture.retire();
}

#[test]
fn a_file_cannot_replace_a_precreated_protocol_directory() {
    let fixture = Fixture::open("file");
    fixture
        .reserve(|layout, proof| {
            let protocol = layout.extraction().protocol();
            fs::remove_dir(protocol).expect("owned directory removes");
            fs::write(protocol, b"file").expect("wrong shape publishes");
            let error = Plan::prepare_in(
                layout,
                &graph(),
                proof,
                toolchain::HOST,
                &cargo::Limits::standard(),
            )
            .err()
            .expect("a file does not supply the reserved directory");
            assert!(matches!(
                error,
                Error::Path {
                    field: "extraction protocol directory",
                    ..
                }
            ));
            assert_eq!(
                fs::read(protocol).expect("wrong shape is not cleared"),
                b"file"
            );
            assert!(!layout.extraction().source().exists());
            Ok(())
        })
        .expect("wrong directory shape cleans the attempt");
    fixture.retire();
}

#[cfg(unix)]
#[test]
fn escaped_directory_is_rejected_before_probe_publication() {
    let fixture = Fixture::open("escape");
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).expect("outside directory creates");
    fs::write(outside.join("keep"), b"outside").expect("outside sentinel publishes");
    fixture
        .reserve(|layout, proof| {
            fs::remove_dir_all(layout.extraction().root()).expect("owned controls remove");
            std::os::unix::fs::symlink(&outside, layout.extraction().root())
                .expect("escaping alias creates");
            let error = Plan::prepare_in(
                layout,
                &graph(),
                proof,
                toolchain::HOST,
                &cargo::Limits::standard(),
            )
            .err()
            .expect("canonical containment rejects the escaped root");
            assert!(matches!(
                error,
                Error::Path {
                    field: "extraction run directory",
                    ..
                }
            ));
            assert!(!outside.join("probe.rs").exists());
            assert_eq!(
                fs::read(outside.join("keep")).expect("outside remains"),
                b"outside"
            );
            Ok(())
        })
        .expect("escaped path refusal cleans the attempt");
    fixture.retire();
}

#[cfg(unix)]
#[test]
fn internal_directory_aliases_are_rejected_before_probe_publication() {
    let fixture = Fixture::open("internal-alias");
    for expected in [
        "extraction run directory",
        "extraction protocol directory",
        "authoritative Build target directory",
    ] {
        fixture
            .reserve(|layout, proof| {
                let assigned = match expected {
                    "extraction run directory" => layout.extraction().root(),
                    "extraction protocol directory" => layout.extraction().protocol(),
                    _ => layout.final_form().target(),
                };
                let redirected = assigned
                    .parent()
                    .expect("assigned directory has a parent")
                    .join("redirected-directory");
                fs::write(assigned.join("keep"), b"owned output").expect("sentinel publishes");
                fs::rename(assigned, &redirected).expect("directory moves within its attempt");
                std::os::unix::fs::symlink(&redirected, assigned).expect("internal alias creates");
                let error = Plan::prepare_in(
                    layout,
                    &graph(),
                    proof,
                    toolchain::HOST,
                    &cargo::Limits::standard(),
                )
                .err()
                .expect("a contained alias cannot change the assigned directory");
                assert!(
                    matches!(&error, Error::Path { field, reason, .. }
                    if *field == expected && reason.as_ref() == "must retain its exact reserved path"),
                    "unexpected rejection for {expected}: {error}"
                );
                assert!(!layout.extraction().source().exists());
                assert!(!redirected.join("probe.rs").exists());
                assert_eq!(
                    fs::read(redirected.join("keep")).expect("output survives"),
                    b"owned output"
                );
                Ok(())
            })
            .expect("internal alias rejection cleans the attempt");
    }
    fixture.retire();
}

#[test]
fn original_plan_limits_admit_exact_usage_and_record_one_less() {
    let fixture = Fixture::open("account");
    fixture
        .reserve(|layout, proof| {
            let graph = graph();
            let limits = cargo::Limits::standard();
            let baseline = Plan::prepare_in(layout, &graph, proof, toolchain::HOST, &limits)?;
            let (items, bytes) = baseline.allocation.usage();
            drop(baseline);
            let mut exact = limits;
            exact.canonical_items = items;
            exact.canonical_bytes = bytes;
            let admitted = Plan::prepare_in(layout, &graph, proof, toolchain::HOST, &exact)?;
            assert_eq!(admitted.allocation.usage(), (items, bytes));
            assert!(!admitted.allocation.exceedances().is_exceeded());
            drop(admitted);
            // One less than the original cumulative charge prepares the
            // same plan and records only the crossed axis, at its floor target
            // and the truthful cumulative high water.
            for (item_floor, expected, other) in [
                (
                    true,
                    "canonical allocation items",
                    "canonical allocation bytes",
                ),
                (
                    false,
                    "canonical allocation bytes",
                    "canonical allocation items",
                ),
            ] {
                let mut floor = exact;
                if item_floor {
                    floor.canonical_items -= 1;
                } else {
                    floor.canonical_bytes -= 1;
                }
                let recorded = Plan::prepare_in(layout, &graph, proof, toolchain::HOST, &floor)
                    .expect("one less than the original cumulative charge records");
                assert_eq!(recorded.allocation.usage(), (items, bytes));
                assert!(!recorded.allocation.is_refused());
                let axis = *recorded
                    .allocation
                    .exceedances()
                    .axis(expected)
                    .expect("the crossed axis records");
                let (target, high_water) = if item_floor {
                    (items - 1, items)
                } else {
                    (bytes - 1, bytes)
                };
                assert_eq!(axis.target, target as u64);
                assert_eq!(axis.high_water, high_water as u64);
                assert!(recorded.allocation.exceedances().axis(other).is_none());
                drop(recorded);
            }
            Ok(())
        })
        .expect("prospective limits remain the caller's limits");
    fixture.retire();
}

#[test]
fn empty_graph_refusal_does_not_publish_controls_and_a_zero_byte_account_records() {
    let fixture = Fixture::open("early");
    fixture
        .reserve(|layout, proof| {
            let empty = unit::BuildGraph::fixture_artifacts(Box::default());
            let limits = cargo::Limits::standard();
            let error = Plan::prepare_in(layout, &empty, proof, toolchain::HOST, &limits)
                .err()
                .expect("empty extraction graph rejects");
            assert_eq!(code(error), "missing-extraction-unit");
            assert!(!layout.extraction().source().exists());
            assert!(!layout.extraction().plan().exists());
            // The original zero-byte account is an accounting target. The
            // plan prepares, publishes its controls, and records the byte axis
            // at its zero target and truthful cumulative high water.
            //
            // `Plan::prepare_in` constructs its own `Allocation::new`, so no
            // latched account can be injected here; the terminal "does not
            // publish controls" half of this proof is the account's own
            // `Allocation::refused()` ordering tests.
            let mut recorded = limits;
            recorded.canonical_bytes = 0;
            let plan = Plan::prepare_in(layout, &graph(), proof, toolchain::HOST, &recorded)
                .expect("a zero-byte account records and still prepares");
            assert!(layout.extraction().source().exists());
            let (_, bytes) = plan.allocation.usage();
            assert!(bytes > 0);
            assert!(!plan.allocation.is_refused());
            let axis = *plan
                .allocation
                .exceedances()
                .axis("canonical allocation bytes")
                .expect("the byte axis records");
            assert_eq!(axis.target, 0);
            assert_eq!(axis.high_water, bytes as u64);
            assert!(plan
                .allocation
                .exceedances()
                .axis("canonical allocation items")
                .is_none());
            Ok(())
        })
        .expect("early rejection cleans the reservation");
    fixture.retire();
}
