//! Run untrusted Rawr snippets for `?play`: a program runs, and its output
//! comes back as text, or an iced program's first settled frame as a PNG.
//!
//! [`run`] runs a snippet on the universe runner when the environment
//! names one (see [`universe`]), and else parses it once and runs it on the
//! base language (see [`plain`]). Every snippet takes that one path: one
//! that names a crate the runner doesn't have fails as any program would.
//!
//! Snippets are untrusted. Every run is on a fresh Engine with bounded
//! operations, call depth, data sizes, output and a wall-clock deadline,
//! and it reads no file.

pub mod budget;
#[cfg(feature = "bot")]
pub mod channels;
pub mod message;
pub mod plain;
#[cfg(feature = "bot")]
pub mod queue;
pub mod server;
#[cfg(feature = "bot")]
pub mod stats;
pub mod universe;
#[cfg(feature = "bot")]
pub mod web;

use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub use plain::Output;
pub use rawr_snippet::SOURCE_NAME;

/// Per-run resource limits for untrusted snippets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Maximum script operations.
    pub operations: u64,
    /// Maximum call depth; never raised above the engine's default.
    pub call_depth: usize,
    /// Maximum string length in bytes.
    pub string_size: usize,
    /// Maximum array length.
    pub array_size: usize,
    /// Maximum object-map size.
    pub map_size: usize,
    /// Wall-clock budget for one run.
    pub time: Duration,
    /// A file the bot may write a sooner deadline to as the run goes on
    /// (see [`rawr::play::deadline`]).
    pub deadline: Option<PathBuf>,
    /// The address space a run on the universe runner may have, in bytes
    /// (`RLIMIT_AS`, Linux only); `None` for no limit.
    pub memory: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            operations: 1_000_000,
            call_depth: 32,
            string_size: 64 * 1024,
            array_size: 64 * 1024,
            // One rule for every collection: a `HashMap` already counts
            // against the array limit, so maps get the same 65,536 items.
            map_size: 64 * 1024,
            time: Duration::from_secs(10),
            deadline: None,
            memory: None,
        }
    }
}

/// Why a snippet did not run: it did not check as Rawr.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlayError {
    /// The rendered diagnostics, against the snippet as written.
    Compile(String),
}

impl PlayError {
    /// The human-readable text, suitable for a reply code block.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Compile(text) => text,
        }
    }
}

impl fmt::Display for PlayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.text())
    }
}

impl std::error::Error for PlayError {}

/// Run one snippet: on the universe runner when the environment names one
/// (`universe::ROOT`, as the image does), and else on the base language.
pub fn run(source: &str, limits: &Limits) -> Result<Output, PlayError> {
    if let Some(universe) = universe::Universe::from_env() {
        return universe.run(source, limits);
    }
    let deadline =
        rawr::play::deadline::Watch::new(Instant::now() + limits.time, limits.deadline.clone());
    let parsed = plain::Parsed::new(source, limits, deadline)?;
    parsed.run(source, limits)
}

/// A report as the user's own source shows it (see [`rawr_snippet::report`]).
fn report(report: &rawr::diagnostic::Report, program: &str, source: &str) -> String {
    rawr_snippet::report(&rawr::diagnostic::render(report), program, source)
}

/// `report`, a run-time failure, as a Cargo-built program prints its panic.
fn panic(report: &rawr::diagnostic::Report, program: &str, source: &str) -> String {
    rawr_snippet::panic(&rawr::diagnostic::panic(report), program, source)
}

