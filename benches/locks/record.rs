//! A run: every chosen workload measured, written as one versioned JSON
//! result, and its counts printed.

use serde_json::{json, Value};
use std::path::PathBuf;

use crate::options::Run;
use crate::{machine, measure, workloads};

/// The result format's name and version. A change to the format changes
/// the version, and `compare` refuses two results of different versions.
pub const SCHEMA: &str = "rawr-lock-bench/1";

/// Measure what `run` asks and write its result.
pub fn run(run: &Run) {
    let names: Vec<&str> = match &run.only {
        None => workloads::NAMES.to_vec(),
        Some(only) => {
            for name in only {
                if !workloads::NAMES.contains(&name.as_str()) {
                    eprintln!(
                        "locks bench: no workload `{name}`; there are: {}",
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
        let counted = measure::counted(&mut workload, run.runs);
        let timed = run
            .timed
            .then(|| measure::timed(&mut workload, run.samples));
        let about = workload.about();
        measured.push(json!({
            "name": about.name,
            "smells": about.smells,
            "run": about.run,
            "fixture_sha256": fixture_hash(workload.fixture()),
            "counted": counted,
            "timed": timed,
        }));
    }
    let source = machine::source();
    let result = json!({
        "schema": SCHEMA,
        "mode": if run.timed { "timed" } else { "counters" },
        "label": run.label,
        "started_utc": started,
        "finished_utc": machine::utc_now(),
        "binary": machine::binary(),
        "source": source,
        "toolchain": machine::toolchain(),
        "machine": machine::machine(),
        "load": { "before": load_before, "after": machine::load() },
        "options": { "runs": run.runs, "samples": run.timed.then_some(run.samples) },
        "workloads": measured,
    });
    print(&result);
    let path = run
        .output
        .clone()
        .unwrap_or_else(|| default_path(&started, &source, run.timed));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the result's directory can be made");
    }
    let text = serde_json::to_string_pretty(&result).expect("a result is plain JSON");
    std::fs::write(&path, text + "\n").expect("the result can be written");
    println!("\nwrote {}", path.display());
}

/// Each workload's counts, one line per site: the median of its runs,
/// marked `~` when the runs differ.
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
        lines.push(("allocations".to_owned(), &counted["allocations"]));
        lines.push(("allocated bytes".to_owned(), &counted["allocated_bytes"]));
        for (site, series) in lines {
            let mark = if series["stable"] == true { " " } else { "~" };
            let median = series["median"].as_f64().unwrap_or_default();
            println!("  {site:<20} {mark}{median:>12}");
        }
        let timed = &workload["timed"]["samples"];
        if let Some(median) = timed["median"].as_f64() {
            let max = timed["max"].as_f64().unwrap_or(median);
            println!(
                "  {:<20}  {:>12.3} ms median, {:.3} ms max",
                "time",
                median / 1e6,
                max / 1e6
            );
        }
    }
}

/// `target/lock-bench/<utc>-<revision>[-counters].json`, in the target
/// directory this binary was built in.
fn default_path(started: &str, source: &Value, timed: bool) -> PathBuf {
    let revision = source["revision"]
        .as_str()
        .map_or("unknown", |rev| &rev[..rev.len().min(10)]);
    let dirty = if source["dirty"] == true {
        "-dirty"
    } else {
        ""
    };
    let mode = if timed { "" } else { "-counters" };
    // The binary is target/<profile>/deps/locks-<hash>.
    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.ancestors().nth(3).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("target"));
    target
        .join("lock-bench")
        .join(format!("{started}-{revision}{dirty}{mode}.json"))
}

fn fixture_hash(fixture: &str) -> String {
    use sha2::Digest as _;
    machine::hex(&sha2::Sha256::digest(fixture.as_bytes()))
}
