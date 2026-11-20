//! A run: every chosen workload measured, written as one versioned JSON
//! result, and printed. A counters build counts; any other times.

use serde_json::{json, Value};
use std::path::PathBuf;

use crate::options::Run;
use crate::{machine, measure, workloads};

/// The result format's name and version. A change to the format changes
/// the version, and `compare` refuses two results of different versions.
pub const SCHEMA: &str = "rawr-lock-bench/2";

/// What this build measures.
const MODE: &str = if cfg!(feature = "counters") {
    "counters"
} else {
    "timed"
};

/// Measure what `run` asks and write its result.
pub fn run(run: &Run) {
    let names: Vec<&str> = match &run.only {
        None => workloads::NAMES.to_vec(),
        Some(only) => {
            for name in only {
                if !workloads::NAMES.contains(&name.as_str()) {
                    eprintln!(
                        "locks bench: this {MODE} build has no workload `{name}`; it has: {}",
                        workloads::NAMES.join(", ")
                    );
                    std::process::exit(2);
                }
            }
            // Measured in the bench's order, whatever order they were named in.
            let named: Vec<&str> = workloads::NAMES
                .iter()
                .copied()
                .filter(|name| only.iter().any(|wanted| wanted == name))
                .collect();
            named
        }
    };
    let started = machine::utc_now();
    let load_before = machine::load();
    let mut measured = Vec::new();
    for name in names {
        eprintln!("locks bench: {name}");
        let mut workload = workloads::prepare(name).expect("every name in NAMES prepares");
        let about = workload.about();
        let mut entry = json!({
            "name": about.name,
            "smells": about.smells,
            "run": about.run,
            "fixture_sha256": fixture_hash(workload.fixture()),
        });
        #[cfg(feature = "counters")]
        {
            entry["counted"] = measure::counted(&mut workload, run.runs);
        }
        #[cfg(not(feature = "counters"))]
        {
            entry["timed"] = measure::timed(&mut workload, run.samples);
        }
        measured.push(entry);
    }
    let build = machine::build();
    let result = json!({
        "schema": SCHEMA,
        "mode": MODE,
        "label": run.label,
        "started_utc": started,
        "finished_utc": machine::utc_now(),
        "build": build,
        "toolchain": machine::toolchain(),
        "machine": machine::machine(),
        "load": { "before": load_before, "after": machine::load() },
        "options": if cfg!(feature = "counters") {
            json!({ "runs": run.runs })
        } else {
            json!({ "samples": run.samples })
        },
        "workloads": measured,
    });
    print(&result);
    let path = run
        .output
        .clone()
        .unwrap_or_else(|| default_path(&started, &build));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the result's directory can be made");
    }
    let text = serde_json::to_string_pretty(&result).expect("a result is plain JSON");
    std::fs::write(&path, text + "\n").expect("the result can be written");
    println!("\nwrote {}", path.display());
}

/// Each workload's counts, one line per site with every run's count, or its
/// times; and what the program reported, run by run.
fn print(result: &Value) {
    for workload in result["workloads"].as_array().into_iter().flatten() {
        println!(
            "\n{} ({})",
            workload["name"].as_str().unwrap_or_default(),
            workload["smells"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let counted = &workload["counted"];
        let mut lines: Vec<(String, &Value)> = counted["sites"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(site, series)| (site.clone(), series))
            .collect();
        if !counted.is_null() {
            lines.push(("allocations".to_owned(), &counted["allocations"]));
            lines.push(("allocated bytes".to_owned(), &counted["allocated_bytes"]));
        }
        for (site, series) in lines {
            let mark = if series["stable"] == true { " " } else { "~" };
            println!("  {site:<16} {mark} {}", series["runs"]);
        }
        let samples = &workload["timed"]["samples"];
        if !samples.is_null() {
            let ms = |key: &str| samples[key].as_f64().unwrap_or_default() / 1e6;
            println!(
                "  time ms           min {:.3}  q1 {:.3}  median {:.3}  q3 {:.3}  max {:.3}",
                ms("min"),
                ms("q1"),
                ms("median"),
                ms("q3"),
                ms("max")
            );
        }
        let reported = if counted.is_null() {
            &workload["timed"]["reported"]
        } else {
            &counted["reported"]
        };
        for (name, series) in reported.as_object().into_iter().flatten() {
            println!(
                "  {name} ({}), median {}, by run:",
                series["unit"].as_str().unwrap_or_default(),
                series["median"]
            );
            for run in series["runs"].as_array().into_iter().flatten() {
                println!("    {run}");
            }
        }
    }
}

/// `target/lock-bench/<utc>-<revision>[-dirty]-<mode>.json`, in the target
/// directory this binary was built in.
fn default_path(started: &str, build: &Value) -> PathBuf {
    let revision = build["revision"]
        .as_str()
        .map_or("unknown", |rev| &rev[..rev.len().min(10)]);
    let dirty = if build["dirty"] == true { "-dirty" } else { "" };
    // The binary is target/<profile>/deps/locks-<hash>.
    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.ancestors().nth(3).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("target"));
    target
        .join("lock-bench")
        .join(format!("{started}-{revision}{dirty}-{MODE}.json"))
}

fn fixture_hash(fixture: &str) -> String {
    use sha2::Digest as _;
    machine::hex(&sha2::Sha256::digest(fixture.as_bytes()))
}
