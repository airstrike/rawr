//! The direct Package command retains its fixed config and original accounts.

use std::ffi::OsString;
use std::path::Path;

use super::super::{environment, Fixture};
use super::built::fresh_steps;
use super::{bind_universe, documents};
use crate::cargo::{self, config, resolution::layout::Layout, unit, Limits, Request};
use crate::input::{build::Build, package::Set};
use crate::process::{self, test_launch, ToolPaths as _};

fn launch(
    request: &Request,
    native: &process::Native,
    layout: &Layout,
    manifest: &Path,
) -> test_launch::Step {
    let mut prefix = Vec::<OsString>::new();
    for path in request.config.explicit_configs() {
        prefix.extend(["--config".into(), path.as_os_str().to_owned()]);
    }
    prefix.extend([
        "--config".into(),
        layout.final_form().config().as_os_str().to_owned(),
    ]);
    let tail = vec![
        "--list".into(),
        "--allow-dirty".into(),
        "--manifest-path".into(),
        manifest.as_os_str().to_owned(),
    ];
    let executable = native
        .production()
        .executable(process::Program::Cargo)
        .to_path_buf();
    let environment = environment(request, native, layout.final_form().target());
    let directory = request.project_root.clone();
    test_launch::Step::new(
        move |_| {
            test_launch::Expected::new(
                executable,
                process::Command::cargo(
                    prefix,
                    process::CargoAction::PackagePath,
                    tail,
                    directory,
                    environment,
                )
                .expect("pinned human Package command seals"),
                Limits::standard().process,
            )
        },
        |_| test_launch::Reply::stdout(b"Cargo.toml\nsrc/lib.rs\n".to_vec()),
    )
}

#[test]
fn direct_package_uses_reserved_config_and_exact_cumulative_prefix_admission() {
    for floor in [0, 1] {
        let mut fixture = Fixture::open();
        let request = &mut fixture.request;
        Fixture::reserve_parts(&fixture.context, &fixture.store, |layout, native| {
            let limits = Limits::standard();
            let cached = cargo::CachedToolchain {
                context: fixture.context.clone(),
                attestation: fixture
                    .context
                    .cached_toolchain()
                    .expect("actual fixture attestation"),
            };
            let (metadata, units) = documents(request, layout.final_form());
            bind_universe(request, &cached);
            let mut steps = fresh_steps(request, native, layout, &metadata, &units);
            let dependency = request
                .dependencies
                .iter()
                .find(|dependency| dependency.alias() == "catalog")
                .expect("the request selects catalog");
            let config::Source::Path(root) = dependency.source() else {
                panic!("the fixture selects an actual Path source");
            };
            // The request account is a target. Both the exact cumulative
            // prefix admission and one byte short launch the Package child.
            steps.push(launch(request, native, layout, &root.join("Cargo.toml")));
            let mut allocation = request.allocation;
            super::super::script(layout, steps)
                .run(|| -> crate::Result<()> {
                    let harness = request.prepare_fresh_harness_in(
                        layout,
                        &cached,
                        &limits,
                        &limits,
                        &mut allocation,
                        unit::Allocation::begin(&limits),
                    )?;
                    let selected = harness
                        .selected
                        .iter()
                        .find(|selected| selected.alias.as_ref() == "catalog")
                        .expect("the parsed harness selects catalog");
                    let package = harness
                        .metadata
                        .package(&selected.package_id)
                        .expect("exact parsed package authority exists");
                    let build = Build::parse(
                        b"{\"reason\":\"build-finished\",\"success\":true}\n",
                        environment(request, native, layout.final_form().target()),
                        harness.toolchain.cargo_commit(),
                        &limits,
                    )?;
                    let usage = build.usage();
                    let mut measured = allocation;
                    drop(super::super::super::prefix(
                        request,
                        layout.final_form().config(),
                        &mut measured,
                    )?);
                    let cost = measured.bytes - allocation.bytes;
                    allocation.bytes(allocation.byte_limit - allocation.bytes - cost + floor)?;
                    assert!(!allocation.exceedances().is_exceeded());
                    let files = cargo::cargo_package_files(
                        &native.production(),
                        None,
                        request,
                        package,
                        package.manifest(),
                        layout.final_form().target(),
                        harness.toolchain.cargo_commit(),
                        usage,
                        &limits,
                        &mut allocation,
                        Some(layout),
                    )?;
                    assert!(matches!(files, Set::Path(_)));
                    assert_eq!(files.base_usage(), usage);
                    assert_ne!(files.usage(), usage);
                    let record = allocation.exceedances();
                    if floor == 0 {
                        assert_eq!(allocation.bytes, allocation.byte_limit);
                        assert!(
                            !record.is_exceeded(),
                            "an exact cumulative prefix admission records nothing"
                        );
                    } else {
                        // One byte short: the reserved prefix's final charge
                        // crosses by exactly that byte, is admitted in full, and
                        // nothing after the prefix charges the account.
                        let limit = allocation.byte_limit as u64;
                        let axis = record
                            .axis(cargo::REQUEST_BYTES_RESOURCE)
                            .expect("one short records the exact prefix total");
                        assert_eq!(
                            (axis.target, axis.first_requested, axis.high_water),
                            (limit, limit + 1, limit + 1)
                        );
                        assert_eq!(allocation.bytes, allocation.byte_limit + 1);
                    }
                    Ok(())
                })
                .expect("Package runs directly under the already-held owner")?;
            Ok(())
        });
        fixture.retire();
    }
}
