//! The real Fresh preparation continues through Build.

use std::fs;

use serde_json::{json, Value};

use super::super::{environment, expected_reply, Command, Fixture};
use super::{bind_universe, documents};
use crate::cargo::{self, resolution::layout, unit, Limits, Request};
use crate::input::build::Build;
use crate::process::{self, test_launch};
use crate::{Error, Result};

#[derive(Clone, Copy)]
enum Outcome {
    Success,
    Exceeded,
    BeforeBuildWorkspaceDrift,
    BuildWorkspaceDrift,
}

pub(super) fn fresh_steps(
    request: &Request,
    native: &process::Native,
    layout: &layout::Layout,
    metadata: &[u8],
    units: &[u8],
) -> Vec<test_launch::Step> {
    fresh_steps_with_lock(request, native, layout, metadata, units, None)
}

/// Like `fresh_steps`, but the unlocked final-form metadata child also writes
/// the given final lock, as Cargo does when it completes the seeded lock.
pub(super) fn fresh_steps_with_lock(
    request: &Request,
    native: &process::Native,
    layout: &layout::Layout,
    metadata: &[u8],
    units: &[u8],
    lock: Option<Vec<u8>>,
) -> Vec<test_launch::Step> {
    let (provisional, _) = documents(request, layout.provisional());
    let candidate = metadata.to_vec();
    let units = units.to_vec();
    let lock = lock.map(|bytes| {
        test_launch::File::new(
            layout
                .final_form()
                .lock()
                .strip_prefix(layout.root())
                .expect("final lock belongs to Attempt")
                .to_path_buf(),
            bytes,
        )
    });
    vec![
        expected_reply(
            request,
            native,
            layout.provisional(),
            Command::Metadata(false),
            true,
            move |_| test_launch::Reply::stdout(provisional),
        ),
        expected_reply(
            request,
            native,
            layout.final_form(),
            Command::Metadata(false),
            true,
            move |_| match lock {
                Some(lock) => test_launch::Reply::exit(0, candidate, Vec::new(), vec![lock]),
                None => test_launch::Reply::stdout(candidate),
            },
        ),
        expected_reply(
            request,
            native,
            layout.final_form(),
            Command::Units,
            true,
            move |_| test_launch::Reply::stdout(units),
        ),
    ]
}

pub(super) fn artifacts(
    layout: &layout::Layout,
    metadata: &[u8],
    target: &str,
) -> (Vec<u8>, Vec<test_launch::File>) {
    let metadata: Value = serde_json::from_slice(metadata).expect("fixture metadata parses");
    let directory = layout.final_form().build().join(target).join("debug/deps");
    let mut bytes = Vec::new();
    let mut files = Vec::new();
    for package in metadata["packages"]
        .as_array()
        .expect("fixture packages exist")
    {
        let name = package["targets"][0]["name"]
            .as_str()
            .expect("crate name exists");
        let artifact = directory.join(format!("lib{name}.rmeta"));
        let dep_info = directory.join(format!("{name}.d"));
        let dependency = format!(
            "{}: {}\n",
            artifact.display(),
            package["targets"][0]["src_path"]
                .as_str()
                .expect("source path exists")
        );
        for (path, contents) in [
            (&artifact, b"fixture rmeta\n".as_slice()),
            (&dep_info, dependency.as_bytes()),
        ] {
            files.push(test_launch::File::new(
                path.strip_prefix(layout.root())
                    .expect("output belongs to Attempt")
                    .to_path_buf(),
                contents.to_vec(),
            ));
        }
        let node = metadata["resolve"]["nodes"]
            .as_array()
            .expect("resolve nodes exist")
            .iter()
            .find(|node| node["id"] == package["id"])
            .expect("package node exists");
        let message = json!({"reason": "compiler-artifact", "package_id": package["id"],
            "manifest_path": package["manifest_path"], "target": package["targets"][0],
            "features": node["features"], "filenames": [artifact], "fresh": false,
            "profile": {"debug_assertions": true, "debuginfo": 2, "opt_level": "0",
                "overflow_checks": true, "test": false}});
        bytes.extend(serde_json::to_vec(&message).expect("artifact message encodes"));
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"{\"reason\":\"build-finished\",\"success\":true}\n");
    (bytes, files)
}

