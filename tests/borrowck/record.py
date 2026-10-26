#!/usr/bin/env python3
"""Record the pinned rustc's verdict on each program of the borrow corpus.

    python3 -I tests/borrowck/record.py [folders or files...]

With folders, or with none for every folder, it records each `.rs` under a
corpus folder and writes that folder's `rustc.tsv`: one row per program, its
path relative to `tests/borrowck`, rustc's error codes with their lines (`E0502:7`,
`lifetime:3` for "lifetime may not live long enough", which has no code), or `ok`
followed by what the program's binary printed (escaped), which the
differential harness compares with rawr's run. With `.rs` files, it prints
the rows instead (for picking cases). Builds go under `target/borrowck`.

The corpus: `ui/` holds the cases of rustc 1.96.0's tests/ui/borrowck and
tests/ui/nll that rawr's frontend lowers (rustc's tests, MIT OR Apache-2.0
as rawr is), `slice1/` the cases written for borrows of locals, and
`mutations/` what `mutate.py` writes from rawr's own programs, and
`patterns/` the cases written for patterns' bindings and reads, with
`patterns/mutations/` what `mutate.py` writes from them, and
`signatures/` the cases written for a body checked against its own
signature and for temporaries (E0515, E0716, E0106). The harness
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
        # rustc gives this region error no code: `lifetime` names it here.
        if code == "error" and message.get("message") == "lifetime may not live long enough":
            code = "lifetime"
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
    arguments = sys.argv[1:]
    files = [path for path in arguments if path.endswith(".rs")]
    for path in files:
        print(f"{path}\t{verdict(os.path.abspath(path))}")
    if files:
        return
    folders = [os.path.abspath(path) for path in arguments] or [
        os.path.join(HERE, name) for name in sorted(os.listdir(HERE)) if os.path.isdir(os.path.join(HERE, name))
    ]
    for folder in folders:
        rows = []
        for directory, _, names in sorted(os.walk(folder)):
            for name in sorted(names):
                if name.endswith(".rs"):
                    path = os.path.join(directory, name)
                    rows.append(f"{os.path.relpath(path, HERE)}\t{verdict(path)}")
        with open(os.path.join(folder, "rustc.tsv"), "w") as out:
            out.write(f"# {version.stdout.strip()}, edition {EDITION}\n")
            out.write("\n".join(rows) + "\n")

if __name__ == "__main__":
    main()
