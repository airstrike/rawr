#!/usr/bin/env python3
"""Measure an installed release's save-to-native-callback observation latency."""

import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import queue
import re
import shutil
import signal
import statistics
import subprocess
import sys
import threading
import time


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "import-cli/tests/fixtures/marked-ticker"
OWNER = "rawr reload benchmark v1\n"
TICK_MS = 20
FACTORS = range(2, 8)


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def files_digest(files):
    result = hashlib.sha256()
    for name, content in sorted(files.items()):
        encoded = name.encode()
        result.update(len(encoded).to_bytes(8, "big"))
        result.update(encoded)
        result.update(len(content).to_bytes(8, "big"))
        result.update(content)
    return result.hexdigest()


def public_path(path):
    return str(path).replace(str(Path.home()), "~")


def command_text(arguments, cwd=ROOT):
    return subprocess.check_output(arguments, cwd=cwd, text=True, timeout=30).strip()


def binaries(directory):
    result = {}
    for name in ("rawr", "rawr-import", "rawr-import-driver"):
        path = (directory / name).resolve(strict=True)
        if path.parent != directory:
            raise RuntimeError("all three binaries must resolve inside --bin-dir")
        result[name] = {
            "version": command_text([str(path), "--version"]),
            "sha256": digest(path),
        }
    return result


def machine():
    result = {
        "system": platform.system(),
        "release": platform.release(),
        "architecture": platform.machine(),
        "logical_cpus": os.cpu_count(),
        "python": platform.python_version(),
    }
    if sys.platform == "darwin":
        for key in ("hw.model", "machdep.cpu.brand_string"):
            result[key] = command_text(["/usr/sbin/sysctl", "-n", key])
    return result


def load():
    return {"utc": utc(), "load_average_1_5_15_minutes": list(os.getloadavg())}


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def summary(samples):
    values = [sample["latency_ms"] for sample in samples]
    return {
        "count": len(values),
        "median_ms": statistics.median(values) if values else None,
        "max_ms": max(values) if values else None,
    }


class Observation:
    """Evidence that output came from one native loop retaining its counter."""

    def __init__(self):
        self.pid = None
        self.tick = 0
        self.value = None
        self.observed_ns = None
        self.intervals_ms = []

    def consume(self, observed_ns, stream, line):
        if stream == "stderr":
            if "restarting" in line:
                raise RuntimeError("the program restarted: " + line)
            return False
        if match := re.fullmatch(r"pid (\d+)", line):
            if self.pid is not None:
                raise RuntimeError("more than one native PID announcement")
            self.pid = int(match[1])
            return False
        if match := re.fullmatch(r"(\d+) value (-?\d+)", line):
            tick, value = map(int, match.groups())
            if self.pid is None or tick != self.tick + 1:
                raise RuntimeError("native counter did not retain its increasing sequence")
            if self.observed_ns is not None:
                self.intervals_ms.append((observed_ns - self.observed_ns) / 1_000_000)
            self.tick, self.value, self.observed_ns = tick, value, observed_ns
            return True
        return False


