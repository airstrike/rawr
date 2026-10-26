#!/usr/bin/env python3
"""Record the pinned rustc's verdict on each program of the borrow corpus.

    python3 -I tests/borrowck/record.py [files or a folder...]

With no files, it records every `.rs` under `tests/borrowck` and writes
`tests/borrowck/rustc.tsv`: one row per program, its path relative to this
directory, rustc's error codes with their lines (`E0502:7`), or `ok`
followed by what the program's binary printed (escaped), which the
differential harness compares with rawr's run. With a folder of the
corpus, it records the programs under it and replaces only that folder's
rows. With files, it prints the rows instead (for picking cases). Builds
go under `target/borrowck`.

The corpus: `ui/` holds the cases of rustc 1.96.0's tests/ui/borrowck and
tests/ui/nll that rawr's frontend lowers (rustc's tests, MIT OR Apache-2.0
as rawr is), `slice1/` the cases written for borrows of locals, and
`mutations/` what `mutate.py` writes from rawr's own programs, and
`patterns/` the cases written for patterns' bindings and reads, with
`patterns/mutations/` what `mutate.py` writes from them. The harness
is `borrowck_differential` in rawr's lib tests (src/resolve/borrowck).
"""

import json
import os
import subprocess
import sys

TOOLCHAIN = "stable"
PINNED = "rustc 1.96.0 "
EDITION = "2024"
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
BUILD = os.path.join(ROOT, "target", "borrowck")


def verdict(path):
    os.makedirs(BUILD, exist_ok=True)
    binary = os.path.join(BUILD, "case")
    if os.path.exists(binary):
        os.remove(binary)
    with open(path, encoding="utf-8") as file:
        library = "fn main" not in file.read()
    kind = ["--crate-type=lib"] if library else []
    compiled = subprocess.run(
        ["rustc", f"+{TOOLCHAIN}", f"--edition={EDITION}", "--error-format=json",
         "-A", "warnings", *kind, "-o", binary, path],
        capture_output=True, text=True, cwd=BUILD,
    )
    codes = []
    for line in compiled.stderr.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("level") != "error" or message.get("message", "").startswith("aborting"):
            continue
        code = (message.get("code") or {}).get("code") or "error"
        spans = [span for span in message.get("spans", []) if span.get("is_primary")]
        line = spans[0]["line_start"] if spans else 0
        codes.append(f"{code}:{line}")
    if codes or compiled.returncode != 0:
        return " ".join(codes) or "error:0"
    if library:
        return "ok\tlib\t\"\""
    try:
        ran = subprocess.run([binary], capture_output=True, timeout=10)
        output = ran.stdout.decode("utf-8", "replace")
        status = ran.returncode
    except subprocess.TimeoutExpired:
        output, status = "", "timeout"
    return f"ok\t{status}\t{json.dumps(output)}"


def main():
    version = subprocess.run(["rustc", f"+{TOOLCHAIN}", "--version"], capture_output=True, text=True)
    if not version.stdout.startswith(PINNED):
        sys.exit(f"the corpus is recorded with {PINNED.strip()}, not {version.stdout.strip()}")
    files = sys.argv[1:]
    if len(files) == 1 and os.path.isdir(files[0]):
        record(os.path.abspath(files[0]), version.stdout.strip())
        return
    if files:
        for path in files:
            print(f"{path}\t{verdict(os.path.abspath(path))}")
        return
    record(HERE, version.stdout.strip())


def record(folder, version):
    """Record the programs under `folder` into rustc.tsv, keeping the rows
    of the programs outside it, in the order a whole run writes them."""
    prefix = os.path.relpath(folder, HERE)
    if prefix.startswith(".."):
        sys.exit(f"{folder} is not in the corpus")
    inside = lambda path: prefix == "." or path == prefix or path.startswith(prefix + os.sep)
    rows = {}
    table = os.path.join(HERE, "rustc.tsv")
    if prefix != "." and os.path.exists(table):
        for line in open(table, encoding="utf-8").read().splitlines():
            if line and not line.startswith("#"):
                path = line.split("\t", 1)[0]
                if not inside(path):
                    rows[path] = line
    for directory, _, names in os.walk(folder):
        for name in names:
            if name.endswith(".rs"):
                path = os.path.join(directory, name)
                relative = os.path.relpath(path, HERE)
                rows[relative] = f"{relative}\t{verdict(path)}"
    order = sorted(rows, key=lambda path: (os.path.dirname(path), os.path.basename(path)))
    with open(table, "w") as out:
        out.write(f"# {version}, edition {EDITION}\n")
        out.write("\n".join(rows[path] for path in order) + "\n")


if __name__ == "__main__":
    main()
