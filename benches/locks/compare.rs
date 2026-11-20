//! Two results side by side: each workload's counts, allocations, times
//! and reported values before and after, and every difference in what they
//! were measured with that makes a figure less comparable.

use serde_json::Value;
use std::path::Path;

use crate::record::SCHEMA;

/// Print `before` against `after`.
pub fn print(before: &Path, after: &Path) {
    let (before, after) = (read(before), read(after));
    for result in [&before, &after] {
        if result["schema"] != SCHEMA {
            eprintln!(
                "locks bench: a result is {}, and this bench reads {SCHEMA}",
                result["schema"]
            );
            std::process::exit(2);
        }
    }
    let differences = [
        ("mode", &before["mode"], &after["mode"]),
        (
            "profile",
            &before["build"]["profile"],
            &after["build"]["profile"],
        ),
        (
            "opt level",
            &before["build"]["opt_level"],
            &after["build"]["opt_level"],
        ),
        (
            "debug assertions",
            &before["build"]["debug_assertions"],
            &after["build"]["debug_assertions"],
        ),
        ("machine", &before["machine"], &after["machine"]),
        ("toolchain", &before["toolchain"], &after["toolchain"]),
        ("options", &before["options"], &after["options"]),
    ];
    for (what, was, now) in differences {
        if was != now {
            println!("note: the {what} differs: {was} then {now}");
        }
    }
    for (which, result) in [("before", &before), ("after", &after)] {
        let build = &result["build"];
        if build["dirty"] == true {
            println!(
                "note: {which} was built from a dirty tree (changes {})",
                build["changes"]
            );
        }
        println!(
            "{which:<6} {} {} {} at {}, load {} .. {}",
            build["revision"],
            build["profile"],
            result["label"],
            result["started_utc"],
            result["load"]["before"],
            result["load"]["after"]
        );
    }
    for was in workloads(&before) {
        let name = was["name"].as_str().unwrap_or_default();
        let Some(now) = workloads(&after).find(|now| now["name"] == was["name"]) else {
            println!("\n{name} is only in the first result");
            continue;
        };
        println!("\n{name}");
        if was["fixture_sha256"] != now["fixture_sha256"] {
            println!("  note: the fixture changed; its figures measure another program");
        }
        compare_counts(&was["counted"], &now["counted"]);
        compare_times(&was["timed"], &now["timed"]);
        let reported = |result: &Value| {
            if result["counted"].is_null() {
                result["timed"]["reported"].clone()
            } else {
                result["counted"]["reported"].clone()
            }
        };
        compare_reported(&reported(was), &reported(now));
    }
    for now in workloads(&after) {
        if !workloads(&before).any(|was| was["name"] == now["name"]) {
            println!(
                "\n{} is only in the second result",
                now["name"].as_str().unwrap_or_default()
            );
        }
    }
}

fn compare_counts(was: &Value, now: &Value) {
    if was.is_null() || now.is_null() {
        return;
    }
    let mut sites: Vec<&String> = Vec::new();
    for counted in [was, now] {
        for (site, _) in counted["sites"].as_object().into_iter().flatten() {
            if !sites.contains(&site) {
                sites.push(site);
            }
        }
    }
    println!("  {:<16} {:>24} {:>24}", "count by run", "before", "after");
    for site in sites {
        line(site, &was["sites"][site], &now["sites"][site]);
    }
    line("allocations", &was["allocations"], &now["allocations"]);
    line(
        "allocated bytes",
        &was["allocated_bytes"],
        &now["allocated_bytes"],
    );
}

/// One count's runs before and after; `~` marks runs that differed.
fn line(name: &str, was: &Value, now: &Value) {
    let shown = |series: &Value| {
        let mark = if series["stable"] == true { "" } else { "~" };
        format!("{mark}{}", series["runs"])
    };
    println!("  {name:<16} {:>24} {:>24}", shown(was), shown(now));
}

fn compare_times(was: &Value, now: &Value) {
    if was.is_null() || now.is_null() {
        return;
    }
    println!(
        "  {:<16} {:>10} {:>10} {:>10}",
        "time ms", "before", "after", "change"
    );
    for key in ["min", "q1", "median", "q3", "max"] {
        let before = was["samples"][key].as_f64().map(|ns| ns / 1e6);
        let after = now["samples"][key].as_f64().map(|ns| ns / 1e6);
        println!(
            "  {key:<16} {:>10} {:>10} {:>10}",
            figure(before),
            figure(after),
            change(before, after)
        );
    }
}

/// Each series a program reported, run by run, before and after.
fn compare_reported(was: &Value, now: &Value) {
    for (name, series) in was.as_object().into_iter().flatten() {
        let unit = series["unit"].as_str().unwrap_or_default();
        for (which, series) in [("before", series), ("after", &now[name])] {
            println!(
                "  {name} {which} ({unit}), median {}, by run:",
                series["median"]
            );
            for run in series["runs"].as_array().into_iter().flatten() {
                println!("    {run}");
            }
        }
    }
}

fn figure(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| format!("{value:.3}"))
}

fn change(before: Option<f64>, after: Option<f64>) -> String {
    match (before, after) {
        (Some(before), Some(after)) if before > 0.0 => {
            format!("{:+.1}%", (after - before) / before * 100.0)
        }
        _ => "-".to_owned(),
    }
}

fn workloads(result: &Value) -> impl Iterator<Item = &Value> {
    result["workloads"].as_array().into_iter().flatten()
}

fn read(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| {
        eprintln!("locks bench: can't read {}: {error}", path.display());
        std::process::exit(2);
    });
    serde_json::from_str(&text).unwrap_or_else(|error| {
        eprintln!("locks bench: {} isn't a result: {error}", path.display());
        std::process::exit(2);
    })
}
