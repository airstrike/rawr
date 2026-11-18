//! libtest's harness for a crate's `#[test]` functions: the tests its
//! arguments select run one after another, as libtest runs them with
//! `--test-threads=1`, and the report is libtest's: `running N tests`, a
//! line for each test, each failure's output and the summary. What a test
//! prints is captured and shown only when it fails. `rawr test` runs it on
//! the base language, and a runner runs it for a program that imports Rust.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use super::panic::Output;
use super::{Expected, Test};
use crate::{diagnostic, Dynamic, Engine, EvalAltResult};

/// libtest's exit status when a test failed.
pub const FAILED: u8 = 101;

/// What the tests that run printed since the last test ended, when the
/// engines they run on [capture](capture) it.
static CAPTURED: Mutex<String> = Mutex::new(String::new());

/// What libtest's arguments ask: which tests run, and whether their output
/// is captured.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// A test runs when its name contains one of these, or any test when
    /// there are none.
    filters: Vec<String>,
    /// A test whose name contains one of these does not run.
    skip: Vec<String>,
    /// Filters match whole names.
    exact: bool,
    ignored: Ignored,
    /// `--nocapture`: tests print as they run.
    nocapture: bool,
    /// `--test-threads`: how many threads libtest would run tests on.
    threads: Option<usize>,
}

/// What becomes of the tests `#[ignore]` leaves out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Ignored {
    /// They are reported as ignored.
    #[default]
    Report,
    /// `--ignored`: only they run.
    Only,
    /// `--include-ignored`: they run with the rest.
    Include,
}

/// How one test went.
enum Verdict {
    Passed,
    /// What the test printed and why it failed, for the failures section.
    Failed(String),
}

/// Where the tests run: what calls a test's function and places its
/// failure in the crate's files.
pub trait Tests {
    /// Call `test`'s function. `Err` is its run-time failure, a panic.
    fn call(&mut self, test: &Test) -> Result<Dynamic, Box<EvalAltResult>>;

    /// The report of `error`, a test's run-time failure, at its place in
    /// the crate's files.
    fn located(&self, error: &EvalAltResult) -> diagnostic::Report;

    /// `path`, a file of the crate, as Cargo names it in a test's failure.
    fn named<'path>(&self, path: &'path Path) -> &'path Path;
}

impl Options {
    /// Read libtest's arguments: test name filters and its options.
    ///
    /// # Errors
    ///
    /// An option libtest does not have, or one missing its value.
    pub fn parse<'a>(arguments: impl IntoIterator<Item = &'a OsString>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut arguments = arguments
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned());
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--exact" => options.exact = true,
                "--ignored" => options.ignored = Ignored::Only,
                "--include-ignored" => options.ignored = Ignored::Include,
                "--nocapture" => options.nocapture = true,
                "--skip" => options
                    .skip
                    .push(arguments.next().ok_or("`--skip` needs a filter")?),
                "--test-threads" => {
                    let threads = arguments.next().ok_or("`--test-threads` needs a number")?;
                    options.threads = Some(threads_from(&threads)?);
                }
                option if option.starts_with("--skip=") => {
                    options.skip.push(option["--skip=".len()..].to_owned());
                }
                option if option.starts_with("--test-threads=") => {
                    options.threads = Some(threads_from(&option["--test-threads=".len()..])?);
                }
                option if option.starts_with('-') => {
                    return Err(format!("unknown argument `{option}`"));
                }
                _ => options.filters.push(argument),
            }
        }
        Ok(options)
    }

    /// Whether what a test prints is captured, shown only when it fails.
    #[must_use]
    pub const fn captures(&self) -> bool {
        !self.nocapture
    }

    /// Whether libtest would run tests on more than one thread, and so
    /// print each test's name with its result rather than before it runs.
    /// The harness runs them one after another on one thread either way.
    fn concurrent(&self) -> bool {
        let threads = self
            .threads
            .or_else(|| std::env::var("RUST_TEST_THREADS").ok()?.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from));
        threads > 1
    }

    /// Where a test's panics are printed.
    const fn output(&self) -> Output {
        if self.nocapture {
            Output::Printed
        } else {
            Output::Captured
        }
    }

    /// Whether `test` runs or is reported, as libtest's filters select.
    fn selects(&self, test: &Test) -> bool {
        let matches = |filter: &String| {
            if self.exact {
                test.name() == filter
            } else {
                test.name().contains(filter.as_str())
            }
        };
        (self.filters.is_empty() || self.filters.iter().any(matches))
            && !self.skip.iter().any(matches)
            && (self.ignored != Ignored::Only || test.ignored().is_some())
    }
}

/// Capture what programs `engine` runs print, for [`run`] to show with a
/// test's failure.
pub fn capture(engine: &mut Engine) {
    engine.on_output(|_, text| capture_text(text));
}

