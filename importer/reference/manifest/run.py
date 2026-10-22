#!/usr/bin/env python3
"""Run the built reference in an owned diagnostic cwd; atomically emit JSONL."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--case")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    output = args.output.resolve()
    with tempfile.TemporaryDirectory(prefix="rawr-manifest-reference-") as directory:
        root = Path(directory).resolve()
        (root / "probe").mkdir()
        (root / "probe/existing").write_bytes(b"")
        # On Unix this is a single filename, exercising the other slash branch.
        (root / "probe\\existing").write_bytes(b"")
        command = [str(binary)]
        if args.case:
            command += ["--case", args.case]
        completed = subprocess.run(command, cwd=root, capture_output=True, check=False)
        rows = []
        ids = set()
        for line in completed.stdout.decode("utf-8").splitlines():
            row = json.loads(line)
            if row["id"] in ids:
                raise ValueError("duplicate fixture id")
            ids.add(row["id"])
            changed = False
            if row["error"] is not None:
                for field in ("message", "display"):
                    text = row["error"][field]
                    normalized = text.replace(root.as_uri(), "file://${FIXTURE_ROOT}").replace(str(root), "${FIXTURE_ROOT}")
                    changed |= normalized != text
                    row["error"][field] = normalized
            row["diagnostic_cwd_elided"] = changed
            rows.append(row)
        if not rows:
            raise ValueError("reference produced no observations")
        encoded = b"".join((json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n").encode("utf-8") for row in rows)
        # Preserve actual mismatches for inspection, without publishing them as
        # a successful artifact. No hand-authored golden replaces these rows.
        destination = output if completed.returncode == 0 else output.with_suffix(output.suffix + ".failed")
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=destination.parent, prefix=".reference-", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(encoded)
        temporary.replace(destination)
        print(json.dumps({"returncode": completed.returncode, "cases": len(rows),
            "bytes": len(encoded), "sha256": hashlib.sha256(encoded).hexdigest(),
            "artifact": str(destination), "stderr": completed.stderr.decode("utf-8")}))
        if completed.returncode:
            raise SystemExit(completed.returncode)


if __name__ == "__main__":
    main()
