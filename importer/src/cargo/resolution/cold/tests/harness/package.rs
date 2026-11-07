//! A path package's fallback inputs are its tree, read with no command.

use super::super::{environment, Fixture};
use super::built::fresh_steps;
use super::{bind_universe, documents};
use crate::cargo::{self, config, unit, Limits};
use crate::input::{build::Build, package::Set};

#[test]
fn a_path_package_falls_back_to_its_tree_without_a_package_command() {
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
        let steps = fresh_steps(request, native, layout, &metadata, &units);
        let dependency = request
            .dependencies
            .iter()
            .find(|dependency| dependency.alias() == "catalog")
            .expect("the request selects catalog");
        let config::Source::Path(_) = dependency.source() else {
            panic!("the fixture selects an actual Path source");
        };
        super::super::script(layout, steps)
            .run(|| -> crate::Result<()> {
                let harness = request.prepare_fresh_harness_in(
                    layout,
                    &cached,
                    &limits,
                    &limits,
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
                let files = cargo::fallback_files(
                    request,
                    package,
                    package.manifest(),
                    harness.toolchain.cargo_commit(),
                    usage,
                    &limits,
                )?;
                assert!(matches!(files, Set::Tree(_)));
                assert_eq!(files.base_usage(), usage);
                assert_ne!(files.usage(), usage);
                Ok(())
            })
            .expect("the fallback set needs no further command")?;
        Ok(())
    });
    fixture.retire();
}
