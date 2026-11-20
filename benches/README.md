Micro Benchmarks
================

Micro benchmarks to measure the speed of Rawr. The scripts they run are typed Rust.

As with all micro benchmarks, take these with a grain of salt.


How to Run
----------

```bash
cargo +nightly bench --features unstable
```

Release hot reload measurements
-------------------------------

After installing each release, run this on macOS or Linux with Python 3.11+:

```bash
python3.12 scripts/measure_reload.py --release 0.3.19 --wait-before-edits
```

The command starts the installed `rawr` and its adjacent `rawr-import` and
`rawr-import-driver`. Add `--release-revision <commit>` to record the commit
used to build that installation. At **Ready**, wait for unrelated builds to finish, then
press Enter. Omit `--wait-before-edits` for an unattended run; use
`--label provisional-busy` when the machine is busy. Load is recorded, never
used to refuse work. `--bin-dir` selects another complete installation.
Use the same observer Python version as the previous comparable baseline;
the first baseline used **3.12.0**. Verify the executable's version before
starting. Preserve unmatched and repeated runs, and state why a matched run
is selected for comparison.

Recorded releases use the same Apple M2 Max machine, fixture and harness.
The following comparison uses Python 3.12.0 and fresh benchmark caches for
all four releases. Each timed edit window began after other builds stopped.

| Release | First edit | Five later edits: median | Later maximum | Separate startup |
|---|---:|---:|---:|---:|
| [0.3.17](reload-results/0.3.17.json) | 999.733 ms | 1107.552 ms | 1134.842 ms | 136.474 s |
| [0.3.18](reload-results/0.3.18-python3.12.json) | 1074.205 ms | 1133.550 ms | 1247.609 ms | 157.436 s |
| [0.3.19](reload-results/0.3.19.json) | 1102.031 ms | 1169.356 ms | 1262.574 ms | 145.785 s |
| [0.3.20](reload-results/0.3.20.json) | 1081.242 ms | 1174.988 ms | 1191.296 ms | 148.713 s |
| [0.3.21](reload-results/0.3.21.json) | 1130.528 ms | 1259.909 ms | 1312.979 ms | 144.876 s |
| [0.3.25](reload-results/0.3.25.json) | 60.058 ms | 77.401 ms | 82.156 ms | 138.997 s |

From 0.3.17 to 0.3.18, the later-edit median increased **25.998 ms (2.35%)**,
and its maximum increased **112.767 ms (9.94%)**.

From 0.3.18 to 0.3.19, the first edit increased **27.825 ms (2.59%)**, the
later-edit median increased **35.806 ms (3.16%)**, and its maximum increased
**14.966 ms (1.20%)**. The fixture, harness, machine and observer match; all
six 0.3.19 edits retained native PID 41586 and its increasing state counter.
The five later samples are 1207.968, 1141.478, 1169.356, 1262.574 and
1125.011 ms; the JSON preserves their full precision.

From 0.3.19 to 0.3.20, the first edit decreased **20.789 ms (1.89%)**,
the later-edit median increased **5.632 ms (0.48%)**, and its maximum decreased
**71.279 ms (5.65%)**. The fixture, harness, machine, observer and fresh-cache
conditions match. All six edits retained native PID 65798 and its increasing
state counter. The five later samples are 1160.873, 1174.988, 1171.894,
1191.296 and 1177.800 ms; the JSON preserves their full precision.

From 0.3.20 to 0.3.21, the first edit increased **49.286 ms (4.56%)**,
the later-edit median increased **84.921 ms (7.23%)**, and its maximum
increased **121.683 ms (10.21%)**. Between those two releases, Rawr's own
code changed only in version numbers; the release changed the playground
bot's deployment checks. The difference is therefore run-to-run variance on
this machine, not a code change. All six edits retained native PID 21568 and
its increasing state counter. The five later samples are 1259.909, 1263.858,
1226.140, 1230.497 and 1312.979 ms.

From 0.3.21 to 0.3.25, the later-edit median fell **1182.508 ms (93.86%)**,
from 1259.909 ms to 77.401 ms, about 16 times faster. The first edit fell
**1070.470 ms (94.69%)**, and the later maximum fell **1230.823 ms (93.74%)**.
All six edits retained native PID 47107 and its increasing state counter. The
five later samples are 77.401, 79.352, 82.156, 76.100 and 54.796 ms.

