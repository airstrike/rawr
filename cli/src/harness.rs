//! `rawr test`: a crate's `#[test]` functions, built as `cargo test`
//! builds them (`cfg(test)` holding) and run one after another, as libtest
//! runs them with `--test-threads=1`. The report is libtest's: `running N
//! tests`, a line for each test, each failure's output and the summary.
//! Each test's printed output is captured and shown only when it fails.

use std::ffi::OsString;
use std::io::Write as _;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rawr::package::test::Expected;
use rawr::package::Test;
use rawr::{diagnostic, CallFnOptions, Dynamic, Scope};

use crate::base;

/// libtest's exit status when a test failed.
const FAILED: u8 = 101;

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
                // Tests run one after another, as with one thread.
                "--test-threads" => {
                    arguments.next().ok_or("`--test-threads` needs a number")?;
                }
                option if option.starts_with("--skip=") => {
                    options.skip.push(option["--skip=".len()..].to_owned());
                }
                option if option.starts_with("--test-threads=") => {}
                option if option.starts_with('-') => {
                    return Err(format!("unknown argument `{option}`"));
                }
                _ => options.filters.push(argument),
            }
        }
        Ok(options)
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

/// Build the loaded program for its tests and run those `options`
/// selects, reporting as libtest does. The exit status is libtest's: 101
/// when a test failed.
#[must_use]
pub fn run(loaded: base::Loaded, options: &Options) -> ExitCode {
    let base::Loaded { target, graph, .. } = loaded;
    let mut engine = base::engine(base::Build::Test);
    let captured = Arc::new(Mutex::new(String::new()));
    if !options.nocapture {
        let captured = Arc::clone(&captured);
        engine.on_output(move |_, text| {
            captured
                .lock()
                .expect("a test's output is never held across a panic")
                .push_str(text);
        });
    }
    let (ast, tests, positions) = match engine.compile_package_tests(graph) {
        Ok(compiled) => compiled,
        Err(error) => return crate::failure(&base::compile_report(&error, &target)),
    };
    let located = |error| base::located(error, &target, &positions);
    let mut scope = Scope::new();
    if let Err(error) = engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast) {
        eprint!("{}", diagnostic::panic(&located(error)));
        return ExitCode::from(FAILED);
    }

    let selected: Vec<&Test> = tests.iter().filter(|test| options.selects(test)).collect();
    let filtered = tests.len() - selected.len();
    let plural = if selected.len() == 1 { "" } else { "s" };
    println!("\nrunning {} test{plural}", selected.len());
    let started = Instant::now();
    let (mut passed, mut ignored) = (0, 0);
    let mut failures = Vec::new();
    // Rust's panic hook notes backtraces after a process's first panic.
    let mut noted = false;
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
        print!("test {label} ... ");
        let _ = std::io::stdout().flush();
        let returned = engine.call_fn_with_options::<Dynamic>(
            CallFnOptions::new().eval_ast(false).rewind_scope(true),
            &mut scope,
            &ast,
            test.function(),
            (),
        );
        let mut output = std::mem::take(
            &mut *captured
                .lock()
                .expect("a test's output is never held across a panic"),
        );
        let panic = match returned {
            Ok(value) => {
                if let Some(Err(error)) = value.try_cast::<Result<Dynamic, Dynamic>>() {
                    output.push_str(&format!("Error: {error:?}\n"));
                    failures.push((test, output));
                    println!("FAILED");
                    continue;
                }
                None
            }
            Err(error) => {
                let report = located(error);
                let message = diagnostic::panic_message(&report);
                let mut text = format!("\n{}", diagnostic::panicked(&report, test.name()));
                if !std::mem::replace(&mut noted, true) {
                    text.push_str(&format!("{}\n", diagnostic::BACKTRACE_NOTE));
                }
                Some((message, text))
            }
        };
        let printed = |output: &mut String, text: &str| {
            if options.nocapture {
                eprint!("{text}");
            } else {
                output.push_str(text);
            }
        };
        let verdict = match (test.expected(), panic) {
            (Expected::Return, None) | (Expected::Panic(None), Some(_)) => Verdict::Passed,
            (Expected::Return, Some((_, text))) => {
                printed(&mut output, &text);
                Verdict::Failed(output)
            }
            (Expected::Panic(Some(expected)), Some((message, text))) => {
                if message.contains(&**expected) {
                    Verdict::Passed
                } else {
                    printed(&mut output, &text);
                    output.push_str(&format!(
                        "note: panic did not contain expected string\n      panic message: \
                         {message:?}\n expected substring: {expected:?}"
                    ));
                    Verdict::Failed(output)
                }
            }
            (Expected::Panic(_), None) => {
                let location = test.location();
                let path = base::cargo_path(location.path(), &target).unwrap_or(location.path());
                output.push_str(&format!(
                    "note: test did not panic as expected at {}:{}:{}",
                    path.display(),
                    location.line(),
                    location.column()
                ));
                Verdict::Failed(output)
            }
        };
        match verdict {
            Verdict::Passed => {
                println!("ok");
                passed += 1;
            }
            Verdict::Failed(output) => {
                println!("FAILED");
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
        ExitCode::SUCCESS
    } else {
        ExitCode::from(FAILED)
    }
}
