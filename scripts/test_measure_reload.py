"""Checks that the benchmark cannot report a restart or reset as a fast reload."""

import argparse
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

import measure_reload as benchmark


class Observations(unittest.TestCase):
    def test_callback_requires_one_native_pid_and_an_unbroken_counter(self):
        observed = benchmark.Observation()
        with self.assertRaises(RuntimeError):
            observed.consume(10, "stdout", "1 value 1")
        observed.consume(20, "stdout", "pid 123")
        self.assertTrue(observed.consume(30, "stdout", "1 value 1"))
        self.assertTrue(observed.consume(40, "stdout", "2 value 200"))
        self.assertEqual(observed.value, 200)
        self.assertEqual(observed.observed_ns, 40)
        for line in ("pid 123", "pid 456", "1 value 100", "4 value 400"):
            with self.subTest(line=line), self.assertRaises(RuntimeError):
                observed.consume(50, "stdout", line)

    def test_restart_diagnostic_is_not_accepted_as_a_body_reload(self):
        with self.assertRaises(RuntimeError):
            benchmark.Observation().consume(10, "stderr", "rawr: `main` changed; restarting the program")

    def test_reader_timestamp_excludes_delayed_consumer_processing(self):
        with tempfile.TemporaryDirectory() as directory, io.StringIO() as log:
            stop = Path(directory) / "stop"
            code = "import os; print('pid', os.getpid(), flush=True); print('1 value 1', flush=True)"
            run = benchmark.Running([sys.executable, "-c", code], os.environ, directory, log)
            try:
                run.child.wait(timeout=5)
                for reader in run.readers:
                    reader.join(timeout=5)
                consumed_ns = time.monotonic_ns()
                observed = run.until(lambda state: state.tick == 1, 5)
                self.assertLess(observed.observed_ns, consumed_ns)
            finally:
                run.stop(stop)

    def test_failed_start_restores_copy_and_preserves_owned_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            sentinel = work / "fixture/target/keep"
            sentinel.parent.mkdir(parents=True)
            sentinel.write_text("cached")
            args = argparse.Namespace(label="test", release_revision=None, wait_before_edits=False, startup_timeout=1, edit_timeout=1)
            output = work / "result.json"
            with mock.patch.object(benchmark, "command_text", return_value="test"), \
                    mock.patch.object(benchmark, "machine", return_value={}), \
                    mock.patch.object(benchmark, "Running", side_effect=OSError("cannot start")), \
                    contextlib.redirect_stdout(io.StringIO()):
                status = benchmark.measure_owned(args, work, {}, "test", "run", output, work)
            self.assertEqual(status, 1)
            self.assertEqual(json.loads(output.read_text())["status"], "incomplete")
            for name in ("src/main.rs", "ticker/src/lib.rs"):
                self.assertEqual((work / "fixture" / name).read_bytes(), (benchmark.FIXTURE / name).read_bytes())
            self.assertEqual(sentinel.read_text(), "cached")

    def test_interrupt_after_save_restores_source_and_stops_only_its_process(self):
        class Interrupted(benchmark.Running):
            calls = 0

            def until(self, predicate, timeout):
                self.calls += 1
                if self.calls == 3:
                    raise KeyboardInterrupt("interrupted after source save")
                return super().until(predicate, timeout)

        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            binary = work / "rawr"
            binary.write_text(f"#!{sys.executable}\n" + """
import os
from pathlib import Path
import time
print('pid', os.getpid(), flush=True)
tick = 0
while not Path(os.environ['TICKER_STOP']).exists():
    tick += 1
    print(f'{tick} value {tick}', flush=True)
    time.sleep(0.005)
""")
            binary.chmod(0o755)
            unrelated = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
            args = argparse.Namespace(label="test", release_revision=None, wait_before_edits=False, startup_timeout=5, edit_timeout=5)
            output = work / "result.json"
            try:
                with mock.patch.object(benchmark, "command_text", return_value="test"), \
                        mock.patch.object(benchmark, "machine", return_value={}), \
                        mock.patch.object(benchmark, "Running", Interrupted), \
                        contextlib.redirect_stdout(io.StringIO()):
                    status = benchmark.measure_owned(args, work, {}, "test", "run", output, work)
                result = json.loads(output.read_text())
                self.assertEqual(status, 1)
                self.assertIn("KeyboardInterrupt", result["error"])
                self.assertIsNone(unrelated.poll())
                with self.assertRaises(ProcessLookupError):
                    os.kill(result["native_pid"], 0)
                self.assertEqual((work / "fixture/src/main.rs").read_bytes(), (benchmark.FIXTURE / "src/main.rs").read_bytes())
            finally:
                unrelated.terminate()
                unrelated.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