This is resident compilation of body edits in the plain runner (#373, D656),
which shipped in 0.3.25. Before it, every save ran full preparation in the
parent: resolving the project again, replaying and validating the cached
native surface and checking the runner plan. That took 93–98% of each reload,
while compiling the edit itself took about 2 ms. Now the running runner
compiles the new source against the native surface it was built with and
installs it in the same process, with its state kept. Anything that surface
can't vouch for, such as a new native call or a manifest change, still takes
full preparation. Most of the remaining time is the watcher's 30 ms debounce
and the wait for the fixture's next 20 ms tick, when the change becomes
observable.

0.3.22, 0.3.23 and 0.3.24 weren't measured to this standard, because parallel
builds kept the machine busy. A provisional run of the installed 0.3.24
against the branch build, both on a busy machine, recorded a 993.8 ms later
median before and 48.6 ms after.

An [earlier 0.3.21 run](reload-results/0.3.21-python3.12.9.json) used
Python 3.12.9 and recorded a 1323.171 ms later-edit median with one
2932.879 ms edit. It is retained as measured. The second run was chosen to
match the original observer version, before its results were known.

These are observed differences across small samples, not an isolated
measurement of a particular code change. All four startups overlapped
other work and are not quiet-startup comparisons.
Requested callback cadence was 20 ms; observed cadence and machine load are
retained in each result.

An [earlier 0.3.18 run](reload-results/0.3.18.json) used Python 3.14.7 and a
previously populated benchmark cache, recording 1178.581 ms later-edit median.
It is retained as measured. The second run was chosen to match the original
observer version and fresh-cache conditions, before its results were known.

The benchmark copies `import-cli/tests/fixtures/marked-ticker` into its owned
`target/reload-bench/fixture`, uses a **20 ms** native tick interval, and extends
the loop count. The checked-in fixture remains untouched (its interval is
250 ms; `3000` is its loop count). After startup, it changes only the callback's
factor, first from 1 to 2, then to 3–7. Every observed callback must retain one
native PID and an unbroken increasing native counter. Two further callbacks
confirm each new body before the next save.

Each sample uses `time.monotonic_ns()`, from just before an in-place source
write to the observer thread receiving the first complete stdout line with
the new callback result. It includes saving, detection, compilation, swapping,
waiting for a native tick, callback execution, stdout transport, and observer
scheduling. The 20 ms sleep is after each callback, so its actual period also
includes callback work; observed intervals and source-write durations are
recorded. These are **native callback observation times, not GUI paint times**.
Startup through the first callback is reported separately.

Results go to `benches/reload-results/<UTC>-<version>.json` (or `--output`). Keep
the release baseline JSON in version control. Each records the first edit and
five later edits, all raw samples, counts/medians/maxima for all and later
edits, fixture and harness hashes, binary hashes and versions, toolchain,
machine, load, and cache conditions. Compare matching fixture hashes and
cadence on the same machine under similar load; five later samples do not
justify a p95. No speed threshold gates a release. The configurable startup
and edit timeouts only bound an incomplete or hung measurement.

The locked work directory retains only this benchmark's build/cache data and
timestamped output logs to avoid rebuilding the runtime on every run. On
success, failure, or handled termination, it stops its own process group and
restores the copied fixture source. It never stops processes by name or cleans
another project's cache. Uncatchable termination such as SIGKILL cannot run
cleanup; a later run restores the owned source before starting. Use a distinct
`--work-dir` for an independent cache or concurrent benchmark.

Harness checks:

```bash
python3 -m unittest discover -s scripts -p 'test_measure_reload.py'
```

Lock workloads
--------------

`benches/locks` measures the paths the locks review's fixes change
(`.Codex/map/oracle-locks-2026-11-20/synthesis.md`), so each fix has a
baseline taken before it lands. Counts come first: how many times each
reviewed lock is taken, and how many allocations a run makes. They don't
depend on machine load, and they prove a lock's removal by themselves.
Wall-clock times come second, for the root to take on a quiet machine.

```bash
# Counts only: one quick pass, any time.
'/Users/andy/projects/rawr/scripts/cargo.mjs' bench --profile local --bench locks \
  --features sync,rust-import,debugging,lock-counters -- --counters

# Counts and times, on a quiet machine.
'/Users/andy/projects/rawr/scripts/cargo.mjs' bench --profile local --bench locks \
  --features sync,rust-import,debugging,lock-counters -- --label quiet

# Two results side by side.
'/Users/andy/projects/rawr/scripts/cargo.mjs' bench --profile local --bench locks \
  --features sync,rust-import,debugging,lock-counters -- \
  compare target/lock-bench/BEFORE.json target/lock-bench/AFTER.json
```

`--profile local` builds what `scripts/install-local.sh` installs; the
`bench` profile's fat LTO takes much longer and is not what runs. Other
options: `--runs N` counted runs per workload (3), `--samples N` timed
samples (11, after one warm-up run), `--only a,b` and `--output PATH`.
The bench uses its own harness and runs on stable.

| Workload | Smells | One run |
|---|---|---|
| `mutex-contended` | B5, C1 | 4 threads × 250 locks of one script `Mutex` with a named guard, then 4 `thread::Builder` threads × 250 with a temporary guard |
| `mutex-handoff` | B5 | 20 handoffs of a held lock to a blocked waiter at the guard's scope end; the script reports each handoff's latency in µs |
| `host-values` | B3 | 20 rounds of `==`, `<` and `{:?}` over 64 `SystemTime` and `PathBuf` values, and `{}` of a `ToUppercase` |
| `recompile` | A1, B4, B6 | one save: a body edit of a two-source program, compiled as a runner compiles it (`compile_package_snapshot_resident_plain` on an engine with a retained native account) |
| `debugger-steps` | D1 | a generation placed under a socket inspector, then 5 pauses at a breakpoint, each stepped over to the function's end by a tool thread (Unix only) |
| `futures-undriven` | C | 1000 converted (`into_native`) and 1000 script (`into_script`) futures polled to their end where no owner drives |
| `futures-driven` | C | the same on a driven owner thread, whose queued jobs make the outputs; it runs last, because a driven process stays driven |

The counted sites are `rawr::counters::Site`. Each reviewed lock calls
`counted!(Site)` where it is taken. That macro expands to nothing without
the `lock-counters` feature, which only this bench enables, so no build that
ships pays for a count. With the feature, a count is a relaxed add on its own
cache line. The allocations come from the bench's own counting global
allocator. One uncounted run comes before the counted runs, so what a first
run sets up once is left out. A count that differs between the counted runs depends on
scheduling, such as a wait that ends at the 1 ms poll or an allocation on a
thread. It is printed with `~` and recorded with `"stable": false`.

Times include the counters, in the baseline and after a fix alike. Each
timed sample is one run. The result keeps every sample with its median, min
and max, and every value a script reported. A result is
`target/lock-bench/<UTC>-<revision>[-dirty][-counters].json`, schema
`rawr-lock-bench/1`. It records the binary's path and SHA-256, rawr's
version and whether debug assertions were on, the revision and whether the
tree was dirty, `rustc -V` and `cargo -V`, the processor, model and thread
count, the load averages before and after, and each workload's fixture
SHA-256. `compare` notes every difference in those, and marks a workload
whose fixture changed. Compare runs of one mode, profile and machine. No
figure gates anything.

The workloads stand in for these sites only:

- **A3** (`place::reading`'s registry) is read only by a generated bridge's
  place values, and this bench has no generated bridge. Its counter is there
  for a run inside a runner. B3's maps are the same kind of lookup on the
  same operations.
- **B4** is counted inside `recompile`, where a compile's charges are. The
  charge itself (`ImportAllocation::retain_checked`) is crate-private, so no
  bare charge loop can drive it.
- **B5**'s handoff has no `drop(guard)` case. rawr never releases a script
  `Mutex` that `drop` was given a guard of, so the next `lock()` waits
  forever, even on one thread. A guard that ends at its scope releases.
  When that is fixed, a `drop` handoff belongs beside the scoped one.
- **C**'s futures are ready ones. The poll's bookkeeping (`served()`, the
  `Made<T>` cell) is the same for every kind of future, and a ready one adds
  no body to the time.

These sites change no compute that this bench could see:

- **B1** (`timing.rs`) records once per command.
- **B2** reads the feature selection a few times per command.
- **A2** is the non-`sync` arm, which no shipped build compiles.
- **D2** is wasm32 only.
- **D3** is taken once per run.
- **E1–E3** run once per importer round, beside Cargo and rustc processes.
- **E4** removes no compute, as the review says.
- **F1–F5** take one uncontended lock per message or request, beside the
  request's JSON and socket I/O. They live in the `rawr` CLI crate.
- **G1** is the watcher, which `scripts/measure_reload.py` (above) times
  end to end on each save.
