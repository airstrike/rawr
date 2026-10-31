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
