//! One workload measured: counted runs first, then timed samples.
//!
//! A counted run reads every lock site's count and the allocations as
//! deltas around one run. A count that differs between runs depends on
//! scheduling (a wait that ends at a poll, an allocation on another
//! thread), and the result says so beside it.

use rawr::counters::Site;
use serde_json::{json, Map, Value};
use std::time::Instant;

use crate::allocations::Allocated;
use crate::workload::{Ran, Reported, Workload};

/// Run `workload` once uncounted, so what a first run sets up once (a
/// lazy table, a kept lowering) is left out, then `runs` times, and give
/// back what each of those counted.
pub fn counted(workload: &mut Workload, runs: usize) -> Value {
    let _ = workload.run();
    let mut sites: Vec<Vec<u64>> = vec![Vec::new(); Site::ALL.len()];
    let mut allocations = Vec::new();
    let mut allocated_bytes = Vec::new();
    let mut reported = Vec::new();
    for _ in 0..runs {
        rawr::counters::reset();
        let before = Allocated::now();
        let ran = workload.run();
        let allocated = Allocated::now().since(before);
        for (counts, site) in sites.iter_mut().zip(Site::ALL) {
            counts.push(site.count());
        }
        allocations.push(allocated.count);
        allocated_bytes.push(allocated.bytes);
        reported.push(ran);
    }
    let mut counted = Map::new();
    for (counts, site) in sites.iter().zip(Site::ALL) {
        if counts.iter().any(|&count| count > 0) {
            counted.insert(site.name().to_owned(), series(counts));
        }
    }
    json!({
        "warmup": 1,
        "runs": runs,
        "sites": counted,
        "allocations": series(&allocations),
        "allocated_bytes": series(&allocated_bytes),
        "reported": reports(reported),
    })
}

/// Run `workload` once to warm it, then `samples` times, timing each run.
pub fn timed(workload: &mut Workload, samples: usize) -> Value {
    let _ = workload.run();
    let mut times = Vec::with_capacity(samples);
    let mut reported = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let ran = workload.run();
        times.push(start.elapsed().as_nanos() as f64);
        reported.push(ran);
    }
    json!({
        "warmup": 1,
        "unit": "ns",
        "samples": summary(&times),
        "reported": reports(reported),
    })
}

/// A count's runs, and whether every run counted the same.
fn series(counts: &[u64]) -> Value {
    let stable = counts.windows(2).all(|pair| pair[0] == pair[1]);
    let values: Vec<f64> = counts.iter().map(|&count| count as f64).collect();
    json!({
        "runs": counts,
        "stable": stable,
        "median": median(&values),
        "max": counts.iter().max(),
    })
}

/// Every run's reported series, joined by name, each summarized.
fn reports(runs: Vec<Ran>) -> Value {
    let mut joined: Vec<Reported> = Vec::new();
    for ran in runs {
        for report in ran.into_reported() {
            match joined.iter_mut().find(|kept| kept.name == report.name) {
                Some(kept) => kept.values.extend(report.values),
                None => joined.push(report),
            }
        }
    }
    let mut reported = Map::new();
    for report in joined {
        let mut summary = summary(&report.values);
        summary["unit"] = json!(report.unit);
        reported.insert(report.name, summary);
    }
    Value::Object(reported)
}

/// Every value kept, with the median, the minimum and the maximum.
pub fn summary(values: &[f64]) -> Value {
    json!({
        "values": values,
        "count": values.len(),
        "median": median(values),
        "min": values.iter().copied().reduce(f64::min),
        "max": values.iter().copied().reduce(f64::max),
    })
}

/// The middle value, or the mean of the middle two; `None` of none.
pub fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        len if len % 2 == 1 => Some(sorted[middle]),
        _ => Some((sorted[middle - 1] + sorted[middle]) / 2.0),
    }
}
