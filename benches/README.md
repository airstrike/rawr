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
python3 scripts/measure_reload.py --release 0.3.17 --wait-before-edits
```

The command starts the installed `rawr` and its adjacent `rawr-import` and
`rawr-import-driver`. Add `--release-revision <commit>` to record the commit
used to build that installation. At **Ready**, wait for unrelated builds to finish, then
press Enter. Omit `--wait-before-edits` for an unattended run; use
`--label provisional-busy` when the machine is busy. Load is recorded, never
used to refuse work. `--bin-dir` selects another complete installation.

The committed [0.3.17 baseline](reload-results/0.3.17.json) measured a first
edit of 999.733 ms, then a median of 1107.552 ms and maximum of 1134.842 ms
over five later edits. Those edits ran after other builds stopped. Its
136.474 s cold-cache startup overlapped other work; the observed callback
interval median was 25.068 ms on this machine.

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