fn exercise(outcome: Outcome) {
    let mut fixture = Fixture::open();
    let ancestor = fixture.root.join("Cargo.toml");
    let request = &mut fixture.request;
    Fixture::reserve_parts(&fixture.context, &fixture.store, |layout, native| {
        let cached = cargo::CachedToolchain {
            context: fixture.context.clone(),
            attestation: fixture
                .context
                .cached_toolchain()
                .expect("current fixture attestation"),
        };
        let limits = Limits::standard();
        let mut cargo_limits = limits;
        cargo_limits.items -= 1;
        let (metadata, units) = documents(request, layout.final_form());
        bind_universe(request, &cached);
        let mut steps = fresh_steps(request, native, layout, &metadata, &units);
        let (build_bytes, files) = artifacts(layout, &metadata, &request.target);
        let reply_bytes = build_bytes.clone();
        let build_ancestor = ancestor.clone();
        if !matches!(outcome, Outcome::BeforeBuildWorkspaceDrift) {
            steps.push(expected_reply(
                request,
                native,
                layout.final_form(),
                Command::Build,
                true,
                move |_| {
                    if matches!(outcome, Outcome::BuildWorkspaceDrift) {
                        fs::write(build_ancestor, b"[workspace.package]\nversion='2.0.0'\n")
                            .unwrap();
                    }
                    test_launch::Reply::exit(0, reply_bytes, Vec::new(), files)
                },
            ));
        }
        let script = test_launch::Script::new(
            layout.root().to_path_buf(),
            test_launch::FileLimits {
                count: 8,
                bytes: 4096,
            },
            steps,
        )
        .expect("bounded script seals");
        let mut allocation = request.allocation;
        script
            .run_result(|| -> Result<()> {
                let harness = request.prepare_fresh_harness_in(
                    layout,
                    &cached,
                    &limits,
                    &cargo_limits,
                    &mut allocation,
                    unit::Allocation::begin(&cargo_limits),
                )?;
                let prepared_bytes = allocation.bytes;
                if matches!(outcome, Outcome::Exceeded) {
                    let mut measured = allocation;
                    harness
                        .workspace
                        .require(&mut crate::input::workspace::Account::borrow(
                            &mut measured,
                            cached.context.graph_budget(),
                            &limits,
                        ))?;
                    drop(super::super::super::prefix(
                        request,
                        layout.final_form().config(),
                        &mut measured,
                    )?);
                    let build_cost = measured.bytes - allocation.bytes;
                    // Leave one byte less than the measured Build-stage
                    // usage. The account is a target: Build still runs, and the
                    // stage's final charge is recorded crossing by exactly that byte.
                    allocation.bytes(allocation.byte_limit - allocation.bytes - build_cost + 1)?;
                    assert!(!allocation.exceedances().is_exceeded());
                }
                if matches!(outcome, Outcome::BeforeBuildWorkspaceDrift) {
                    fs::write(&ancestor, b"[workspace.package]\nversion='2.0.0'\n").unwrap();
                }
                let result = request.build_harness_in(
                    &cached,
                    layout,
                    harness,
                    &mut allocation,
                    &limits,
                    &cargo_limits,
                );
                match outcome {
                    Outcome::Success | Outcome::Exceeded => {
                        let built = result?;
                        assert_eq!(built.harness.harness_root, layout.final_form().root());
                        assert_eq!(
                            built.harness.extraction_plan.protocol_directory(),
                            layout.extraction().protocol()
                        );
                        assert!(built.harness.dag.is_none());
                        assert!(built.harness.universe.is_some());
                        assert!(built.allocation.bytes > prepared_bytes);
                        built.harness.cargo_allocation.require(&cargo_limits)?;
                        let expected = Build::parse(
                            &build_bytes,
                            environment(request, native, layout.final_form().target()),
                            cached.attestation.value().proof().cargo_commit(),
                            &limits,
                        )?;
                        assert_eq!(built.build.usage(), expected.usage());
                        assert!(!layout.extraction().plan().exists());
                        let record = built.allocation.exceedances();
                        if matches!(outcome, Outcome::Exceeded) {
                            // Prior fresh and Build usage share one original
                            // account: the Build stage's final charge crossed
                            // by exactly the withheld byte, and later charges
                            // kept accruing truthfully.
                            let limit = built.allocation.byte_limit as u64;
                            let axis = record
                                .axis(cargo::REQUEST_BYTES_RESOURCE)
                                .expect("the spent account records the Build-stage crossing");
                            assert_eq!((axis.target, axis.first_requested), (limit, limit + 1));
                            assert_eq!(axis.high_water, built.allocation.bytes as u64);
                            assert!(axis.high_water > axis.first_requested);
                        } else {
                            assert!(!record.is_exceeded());
                        }
                    }
                    Outcome::BeforeBuildWorkspaceDrift | Outcome::BuildWorkspaceDrift => {
                        let error = result
                            .err()
                            .expect("original workspace image rejects drift");
                        assert!(
                            matches!(
                                error,
                                Error::ResolutionDrift {
                                    command: "workspace input observation",
                                    ..
                                }
                            ),
                            "original error: {error:?}"
                        );
                        assert!(allocation.bytes > prepared_bytes);
                    }
                }
                assert!(fs::read(layout.extraction().source()).is_ok());
                Ok(())
            })
            .expect("only the ordered expected children occur")?;
        Ok(())
    });
    fixture.retire();
}

