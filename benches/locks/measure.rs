//! One workload measured: counted runs in a counters build, timed samples
//! in a build without them. Each begins with one run left out, so what a
//! first run sets up once (a lazy table, a kept lowering) is not in it.
//!
//! A counted run reads every lock site's count and the allocations as
//! deltas around one run. A count that differs between runs depends on
//! scheduling, and the result says so beside it.

use serde_json::{json, Map, Value};

use crate::workload::{Ran, Workload};

/// Run `workload` once uncounted, then `runs` times, and give back what
/// each of those counted.
#[cfg(feature = "counters")]
pub fn counted(workload: &mut Workload, runs: usize) -> Value {
    use crate::allocations::Allocated;
    use rawr::counters::Site;

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
        counted.insert(site.name().to_owned(), series(counts));
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

/// Run `workload` once untimed, then `samples` times, timing each run.
#[cfg(not(feature = "counters"))]
pub fn timed(workload: &mut Workload, samples: usize) -> Value {
    let _ = workload.run();
    let mut times = Vec::with_capacity(samples);
    let mut reported = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = std::time::Instant::now();
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
#[cfg(feature = "counters")]
fn series(counts: &[u64]) -> Value {
    let stable = counts.windows(2).all(|pair| pair[0] == pair[1]);
    let values: Vec<f64> = counts.iter().map(|&count| count as f64).collect();
    json!({
        "runs": counts,
        "stable": stable,
        "median": median(&values),
        "min": counts.iter().min(),
        "max": counts.iter().max(),
    })
}

/// Every run's reported series: each run's values as it gave them, and a
/// summary of them all, by name.
fn reports(runs: Vec<Ran>) -> Value {
    let mut joined: Map<String, Value> = Map::new();
    let mut units: Map<String, Value> = Map::new();
    for ran in runs {
        for report in ran.into_reported() {
            units.insert(report.name.clone(), json!(report.unit));
            let per_run = joined.entry(report.name).or_insert_with(|| json!([]));
            per_run
                .as_array_mut()
                .expect("each name's runs are a list")
                .push(json!(report.values));
        }
    }
    let mut reported = Map::new();
    for (name, per_run) in joined {
        let all: Vec<f64> = per_run
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|run| {
                run.as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_f64)
            })
            .collect();
        let mut summary = summary(&all);
        summary["unit"] = units[&name].clone();
        summary["runs"] = per_run;
        reported.insert(name, summary);
    }
    Value::Object(reported)
}

/// Every value kept, with its median, quartiles, minimum and maximum.
pub fn summary(values: &[f64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({
        "values": values,
        "count": values.len(),
        "min": sorted.first(),
        "q1": quantile(&sorted, 0.25),
        "median": quantile(&sorted, 0.5),
        "q3": quantile(&sorted, 0.75),
        "max": sorted.last(),
    })
}

/// The middle value, or the mean of the middle two; `None` of none.
#[cfg(feature = "counters")]
fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    quantile(&sorted, 0.5)
}

/// The `q` quantile of `sorted`, interpolated between neighbours.
fn quantile(sorted: &[f64], q: f64) -> Option<f64> {
    let last = sorted.len().checked_sub(1)?;
    let at = q * last as f64;
    let (below, above) = (at.floor() as usize, at.ceil() as usize);
    Some(sorted[below] + (sorted[above] - sorted[below]) * (at - below as f64))
}
