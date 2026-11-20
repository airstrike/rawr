//! What the command line asks for: a run, or a comparison of two results.

use std::path::PathBuf;

/// How to call the bench, printed with a bad argument.
pub const USAGE: &str = "\
usage: cargo bench -p rawr-lock-bench --profile local [-- options]
         counts (the default build)
       cargo bench -p rawr-lock-bench --profile local --no-default-features [-- options]
         times: no counted sites, no counting allocator
       cargo bench -p rawr-lock-bench --profile local -- compare BEFORE.json AFTER.json

options:
  --runs N          counted runs per workload, after one warm-up (default 3)
  --samples N       timed samples per workload, after one warm-up (default 11)
  --only A,B        only these workloads
  --label TEXT      a label kept in the result (`quiet`, `busy`, a branch)
  --output PATH     where to write the result (default target/lock-bench/<utc>-<rev>.json)";

/// The command line, read.
pub struct Options {
    command: Command,
}

/// What a run measures and where it writes. Whether it counts or times is
/// the build's: the `counters` feature counts.
pub struct Run {
    pub runs: usize,
    pub samples: usize,
    pub only: Option<Vec<String>>,
    pub label: Option<String>,
    pub output: Option<PathBuf>,
}

/// A run, or a comparison of two results.
pub enum Command {
    Run(Run),
    Compare { before: PathBuf, after: PathBuf },
}

impl Options {
    /// Read `args`, the bench binary's arguments. Cargo adds `--bench`,
    /// which is ignored.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = args.into_iter().filter(|arg| arg != "--bench").peekable();
        if args.peek().map(String::as_str) == Some("compare") {
            args.next();
            let (Some(before), Some(after), None) = (args.next(), args.next(), args.next()) else {
                return Err("compare takes two result files".to_owned());
            };
            return Ok(Self {
                command: Command::Compare {
                    before: before.into(),
                    after: after.into(),
                },
            });
        }
        let mut run = Run {
            runs: 3,
            samples: 11,
            only: None,
            label: None,
            output: None,
        };
        while let Some(arg) = args.next() {
            let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} takes a value"));
            match arg.as_str() {
                "--runs" => run.runs = count(&value("--runs")?)?,
                "--samples" => run.samples = count(&value("--samples")?)?,
                "--only" => {
                    let names: Vec<String> =
                        value("--only")?.split(',').map(str::to_owned).collect();
                    run.only = Some(names);
                }
                "--label" => run.label = Some(value("--label")?),
                "--output" => run.output = Some(PathBuf::from(value("--output")?)),
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        Ok(Self {
            command: Command::Run(run),
        })
    }

    /// What was asked.
    pub fn command(&self) -> &Command {
        &self.command
    }
}

fn count(text: &str) -> Result<usize, String> {
    match text.parse() {
        Ok(0) | Err(_) => Err(format!("`{text}` isn't a count of one or more")),
        Ok(count) => Ok(count),
    }
}
