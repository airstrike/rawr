// Replays a parity script through iced_test's emulator and records how it ends.
//
// scripts/iced-parity.mjs writes this file, the example's `program()` and its
// `ice_` test into `src/parity_emulator.rs` of a copy of the example. The test
// prints its outcome as an `iced-parity` line, so the harness can compare what
// `cargo test` and `rawr test` saw. The final screenshot is drawn by the
// headless renderer the harness picks through `ICED_TEST_BACKEND`.
//
// `replay!` is a macro, not a function generic over `Program`, so the program's
// concrete type is in scope and no projection like `P::State` is written:
// `rawr test` 0.3.214 reports those as unknown members.

use iced_test::emulator;
use iced_test::futures::futures::channel::mpsc;
use iced_test::program::Program;
use iced_test::Ice;

use std::thread;
use std::time;

/// How a replay ended.
enum Outcome {
    /// Every instruction ran. `screenshot` hashes the final frame's pixels.
    Passed { screenshot: u64 },
    /// The instruction found no widget to act on, or no expected text.
    Failed { instruction: String },
}

// `replay!` builds an outcome through these, and its locals are named apart
// from them: `rawr test` 0.3.214 renames a field or path segment in a macro
// body that shares its name with one of the body's `let` bindings, so
// `Outcome::Passed { screenshot: .. }` beside `let screenshot` became
// `Outcome::__rawr_macro2_screenshot`.
impl Outcome {
    /// A replay that ran every instruction and drew `rgba` last.
    fn passed(rgba: &[u8]) -> Self {
        Outcome::Passed {
            screenshot: fnv1a(rgba),
        }
    }

    /// A replay that stopped at `instruction`.
    fn failed(instruction: String) -> Self {
        Outcome::Failed { instruction }
    }

    /// Prints the outcome as one line the harness reads from the test output,
    /// which it runs with `--nocapture`. Stdout, not a file the environment
    /// names: `rawr test` 0.3.214 doesn't run `std::env::var` or `var_os`.
    fn record(&self, test: &str) {
        match self {
            Outcome::Passed { screenshot } => {
                println!("iced-parity {test} passed {screenshot:016x}")
            }
            Outcome::Failed { instruction } => println!("iced-parity {test} failed {instruction}"),
        }
    }

    /// Fails the test when the replay failed, after the outcome is recorded.
    fn assert_passed(&self) {
        if let Outcome::Failed { instruction } = self {
            panic!("`{instruction}` found nothing to act on or expect");
        }
    }
}

/// Parses a parity script, which the harness only writes valid.
fn parse(script: &str) -> Ice {
    match Ice::parse(script) {
        Ok(ice) => ice,
        Err(error) => panic!("the parity script is valid .ice: {error:?}"),
    }
}

/// Runs a script against a fresh emulator of a program, as `iced_test::run`
/// runs one `.ice` file, then screenshots the final state. With `preset`, the
/// emulator boots from the program's preset of that name.
macro_rules! replay {
    ($program:expr, $script:expr) => {
        replay!(@run $program, None, $script)
    };
    ($program:expr, preset $name:literal, $script:expr) => {{
        let program = &$program;
        let preset = match program.presets().iter().find(|preset| preset.name() == $name) {
            Some(preset) => preset,
            None => panic!("the parity script names a preset the program has: {}", $name),
        };
        replay!(@run $program, Some(preset), $script)
    }};
    (@run $program:expr, $preset:expr, $script:expr) => {{
        let program = &$program;
        let ice = parse($script);
        let (sender, mut receiver) = mpsc::channel(1);
        let mut emulator =
            iced_test::Emulator::with_preset(sender, program, ice.mode, ice.viewport, $preset);
        let mut instructions = ice.instructions.iter();
        let mut stopped = None;

        while stopped.is_none() {
            // Polls as `iced_test::screenshot` does. `rawr test` 0.3.214 does
            // not see `StreamExt::next`'s `Next` as a future, so
            // `executor::block_on(receiver.next())` doesn't build there.
            let Ok(event) = receiver.try_recv() else {
                thread::sleep(time::Duration::from_millis(1));
                continue;
            };

            match event {
                emulator::Event::Action(action) => emulator.perform(program, action),
                emulator::Event::Failed(instruction) => stopped = Some(instruction.to_string()),
                emulator::Event::Ready => match instructions.next() {
                    Some(instruction) => emulator.run(program, instruction),
                    None => break,
                },
            }
        }

        // The outcome is bound before it ends the block: `rawr test` 0.3.214
        // reports E0597 for the screenshot's borrows in a tail expression,
        // whose temporaries the 2024 edition drops before the block's locals.
        let outcome = match stopped {
            Some(instruction) => Outcome::failed(instruction),
            None => {
                // A program without its own theme draws with
                // `theme::Base::default(theme::Mode::None)`, which is
                // `Theme::Light` while ICED_THEME is unset, as the harness
                // keeps it. The value is written out because `rawr test`
                // 0.3.214 resolves that call to `theme::Mode::default`, and
                // refuses `<iced::Theme as theme::Base>::default` as a
                // qualified call through a trait the crate doesn't declare.
                let drawn = match emulator.theme(program) {
                    Some(own) => own,
                    None => iced_test::core::Theme::Light,
                };
                Outcome::passed(&emulator.screenshot(program, &drawn, 1.0).rgba)
            }
        };
        outcome
    }};
}

/// FNV-1a over the bytes: the same pixels give the same hash under any
/// toolchain or interpreter, unlike std's unspecified default hasher.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