class Running:
    def __init__(self, arguments, environment, directory, log):
        self.events = queue.Queue()
        self.observation = Observation()
        self.started_ns = time.monotonic_ns()
        self.child = subprocess.Popen(
            arguments,
            cwd=directory,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        self.log = log
        self.readers = []
        self.closed = set()
        for name, pipe in (("stdout", self.child.stdout), ("stderr", self.child.stderr)):
            reader = threading.Thread(target=self.read, args=(name, pipe), daemon=True)
            reader.start()
            self.readers.append(reader)

    def read(self, name, pipe):
        for line in iter(pipe.readline, b""):
            # Timestamp before decoding, queueing, parsing, or writing the log.
            observed_ns = time.monotonic_ns()
            self.events.put((observed_ns, name, line.decode(errors="replace").rstrip("\r\n")))
        self.events.put((time.monotonic_ns(), name, None))
        pipe.close()

    def next(self, deadline):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("observation wait expired; measurement is incomplete")
        try:
            event = self.events.get(timeout=remaining)
        except queue.Empty as error:
            raise TimeoutError("observation wait expired; measurement is incomplete") from error
        observed_ns, stream, line = event
        if stream == "continue":
            if line is None:
                raise RuntimeError("stdin closed before the measurement was released")
            return "continue"
        if line is None:
            self.closed.add(stream)
            if len(self.closed) == 2:
                raise RuntimeError(f"program output ended (exit {self.child.poll()})")
            return False
        self.log.write(json.dumps({
            "since_start_ns": observed_ns - self.started_ns,
            "stream": stream,
            "line": public_path(line),
        }) + "\n")
        self.log.flush()
        return self.observation.consume(*event)

    def until(self, predicate, timeout):
        deadline = time.monotonic() + timeout
        while True:
            if self.next(deadline) is True and predicate(self.observation):
                return self.observation

    def release(self, timeout):
        def enter():
            line = sys.stdin.readline()
            self.events.put((time.monotonic_ns(), "continue", line if line else None))

        threading.Thread(target=enter, daemon=True).start()
        deadline = time.monotonic() + timeout
        while self.next(deadline) != "continue":
            pass

    def stop(self, stop_file):
        stop_file.touch()
        try:
            self.child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        # Also stop a surviving descendant if its launcher already exited.
        # This process group was created exclusively for this run.
        try:
            os.killpg(self.child.pid, signal.SIGTERM)
            try:
                self.child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(self.child.pid, signal.SIGKILL)
                self.child.wait()
        except ProcessLookupError:
            pass
        for reader in self.readers:
            reader.join(timeout=2)


def interrupted(signum, _frame):
    raise KeyboardInterrupt(f"signal {signum}")


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    installed = shutil.which("rawr")
    parser.add_argument("--bin-dir", type=Path, default=Path(installed).resolve().parent if installed else None)
    parser.add_argument("--release", help="Require this installed rawr/rawr-import version")
    parser.add_argument("--release-revision", help="Git commit used to build the installed release")
    parser.add_argument("--work-dir", type=Path, default=ROOT / "target/reload-bench")
    parser.add_argument("--output", type=Path, help="JSON result (default: benches/reload-results/<UTC>-<version>.json)")
    parser.add_argument("--label", default="release-baseline", help="Context such as release-baseline or provisional-busy")
    parser.add_argument("--wait-before-edits", action="store_true", help="Wait for Enter after startup; keep observing callbacks")
    parser.add_argument("--startup-timeout", type=float, default=900)
    parser.add_argument("--edit-timeout", type=float, default=120)
    return parser.parse_args()


def measure(args):
    if args.bin_dir is None:
        raise RuntimeError("rawr is not installed on PATH; pass --bin-dir")
    directory = args.bin_dir.resolve()
    installed = binaries(directory)
    version = installed["rawr"]["version"].removeprefix("rawr ")
    if installed["rawr-import"]["version"] != "rawr-import " + version:
        raise RuntimeError("installed rawr and rawr-import versions differ")
    if args.release and version != args.release:
        raise RuntimeError(f"expected release {args.release}, found {version}")
    run_id = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    output = (args.output or ROOT / "benches/reload-results" / f"{run_id}-{version}.json").resolve()
    if output.exists():
        raise RuntimeError(f"result already exists: {output}")
    work = args.work_dir.resolve()
    marker = work / ".rawr-reload-benchmark"
    if work.exists() and any(work.iterdir()) and (not marker.is_file() or marker.read_text() != OWNER):
        raise RuntimeError(f"refusing to overwrite an unowned work directory: {work}")
    work.mkdir(parents=True, exist_ok=True)
    marker.write_text(OWNER)
    with (work / "lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        return measure_owned(args, directory, installed, version, run_id, output, work)


def measure_owned(args, directory, installed, version, run_id, output, work):
    pristine = {
        str(path.relative_to(FIXTURE)): path.read_bytes()
        for path in FIXTURE.rglob("*")
        if path.is_file() and not {"target", ".git", "Cargo.lock"}.intersection(path.relative_to(FIXTURE).parts)
    }
    prepared = dict(pristine)
    main_name, native_name = "src/main.rs", "ticker/src/lib.rs"
    prepared[main_name] = pristine[main_name].replace(b"ticker::run(step, 3000);", b"ticker::run(step, 1_000_000);")
    prepared[native_name] = pristine[native_name].replace(b"250 ms", b"20 ms").replace(b"from_millis(250)", b"from_millis(20)")
    if prepared[main_name] == pristine[main_name] or prepared[native_name] == pristine[native_name]:
        raise RuntimeError("marked-ticker changed; review the benchmark's fixture preparation")
    workspace = work / "fixture"
    home, stop = work / "home", workspace / "stop"
    cache_existed = home.exists()
    target_existed = (workspace / "target").exists()
    log_path = work / "logs" / f"{run_id}.jsonl"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment.pop("CARGO_TARGET_DIR", None)
    environment.pop("RAWR_VERBOSE", None)
    environment.update({
        "RAWR_HOME": str(home), "TICKER_STOP": str(stop), "CARGO_BUILD_JOBS": "2",
        "PATH": str(directory) + os.pathsep + environment.get("PATH", ""),
    })
    result = {
        "schema": 1, "benchmark": "save-to-native-callback-output", "release": version,
        "run_id": run_id, "label": args.label, "started_utc": utc(), "status": "incomplete",
        "release_revision": args.release_revision,
        "scope": "body-only edit; file write begins to first observed newline with the new callback result; not GUI paint",
        "clock": "time.monotonic_ns", "native_tick_interval_ms": TICK_MS,
        "fixture": {"source": str(FIXTURE.relative_to(ROOT)), "template_sha256": files_digest(pristine),
                    "prepared_sha256": files_digest(prepared), "initial_factor": 1, "edit_factors": list(FACTORS)},
        "binaries": installed,
        "cache": {"rawr_home_existed": cache_existed, "project_target_existed": target_existed,
                  "policy": "persistent benchmark-owned rawr home and fixture target"},
        "build_jobs": 2, "samples": [], "log_relative_to_work_dir": str(log_path.relative_to(work)),
        "harness_sha256": digest(Path(__file__)),
    }
    running = None
    try:
        for name, content in prepared.items():
            path = workspace / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        stop.unlink(missing_ok=True)
        result.update({
            "machine": machine(), "load_before_start": load(),
            "rustc": command_text(["rustc", "-Vv"], workspace),
            "cargo": command_text(["cargo", "-V"], workspace),
        })
        with log_path.open("w") as log:
            running = Running([str(directory / "rawr"), "run", str(workspace)], environment, workspace, log)
            first = running.until(lambda observed: observed.value == observed.tick, args.startup_timeout)
            result["startup"] = {"to_first_callback_ms": (first.observed_ns - running.started_ns) / 1_000_000,
                                 "native_pid": first.pid, "first_tick": first.tick}
            running.until(lambda observed: observed.tick >= 3, args.edit_timeout)
            print(f"Ready: {version}, native PID {first.pid}; startup {result['startup']['to_first_callback_ms']:.3f} ms", flush=True)
            if args.wait_before_edits:
                print("Waiting for Enter before edits; callbacks continue to be observed.", flush=True)
                running.release(args.startup_timeout)
            result["load_before_edits"] = load()
            result["startup_to_edits_ms"] = (time.monotonic_ns() - running.started_ns) / 1_000_000
            for index, factor in enumerate(FACTORS):
                before = running.observation.tick
                body = prepared[main_name].replace(b"Scale { factor: 1 }", f"Scale {{ factor: {factor} }}".encode())
                sample_load = load()
                saved_ns = time.monotonic_ns()
                (workspace / main_name).write_bytes(body)
                written_ns = time.monotonic_ns()
                observed = running.until(lambda state: state.value == state.tick * factor, args.edit_timeout)
                sample = {
                    "edit": index + 1, "phase": "first" if index == 0 else "later", "factor": factor,
                    "latency_ms": (observed.observed_ns - saved_ns) / 1_000_000,
                    "save_write_ms": (written_ns - saved_ns) / 1_000_000,
                    "save_since_start_ns": saved_ns - running.started_ns,
                    "observed_since_start_ns": observed.observed_ns - running.started_ns,
                    "tick_before_save": before, "observed_tick": observed.tick,
                    "observed_value": observed.value, "native_pid": observed.pid, "load_before_save": sample_load,
                }
                if observed.tick <= before or sample["latency_ms"] < 0:
                    raise RuntimeError("new callback output did not follow this save")
                result["samples"].append(sample)
                print(f"Edit {index + 1} ({sample['phase']}): {sample['latency_ms']:.3f} ms; tick {before} -> {observed.tick}", flush=True)
                confirmation = observed.tick + 2
                running.until(lambda state: state.tick >= confirmation and state.value == state.tick * factor, args.edit_timeout)
            if binaries(directory) != installed:
                raise RuntimeError("installed binaries changed during the measurement")
            result["status"] = "complete"
    except (Exception, KeyboardInterrupt) as error:
        result["error"] = f"{type(error).__name__}: {public_path(str(error))}"
    finally:
        # A repeated Ctrl-C must not interrupt owned-process/source cleanup.
        handlers = {}
        if threading.current_thread() is threading.main_thread():
            for termination in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
                handlers[termination] = signal.signal(termination, signal.SIG_IGN)
        if running is not None:
            try:
                running.stop(stop)
            except Exception as error:
                result["status"] = "incomplete"
                result["cleanup_error"] = str(error)
            result["native_pid"] = running.observation.pid
            result["last_native_tick"] = running.observation.tick
            intervals = running.observation.intervals_ms
            result["observed_callback_intervals_ms"] = {
                "count": len(intervals), "median": statistics.median(intervals) if intervals else None,
                "max": max(intervals) if intervals else None,
            }
        for name, content in pristine.items():
            path = workspace / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        result["load_after"] = load()
        result["finished_utc"] = utc()
        result["summary"] = {"all_edits": summary(result["samples"]), "later_edits": summary(result["samples"][1:])}
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(result, indent=2) + "\n")
        print(f"Result: {output}", flush=True)
        for termination, handler in handlers.items():
            signal.signal(termination, handler)
    print(json.dumps(result["summary"], indent=2), flush=True)
    return 0 if result["status"] == "complete" else 1


if __name__ == "__main__":
    for termination in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(termination, interrupted)
    try:
        sys.exit(measure(arguments()))
    except (Exception, KeyboardInterrupt) as error:
        print(f"measurement failed: {error}", file=sys.stderr)
        sys.exit(1)
