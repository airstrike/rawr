//! The differential harness (D572.8): rawr's own frontend verdict on each
//! program of `tests/borrowck`, beside the pinned rustc's verdict on the
//! same file (each corpus folder's `rustc.tsv`, written by `record.py`). The rawr
//! side is the frontend alone: `rawr check` runs rustc too, which would
//! hide rawr's misses.
//!
//! - A false refusal is a borrow error rawr reports that rustc does not
//!   report with that code. There must be none.
//! - A miss is a program rustc rejects with one of these borrow errors that
//!   rawr accepts. Within what the checker models there must be none: a
//!   body whose facts note something they do not model yet is left to
//!   `cargo check` (D572.5).
//! - A program both accept must print what rustc's build prints.

use crate::{Dynamic, Engine, Scope};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// What a frontend made of a program: it lowered it, it reported a borrow
/// error with rustc's code on a line, or it stopped at something else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Verdict {
    Accepted,
    Borrow {
        code: String,
        line: usize,
    },
    Other {
        code: String,
        line: usize,
        message: String,
    },
}

/// Programs both accept that rawr runs differently from rustc's build for
/// reasons other than borrow checking, each an open item, by the start of
/// their path (a case's mutations share its name).
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[
    (
        "patterns/holder_subject_disjoint",
        "a binding in the default mode of a match on a `&mut` local is not written back",
    ),
    (
        "patterns/mutations/holder_subject_disjoint_",
        "a binding in the default mode of a match on a `&mut` local is not written back",
    ),
    (
        "patterns/nested_option_ref",
        "a `ref mut` binding below a tuple member in `if let` is not written back",
    ),
    (
        "patterns/mutations/nested_option_ref_",
        "a `ref mut` binding below a tuple member in `if let` is not written back",
    ),
    (
        "ui/nll/issue-54556-niconii.rs",
        "a field holding `&T` of a type with `Drop` drops a copy of the value with its struct",
    ),
    (
        "ui/nll/issue-54556-temps-in-tail-diagnostic.rs",
        "a field holding `&T` of a type with `Drop` drops a copy of the value with its struct",
    ),
];

/// The borrow errors this checker reports, with rustc's codes.
pub(super) const BORROW_CODES: &[&str] = &[
    "E0499", "E0502", "E0503", "E0505", "E0506", "E0515", "E0597", "E0716",
];

fn load(path: &Path) -> Result<crate::package::Graph, Verdict> {
    let target = crate::project::Target::discover(path).map_err(|error| Verdict::Other {
        code: "load".into(),
        line: 0,
        message: error.to_string(),
    })?;
    crate::package::Graph::load(&Engine::new(), &target).map_err(|error| verdict(&error))
}

fn verdict(error: &crate::package::Error) -> Verdict {
    let (code, line) = match error.report() {
        Ok(report) => (
            report.code().as_str().to_string(),
            report
                .labels()
                .first()
                .and_then(|label| label.span().start().line())
                .unwrap_or(0),
        ),
        Err(_) => ("unreported".to_string(), 0),
    };
    if BORROW_CODES.contains(&code.as_str()) {
        Verdict::Borrow { code, line }
    } else {
        Verdict::Other {
            code,
            line,
            message: error.to_string(),
        }
    }
}

/// rawr's frontend verdict on the program at `path`: compiled, not run.
pub(super) fn frontend(path: &Path) -> Verdict {
    let graph = match load(path) {
        Ok(graph) => graph,
        Err(verdict) => return verdict,
    };
    match Engine::new().compile_package(graph) {
        Ok(_) => Verdict::Accepted,
        Err(error) => verdict(&error),
    }
}

/// What the program at `path` prints when rawr runs it, or why it stopped.
fn run(path: &Path) -> Result<String, String> {
    let graph = load(path).map_err(|verdict| format!("{verdict:?}"))?;
    let printed = Arc::new(Mutex::new(String::new()));
    let mut engine = Engine::new();
    let sink = Arc::clone(&printed);
    engine.on_output(move |output, text| {
        if matches!(output, crate::Output::Stdout) {
            sink.lock()
                .expect("an unpoisoned print buffer")
                .push_str(text);
        }
    });
    // A program that runs away under rawr is a difference too.
    engine.set_max_operations(10_000_000);
    let (ast, main) = engine
        .compile_package(graph)
        .map_err(|error| error.to_string())?;
    let mut scope = Scope::new();
    let _: Dynamic = engine
        .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
        .map_err(|error| error.to_string())?;
    if let Some(main) = main {
        let _: Dynamic = engine
            .call_fn_with_options::<Dynamic>(
                crate::CallFnOptions::new()
                    .eval_ast(false)
                    .rewind_scope(false),
                &mut scope,
                &ast,
                main,
                (),
            )
            .map_err(|error| error.to_string())?;
    }
    let printed = printed.lock().expect("an unpoisoned print buffer").clone();
    Ok(printed)
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/borrowck")
}

/// rustc's verdict on a program: its errors (code and line), or what its
/// build printed.
enum Recorded {
    Rejected(Vec<(String, usize)>),
    Accepted(Option<String>),
}

