//! The lock sites' workloads: what each lock fix of the locks review
//! removes, as counts in the default build and as wall-clock times in a
//! build without the counters, written to one JSON result that a later run
//! compares against.
//!
//! See benches/README.md, "Lock workloads".

#[cfg(feature = "counters")]
mod allocations;
mod compare;
mod machine;
mod measure;
mod options;
mod record;
mod script;
mod workload;
mod workloads;

fn main() {
    let options = options::Options::from_args(std::env::args().skip(1)).unwrap_or_else(|error| {
        eprintln!("locks bench: {error}\n\n{}", options::USAGE);
        std::process::exit(2);
    });
    match options.command() {
        options::Command::Run(run) => record::run(run),
        options::Command::Compare { before, after } => compare::print(before, after),
    }
}