#[test]
fn four_commands_preserve_plan_and_both_original_accounts() {
    exercise(Outcome::Success);
}

#[test]
fn prior_fresh_and_build_usage_records_exceedance_at_the_build_stage() {
    exercise(Outcome::Exceeded);
}

#[test]
fn original_workspace_image_survives_actual_build_boundaries() {
    exercise(Outcome::BeforeBuildWorkspaceDrift);
    exercise(Outcome::BuildWorkspaceDrift);
}

#[test]
fn a_different_real_attestation_cannot_execute_the_prepared_harness() {
    let mut fixture = Fixture::open();
    let other = Fixture::open();
    let wrong = cargo::CachedToolchain {
        context: other.context.clone(),
        attestation: other
            .context
            .cached_toolchain()
            .expect("other current attestation"),
    };
    let request = &mut fixture.request;
    Fixture::reserve_parts(&fixture.context, &fixture.store, |layout, native| {
        let cached = cargo::CachedToolchain {
            context: fixture.context.clone(),
            attestation: fixture
                .context
                .cached_toolchain()
                .expect("current fixture attestation"),
        };
        let limits = Limits::standard();
        let (metadata, units) = documents(request, layout.final_form());
        bind_universe(request, &cached);
        let steps = fresh_steps(request, native, layout, &metadata, &units);
        let mut allocation = request.allocation;
        super::super::script(layout, steps)
            .run(|| -> Result<()> {
                let harness = request.prepare_fresh_harness_in(
                    layout,
                    &cached,
                    &limits,
                    &limits,
                    &mut allocation,
                    unit::Allocation::begin(&limits),
                )?;
                assert_ne!(&harness.toolchain, wrong.attestation.value().proof());
                let result = request.build_harness_in(
                    &wrong,
                    layout,
                    harness,
                    &mut allocation,
                    &limits,
                    &limits,
                );
                assert!(matches!(
                    result
                        .err()
                        .expect("different attestation rejects before Build"),
                    Error::ResolutionDrift {
                        command: "reserved Build preparation",
                        field: "prepared harness toolchain"
                    }
                ));
                Ok(())
            })
            .expect("a mismatched identity cannot launch Build")?;
        Ok(())
    });
    fixture.retire();
    drop(wrong);
    other.retire();
}