fn recorded() -> Vec<(String, Recorded)> {
    let mut folders = std::fs::read_dir(corpus())
        .expect("the borrow corpus")
        .map(|entry| entry.expect("a corpus entry").path().join("rustc.tsv"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    folders.sort();
    let text = folders
        .iter()
        .map(|path| std::fs::read_to_string(path).expect("the recorded verdicts"))
        .collect::<String>();
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let path = fields.next().expect("a path").to_string();
            let verdict = fields.next().expect("a verdict");
            let recorded = if verdict == "ok" {
                let status = fields.next().unwrap_or("lib");
                let output = fields
                    .next()
                    .and_then(|output| serde_json::from_str::<String>(output).ok());
                Recorded::Accepted((status == "0").then_some(output).flatten())
            } else {
                Recorded::Rejected(
                    verdict
                        .split(' ')
                        .map(|error| {
                            let (code, line) = error.split_once(':').unwrap_or((error, "0"));
                            (code.to_string(), line.parse().unwrap_or(0))
                        })
                        .collect(),
                )
            };
            (path, recorded)
        })
        .collect()
}

#[test]
fn borrowck_differential() {
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut failures = Vec::new();
    for (path, rustc) in recorded() {
        let file = corpus().join(&path);
        super::UNMODELED.with(|notes| notes.borrow_mut().clear());
        let rawr = frontend(&file);
        let unmodeled = super::UNMODELED.with(|notes| !notes.borrow().is_empty());
        let outcome = match (&rawr, &rustc) {
            (Verdict::Other { .. }, _) => "not lowered by rawr",
            (Verdict::Borrow { code, line }, Recorded::Rejected(errors)) => {
                if errors.iter().any(|(c, l)| c == code && l == line) {
                    "both reject: same code and line"
                } else if errors.iter().any(|(c, _)| c == code) {
                    "both reject: same code, rustc's on another line"
                } else {
                    failures.push(format!(
                        "{path}: rawr reports {code} at line {line}, rustc {errors:?}"
                    ));
                    "false refusal"
                }
            }
            (Verdict::Borrow { code, line }, Recorded::Accepted(_)) => {
                failures.push(format!(
                    "{path}: rawr reports {code} at line {line}, rustc accepts it"
                ));
                "false refusal"
            }
            (Verdict::Accepted, Recorded::Rejected(errors)) => {
                if !errors
                    .iter()
                    .any(|(code, _)| BORROW_CODES.contains(&code.as_str()))
                {
                    "rustc rejects for errors other than these borrow errors"
                } else if unmodeled {
                    "miss outside what the checker models (left to cargo check)"
                } else {
                    failures.push(format!("{path}: rawr accepts it, rustc reports {errors:?}"));
                    "miss within the model"
                }
            }
            // A library, or a build that did not exit cleanly, is not run.
            (Verdict::Accepted, Recorded::Accepted(None)) => "both accept (not run)",
            (Verdict::Accepted, Recorded::Accepted(Some(expected))) => match run(&file) {
                Ok(printed) if *expected == printed => "both accept, same output",
                _ if KNOWN_DIFFERENCES
                    .iter()
                    .any(|(known, _)| path.starts_with(known)) =>
                {
                    "both accept, different output for a known reason"
                }
                printed => {
                    failures.push(format!(
                        "{path}: rustc's build prints {expected:?}, rawr's run {printed:?}"
                    ));
                    "both accept, different output"
                }
            },
        };
        *counts.entry(outcome).or_default() += 1;
    }
    let summary = counts
        .iter()
        .map(|(outcome, count)| format!("{count:5}  {outcome}"))
        .collect::<Vec<_>>()
        .join("\n");
    println!("borrow corpus:\n{summary}");
    assert!(failures.is_empty(), "{summary}\n\n{}", failures.join("\n"));
}

/// A probe over a directory of programs (`RAWR_BORROWCK_PROBE`), writing
/// each one's frontend verdict to `RAWR_BORROWCK_PROBE_OUT`: what picks the
/// rustc UI cases rawr lowers.
#[test]
#[ignore = "a probe run by hand over rustc's UI tests"]
fn borrowck_probe() {
    let Ok(directory) = std::env::var("RAWR_BORROWCK_PROBE") else {
        return;
    };
    let out = std::env::var("RAWR_BORROWCK_PROBE_OUT").expect("an output path");
    let mut files = Vec::new();
    let mut stack = vec![PathBuf::from(directory)];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("a readable directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut lines = String::new();
    for path in files {
        super::UNMODELED.with(|notes| notes.borrow_mut().clear());
        let verdict = std::panic::catch_unwind(|| frontend(&path)).unwrap_or(Verdict::Other {
            code: "panic".into(),
            line: 0,
            message: String::new(),
        });
        let unmodeled = super::UNMODELED.with(|notes| {
            notes
                .borrow()
                .iter()
                .map(|(what, span)| format!("{}@{}", what, span.start().line().unwrap_or(0)))
                .collect::<Vec<_>>()
                .join(";")
        });
        let row = match verdict {
            Verdict::Accepted if !unmodeled.is_empty() => format!("ok\tunmodeled\t{unmodeled}"),
            Verdict::Accepted => "ok".to_string(),
            Verdict::Borrow { code, line } => format!("borrow\t{code}\t{line}"),
            Verdict::Other {
                code,
                line,
                message,
            } => format!(
                "other\t{code}\t{line}\t{}",
                message.lines().next().unwrap_or("").replace('\t', " ")
            ),
        };
        lines.push_str(&format!("{}\t{row}\n", path.display()));
    }
    std::fs::write(out, lines).expect("a writable output");
}
