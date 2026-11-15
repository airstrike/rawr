// Helpers of the `sim_` tests, which replay a parity script on iced_test
// simulators of the example's view.
//
// scripts/iced-parity.mjs writes this file and the tests into the example's
// `src/parity.rs`. A test prints an `iced-parity` line with its outcome and
// leaves its final frame's hash in
// `target/iced-parity/snapshots/<test>-<renderer>.sha256`, where the harness
// reads it after each runner.

/// Ends a replay at the instruction that found nothing to act on or expect.
fn stop(test: &str, instruction: &str) -> ! {
    println!("iced-parity {test} failed {instruction}");
    panic!("`{instruction}` found nothing to act on or expect");
}
