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
python3.12 scripts/measure_reload.py --release 0.3.18 --wait-before-edits
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
both releases. Each timed edit window began after other builds stopped.

| Release | First edit | Five later edits: median | Later maximum | Separate startup |
|---|---:|---:|---:|---:|
| [0.3.17](reload-results/0.3.17.json) | 999.733 ms | 1107.552 ms | 1134.842 ms | 136.474 s |
| [0.3.18](reload-results/0.3.18-python3.12.json) | 1074.205 ms | 1133.550 ms | 1247.609 ms | 157.436 s |

The 0.3.18 later-edit median increased **25.998 ms (2.35%)**, and its maximum
increased **112.767 ms (9.94%)**. These are observed differences across two
small samples, not an isolated measurement of a particular code change.
Both startups overlapped other work and are not quiet-startup comparisons.
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