/// `threads`, `--test-threads`'s value, as libtest reads it.
fn threads_from(threads: &str) -> Result<usize, String> {
    threads
        .parse()
        .ok()
        .filter(|threads| *threads > 0)
        .ok_or_else(|| "argument for --test-threads must be a number > 0".to_owned())
}

/// Add `text` to what the running test printed.
pub(super) fn capture_text(text: &str) {
    CAPTURED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push_str(text);
}

/// What the tests printed since this was last asked.
fn captured() -> String {
    std::mem::take(
        &mut *CAPTURED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// Run those of `tests` that `options` selects in `place`, reporting as
/// libtest does. The status is libtest's: [`FAILED`] when a test failed.
pub fn run(tests: &[Test], options: &Options, place: &mut impl Tests) -> u8 {
    let selected: Vec<&Test> = tests.iter().filter(|test| options.selects(test)).collect();
    let filtered = tests.len() - selected.len();
    let plural = if selected.len() == 1 { "" } else { "s" };
    println!("\nrunning {} test{plural}", selected.len());
    let started = Instant::now();
    let (mut passed, mut ignored) = (0, 0);
    let mut failures = Vec::new();
    let concurrent = options.concurrent();
    // What the top level printed is no test's.
    captured();
    for test in selected {
        let label = match test.expected() {
            Expected::Return => test.name().to_owned(),
            Expected::Panic(_) => format!("{} - should panic", test.name()),
        };
        if let Some(reason) = test.ignored()
            && options.ignored == Ignored::Report
        {
            match reason {
                Some(reason) => println!("test {label} ... ignored, {reason}"),
                None => println!("test {label} ... ignored"),
            }
            ignored += 1;
            continue;
        }
        // libtest names a test before it runs on one thread, else with
        // its result.
        if !concurrent {
            print!("test {label} ... ");
            let _ = std::io::stdout().flush();
        }
        let finished = |status: &str| {
            if concurrent {
                println!("test {label} ... {status}");
            } else {
                println!("{status}");
            }
        };
        // A run-time failure, the program's panic, is printed while the
        // test still runs on this thread; a panic that unwinds out of the
        // call, from native code the test called, was printed as it
        // happened.
        let returned = super::panic::catch(test.name(), options.output(), || {
            place.call(test).map_err(|error| {
                let report = place.located(&error);
                super::panic::print(|thread| diagnostic::hooked(&report, thread));
                diagnostic::panic_message(&report)
            })
        });
        let panic = match returned {
            Ok(Ok(value)) => {
                if let Some(Err(error)) = value.try_cast::<Result<Dynamic, Dynamic>>() {
                    let mut output = captured();
                    output.push_str(&format!("Error: {error:?}\n"));
                    failures.push((test, output));
                    finished("FAILED");
                    continue;
                }
                None
            }
            Ok(Err(message)) | Err(message) => Some(message),
        };
        let mut output = captured();
        let verdict = match (test.expected(), panic) {
            (Expected::Return, None) | (Expected::Panic(None), Some(_)) => Verdict::Passed,
            (Expected::Return, Some(_)) => Verdict::Failed(output),
            (Expected::Panic(Some(expected)), Some(message)) => {
                if message.contains(&**expected) {
                    Verdict::Passed
                } else {
                    output.push_str(&format!(
                        "note: panic did not contain expected string\n      panic message: \
                         {message:?}\n expected substring: {expected:?}"
                    ));
                    Verdict::Failed(output)
                }
            }
            (Expected::Panic(_), None) => {
                let location = test.location();
                output.push_str(&format!(
                    "note: test did not panic as expected at {}:{}:{}",
                    place.named(location.path()).display(),
                    location.line(),
                    location.column()
                ));
                Verdict::Failed(output)
            }
        };
        match verdict {
            Verdict::Passed => {
                finished("ok");
                passed += 1;
            }
            Verdict::Failed(output) => {
                finished("FAILED");
                failures.push((test, output));
            }
        }
    }

    if !failures.is_empty() {
        println!("\nfailures:");
        let sections: String = failures
            .iter()
            .filter(|(_, output)| !output.is_empty())
            .map(|(test, output)| format!("---- {} stdout ----\n{output}\n", test.name()))
            .collect();
        if !sections.is_empty() {
            print!("\n{sections}");
        }
        println!("\nfailures:");
        for (test, _) in &failures {
            println!("    {}", test.name());
        }
    }
    let result = if failures.is_empty() { "ok" } else { "FAILED" };
    println!(
        "\ntest result: {result}. {passed} passed; {} failed; {ignored} ignored; 0 measured; \
         {filtered} filtered out; finished in {:.2}s\n",
        failures.len(),
        started.elapsed().as_secs_f64()
    );
    if failures.is_empty() {
        0
    } else {
        FAILED
    }
}
