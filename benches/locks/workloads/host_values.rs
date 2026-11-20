//! B3: host values' `==`, `<`, `{:?}` and `{}`, each of which reads its own
//! process-wide map under its own lock, all keyed by the same `TypeId`.
//! A3's per-native-type registry is reached only from a generated bridge's
//! place values, which this bench has none of (benches/README.md).

use crate::script::{self, Script};
use crate::workload::{About, Ran, Workload};

pub const NAME: &str = "host-values";

/// Sorting and searching times and paths, and printing them, as a program
/// that keeps file timestamps does.
const SOURCE: &str = r#"
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const VALUES: u64 = 64;
const ROUNDS: u64 = 20;

fn main() {
    let start = SystemTime::UNIX_EPOCH;
    let mut times: Vec<SystemTime> = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    for n in 0..VALUES {
        times.push(start + Duration::from_secs((n * 7919) % VALUES));
        paths.push(PathBuf::from(format!("/project/src/{}.rs", (n * 31) % VALUES)));
    }
    let mut equal = 0u64;
    let mut ordered = 0u64;
    let mut text = 0usize;
    for _ in 0..ROUNDS {
        for i in 1..times.len() {
            if times[i] == times[i - 1] {
                equal += 1;
            }
            if times[i - 1] < times[i] {
                ordered += 1;
            }
            if paths[i] == paths[i - 1] {
                equal += 1;
            }
            text += format!("{:?} {:?}", times[i], paths[i]).len();
            text += format!("{}", 'a'.to_uppercase()).len();
        }
    }
    println!("equal {equal}");
    println!("ordered {ordered}");
    println!("text {text}");
}
main();
"#;

pub fn workload() -> Workload {
    let script = Script::compile(SOURCE);
    let about = About {
        name: NAME,
        smells: &["B3"],
        run: "20 rounds over 64 SystemTime and PathBuf values: == and < of neighbours, {:?} of each, {} of a ToUppercase",
    };
    Workload::new(about, SOURCE.to_owned(), move || {
        let printed = script.run();
        let ordered = script::values(&printed, "ordered");
        assert!(ordered[0] > 0.0, "some neighbours are in order");
        Ran::default()
    })
}
