//! Two results side by side: each workload's counts, allocations and
//! times before and after, and every difference in what they were
//! measured with that makes a figure less comparable.

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
        ("machine", &before["machine"], &after["machine"]),
        ("toolchain", &before["toolchain"], &after["toolchain"]),
        ("mode", &before["mode"], &after["mode"]),
        ("options", &before["options"], &after["options"]),
        (
            "debug assertions",
            &before["binary"]["debug_assertions"],
            &after["binary"]["debug_assertions"],
        ),
    ];
    for (what, was, now) in differences {
        if was != now {
            println!("note: the {what} differs: {was} then {now}");
        }
    }
    println!(
        "before: {} {} at {}, load {} .. {}",
        before["source"]["revision"],
        before["label"],
        before["started_utc"],
        before["load"]["before"],
        before["load"]["after"]
    );
    println!(
        "after:  {} {} at {}, load {} .. {}",
        after["source"]["revision"],
        after["label"],
        after["started_utc"],
        after["load"]["before"],
        after["load"]["after"]
    );
    for was in workloads(&before) {
        let name = &was["name"];
        let Some(now) = workloads(&after).find(|now| now["name"] == *name) else {
            println!(
                "\n{} is only in the first result",
                name.as_str().unwrap_or_default()
            );
            continue;
        };
        println!("\n{}", name.as_str().unwrap_or_default());
        if was["fixture_sha256"] != now["fixture_sha256"] {
            println!("  note: the fixture changed; its counts measure another program");
        }
        compare_counts(&was["counted"], &now["counted"]);
        compare_times(&was["timed"], &now["timed"]);
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
    let mut sites: Vec<&String> = Vec::new();
    for counted in [was, now] {
        for site in counted["sites"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(site, _)| site)
        {
            if !sites.contains(&site) {
                sites.push(site);
            }
        }
    }
    println!(
        "  {:<20} {:>13} {:>13} {:>13}",
        "count (median)", "before", "after", "change"
    );
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

fn compare_times(was: &Value, now: &Value) {
    if was.is_null() || now.is_null() {
        return;
    }
    let (was, now) = (&was["samples"], &now["samples"]);
    for (what, key) in [("time median ms", "median"), ("time max ms", "max")] {
        let before = was[key].as_f64().map(|ns| ns / 1e6);
        let after = now[key].as_f64().map(|ns| ns / 1e6);
        println!(
            "  {what:<20} {:>13} {:>13} {:>13}",
            figure(before),
            figure(after),
            change(before, after)
        );
    }
}

/// One count's medians before and after, and the change; `~` marks a
/// count whose runs differed. A site a result has no entry for took its
/// lock no time.
fn line(name: &str, was: &Value, now: &Value) {
    let mark = |series: &Value| {
        if series.is_null() || series["stable"] == true {
            ""
        } else {
            "~"
        }
    };
    let median = |series: &Value| {
        if series.is_null() {
            Some(0.0)
        } else {
            series["median"].as_f64()
        }
    };
    let (before, after) = (median(was), median(now));
    println!(
        "  {name:<20} {:>13} {:>13} {:>13}",
        format!("{}{}", mark(was), figure(before)),
        format!("{}{}", mark(now), figure(after)),
        change(before, after)
    );
}

fn figure(value: Option<f64>) -> String {
    value.map_or_else(
        || "-".to_owned(),
        |value| format!("{value:.3}").trim_end_matches(".000").to_owned(),
    )
}

fn change(before: Option<f64>, after: Option<f64>) -> String {
    match (before, after) {
        (Some(before), Some(after)) if before == 0.0 => format!("{:+}", after - before),
        (Some(before), Some(after)) => format!("{:+.1}%", (after - before) / before * 100.0),
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
