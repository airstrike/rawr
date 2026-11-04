use crate::p3;
use crate::shared;

/// The generated crate on its own: its tests execute the direct and
/// arena-backed thunks, and the runner itself builds, which typechecks the
/// rooted adapter table outside `cfg(test)`.
#[test]
fn generated_closed_subset_executes_direct_and_arena_backed_thunks() {
    let project = shared::generated_crate(p3::generated(), "rawr-monomorphic-execution");
    let output = shared::cargo_on(&project, &["test"]);
    shared::assert_success(&output, "the generated runtime tests");
    let output = shared::cargo_on(&project, &["build"]);
    shared::assert_success(&output, "the generated crate's build");
}