/// The limit an error's `text` reports, as a user reads it, or `None` when
/// it is no limit.
fn limit(text: &str, limits: &Limits) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    Some(
        if lower.contains(plain::TIME_LIMIT) || lower.contains("terminated") {
            plain::TIME_LIMIT.to_owned()
        } else if lower.contains("too many operations") {
            format!(
                "operation limit exceeded: more than {} operations (an endless loop?)",
                limits.operations
            )
        } else if lower.contains("stack overflow") {
            format!(
                "call depth limit exceeded: deeper than {} calls (endless recursion?)",
                limits.call_depth
            )
        } else if lower.contains("too large") {
            // A position here counts in the wrapped program, so it is dropped.
            let what = rawr_snippet::without_positions(text.rsplit(": ").next().unwrap_or(text));
            format!(
                "size limit exceeded: {what} (strings up to {} bytes, arrays and maps up to {} items)",
                limits.string_size, limits.array_size
            )
        } else {
            return None;
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{run, Limits, PlayError};

    fn text(source: &str) -> super::Output {
        let output = run(source, &Limits::default()).unwrap();
        assert!(output.frame.is_none(), "{source} drew a frame");
        output
    }

    #[test]
    fn maps_and_arrays_share_one_size_limit() {
        let limits = Limits::default();
        assert_eq!((limits.array_size, limits.map_size), (65_536, 65_536));
        let reason = super::limit("Size of object map too large", &limits).unwrap();
        assert!(
            reason.contains("arrays and maps up to 65536 items"),
            "{reason}"
        );
    }

    /// A snippet runs on a server whose files, environment and process are
    /// not its own (D683.4): `std::fs` is refused as the operating system
    /// refuses, and `env!` finds no variable.
    #[test]
    fn run_plain_refuses_files_and_the_environment() {
        let output = text(
            "fn main() {\n    println!(\"{:?}\", std::fs::read_to_string(\"/etc/hosts\").map_err(|e| e.kind()));\n    println!(\"{:?}\", std::fs::write(\"/tmp/rawr-play\", \"x\").map_err(|e| e.kind()));\n    println!(\"{:?}\", option_env!(\"PATH\"));\n}\n",
        );
        assert_eq!(
            output.text(),
            "Err(PermissionDenied)\nErr(PermissionDenied)\nNone\n"
        );
        assert!(!std::path::Path::new("/tmp/rawr-play").exists());
    }

    #[test]
    fn run_plain_program_prints_its_output() {
        let cart = text(include_str!("../../examples/scripts/cart.rs"));
        assert_eq!(cart.text(), "total: 27\n");
        assert_eq!((cart.status, cart.truncated, cart.limit), (0, false, false));
    }

    #[test]
    fn run_plain_program_interleaves_streams_in_order() {
        use super::plain::Stream::{Stderr, Stdout};
        let output = text("fn main() {\n    println!(\"a\");\n    eprintln!(\"b\");\n    print!(\"c\");\n    println!(\"d\");\n}\n");
        assert_eq!(
            output.chunks,
            [
                (Stdout, "a\n".into()),
                (Stderr, "b\n".into()),
                (Stdout, "cd\n".into())
            ]
        );
        let json = output.to_json();
        assert!(json.starts_with('{'), "{json}");
        assert_eq!(super::Output::from_json(json.as_bytes()).unwrap(), output);
    }

    #[test]
    fn run_plain_main_returning_err_exits_1() {
        let output = text("fn main() -> Result<(), String> {\n    println!(\"before\");\n    Err(\"bad\".to_string())\n}\n");
        assert_eq!(output.status, 1);
        assert_eq!(output.text(), "before\nError: \"bad\"\n");
        assert!(!output.limit);
    }

    #[test]
    fn run_plain_runtime_errors_point_at_the_users_text() {
        // A Vec's index panics at its `[`, as rustc places it.
        let output = text("let a = vec![1,2,3]; let i = 5; println!(\"{}\", a[i]);");
        let shown = output.text();
        assert_eq!(
            shown,
            format!(
                "thread 'main' panicked at play.rs:1:49:\n\
                 index out of bounds: the len is 3 but the index is 5\n{}\n",
                rawr::diagnostic::BACKTRACE_NOTE
            )
        );
        let output = text("let x: u8 = 255; let y = x + 1; y");
        let shown = output.text();
        assert!(
            shown.contains("panicked at play.rs:1:26:\nattempt to add with overflow\n"),
            "{shown}"
        );
        let limited = text("let mut s = String::new(); loop { s.push_str(\"xxxxxxxxxxxxxxxx\"); }");
        assert!(
            limited.limit && !limited.text().contains("(line "),
            "{}",
            limited.text()
        );
        // The wrapping's own text is never a position in the user's.
        let program = super::plain::program("1 + 2");
        assert!(!rawr_snippet::in_source(&program, "1 + 2", 1, 3));
        assert!(rawr_snippet::in_source(&program, "1 + 2", 1, 25));
        assert!(!rawr_snippet::in_source(&program, "1 + 2", 3, 1));
    }

    #[test]
    fn an_array_index_panics_at_the_expression_start_and_a_vec_index_at_its_bracket() {
        // An index rustc can't see at compile time, which it would refuse.
        let i = "let i: usize = \"5\".parse().unwrap(); ";
        // Each line with the column `cargo run` prints for it.
        let cases = [
            ("let a = [1, 2, 3]; a[i]", 57),
            ("let a = [1, 2, 3]; let r = &a; r[i]", 69),
            ("let mut a = [1, 2, 3]; a[i] = 0;", 61),
            ("let mut a = [1, 2, 3]; a[i] += 1;", 61),
            ("let g = [[1, 2, 3]; 3]; g[1][i]", 62),
            ("let v = vec![[1, 2, 3]]; v[0][i]", 63),
            ("let v = vec![1, 2, 3]; v[i]", 62),
            ("let mut v = vec![1, 2, 3]; v[i] = 0;", 66),
        ];
        let shown = cases
            .iter()
            .map(|(line, _)| {
                let text = text(&format!("{i}{line}")).text().to_owned();
                text.lines().next().unwrap_or_default().to_owned()
            })
            .collect::<Vec<_>>();
        let expected = cases
            .iter()
            .map(|(_, column)| format!("thread 'main' panicked at play.rs:1:{column}:"))
            .collect::<Vec<_>>();
        assert_eq!(shown, expected);
    }

    #[test]
    fn run_plain_expression_prints_its_value() {
        assert_eq!(text("1 + 2").text(), "3\n");
        assert_eq!(text("vec![1, 2, 3].iter().sum::<i64>()").text(), "6\n");
        assert_eq!(text("let a = 20;\na + 7").text(), "27\n");
        assert_eq!(text("println!(\"hi\")").text(), "hi\n");
        // As in a Rust block: a final `;` leaves no value to print.
        assert_eq!(text("1 + 2;").text(), "");
        assert_eq!(text("let a = 2;\n{ let b = a * 3; b }").text(), "6\n");
        assert_eq!(text("\"a\".to_string() + \"b\"").text(), "\"ab\"\n");
    }

    #[test]
    fn run_plain_panic_exits_101_like_rust() {
        let output = text("println!(\"before\");\nassert_eq!(1 + 1, 3);");
        assert_eq!(output.status, 101);
        assert!(!output.limit);
        let text = output.text();
        assert_eq!(
            text,
            format!(
                "before\nthread 'main' panicked at play.rs:2:1:\n\
                 assertion `left == right` failed\n  left: 2\n right: 3\n{}\n",
                rawr::diagnostic::BACKTRACE_NOTE
            )
        );
    }

    #[test]
    fn run_plain_statement_errors_keep_their_positions() {
        let PlayError::Compile(text) = run("let x: i64 = \"s\";", &Limits::default()).unwrap_err();
        assert!(text.contains("play.rs:1:14"), "{text}");
        assert!(text.contains(" 1 | let x: i64 = \"s\";"), "{text}");
        let PlayError::Compile(text) = run("let a = 1;\na + true", &Limits::default()).unwrap_err();
        assert!(text.contains("play.rs:2:"), "{text}");
    }

    #[test]
    fn run_plain_endless_loop_hits_the_time_limit() {
        let limits = Limits {
            operations: u64::MAX,
            time: std::time::Duration::from_millis(100),
            ..Limits::default()
        };
        let source =
            "fn main() {\n    println!(\"start\");\n    let mut n = 0;\n    loop { n += 1; }\n}\n";
        let output = run(source, &limits).unwrap();
        assert_eq!((output.status, output.limit), (1, true));
        assert_eq!(output.text(), "start\ntime limit exceeded\n");
    }

    #[test]
    fn run_plain_output_is_cut_and_the_program_stopped() {
        let limits = Limits {
            operations: u64::MAX,
            time: std::time::Duration::from_secs(20),
            ..Limits::default()
        };
        let source = "fn main() {\n    loop { println!(\"0123456789abcdef\"); }\n}\n";
        let started = std::time::Instant::now();
        let output = run(source, &limits).unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "stopped once full"
        );
        assert!(output.truncated && output.limit && output.status == 1);
        let text = output.text();
        assert!(
            text.len() <= super::plain::MAX_OUTPUT + 64,
            "{}",
            text.len()
        );
        assert!(
            text.ends_with("… output truncated\n"),
            "{}",
            &text[text.len() - 40..]
        );
    }

    #[test]
    fn run_plain_compile_error_is_a_diagnostic() {
        let PlayError::Compile(text) = run(
            "fn main() {\n    let x: i64 = \"s\";\n}\n",
            &Limits::default(),
        )
        .unwrap_err();
        assert!(text.contains("play.rs:2:"), "{text}");
    }

    #[test]
    fn run_plain_sandbox_refuses_escapes() {
        for body in ["eval(\"40 + 2\")", "import \"x\" as m; x", "sleep(100); x"] {
            let source = format!("fn leak(x) {{ {body} }}\nfn main() {{ leak(1); }}\n");
            let refused = match run(&source, &Limits::default()) {
                Err(_) => true,
                Ok(output) => output.status != 0,
            };
            assert!(refused, "{body}");
        }
    }

    #[test]
    fn unknown_function_in_a_snippet_is_an_error_at_its_name() {
        let PlayError::Compile(error) = run("nonexistent(1)", &Limits::default()).unwrap_err();
        assert!(error.contains("cannot find function `nonexistent`"), "{error}");
        assert!(error.contains("play.rs:1:1-1:12"), "{error}");
    }

    // Discord feedback: snippets as users posted them, with no `fn main`.

    #[test]
    fn play_final_iterator_is_not_printed_and_runs_no_stage() {
        for source in [
            r#"[0, 1, 2, 3].iter().map(|idx| println!("It's me! {idx}"))"#,
            r#"[0, 1, 2, 3].into_iter().map(|idx| println!("It's me! {idx}"))"#,
        ] {
            let output = text(source);
            assert_eq!(output.text(), "", "{source}: {output:?}");
            assert_eq!(output.status, 0);
            assert_eq!(output.note.as_deref(), Some(super::plain::UNUSED_ITERATOR));
        }
    }

    #[test]
    fn play_final_value_without_debug_is_not_printed() {
        let output = text("let f = |x: i64| x + 1; f");
        assert_eq!(
            (output.text().as_str(), output.status, output.note),
            ("", 0, None)
        );
    }

    #[test]
    fn play_programs_see_their_name_and_no_files() {
        let output = text(
            r#"fn main() {
    let args: Vec<String> = std::env::args().collect();
    println!("{args:?}");
    match std::fs::read_to_string("/etc/hosts") {
        Ok(text) => println!("read {}", text.len()),
        Err(error) => println!("{error}"),
    }
}"#,
        );
        assert_eq!(
            (output.text().as_str(), output.status),
            ("[\"play\"]\n?play has no files\n", 0)
        );
    }

    #[test]
    fn play_endless_loop_runs_to_a_limit() {
        let source = r#"let mut iteration = 0; loop { iteration += 1; println!("Iteration No{iteration}"); }"#;
        let output = text(source);
        assert!(output.limit && output.truncated, "{output:?}");
        assert!(output.text().starts_with("Iteration No1\nIteration No2\n"));
        assert!(output
            .text()
            .ends_with(&format!("\n{}\n", super::plain::TRUNCATED)));
        assert!(!output.text().contains("Debug"));
    }

    #[test]
    fn play_errors_never_show_the_wrapper() {
        for source in [
            "let x = 1; x +",
            "let x = (1 + 2;",
            "[1, 2].iter().map(|x| x.nope())",
        ] {
            let error = run(source, &Limits::default()).unwrap_err();
            let text = error.to_string();
            for wrapper in ["__v", "__d", "fn main", "play.rawr"] {
                assert!(!text.contains(wrapper), "{source}: {text}");
            }
            assert!(text.contains(" --> play.rs:1:"), "{source}: {text}");
            assert!(
                text.contains(&format!(" 1 | {source}\n")),
                "{source}: {text}"
            );
        }
    }

    /// A plain program with inline modules runs as the same program
    /// without them does. rustc prints `6 2.25` for it.
    #[test]
    fn plain_snippets_take_inline_modules() {
        let plain = text(
            "mod geometry {\n    pub(crate) fn area(w: f64, h: f64) -> f64 {\n        w * h\n    }\n\n    \
             pub mod shapes {\n        pub struct Square(pub f64);\n\n        impl Square {\n            \
             pub fn area(&self) -> f64 {\n                super::area(self.0, self.0)\n            }\n        \
             }\n    }\n}\n\nuse geometry::shapes::Square;\n\nfn main() {\n    let a = geometry::area(2.0, 3.0);\n    \
             println!(\"{a} {}\", Square(1.5).area());\n}\n",
        );
        assert_eq!(plain.text(), "6 2.25\n");
        assert_eq!(plain.status, 0);
        // A file module is still refused: a snippet has no other file.
        let PlayError::Compile(refused) =
            run("mod util;\nfn main() {}\n", &Limits::default()).unwrap_err();
        assert!(refused.contains("file modules require"), "{refused}");
    }
}
