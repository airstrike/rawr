#!/usr/bin/env python3
"""Observe the separate pinned spec binary in an owned diagnostic cwd."""

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
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    output = args.output.resolve()
    with tempfile.TemporaryDirectory(prefix="rawr-spec-reference-") as directory:
        root = Path(directory).resolve()
        (root / "probe").mkdir()
        (root / "probe/existing").write_bytes(b"")
        (root / "probe\\existing").write_bytes(b"")
        completed = subprocess.run([str(binary)], cwd=root, capture_output=True, check=False)
        rows = [json.loads(line) for line in completed.stdout.decode("utf-8").splitlines()]
        seen = set()
        statuses = {}
        source = []
        orders = []
        for row in rows:
            if row["schema"] != 1:
                raise ValueError("unknown spec reference schema")
            if row["row"] == "orders":
                orders.append(row)
                continue
            if row["row"] != "spec" or row["id"] in seen:
                raise ValueError("unknown row or duplicate spec case id")
            seen.add(row["id"])
            statuses[row["status"]] = statuses.get(row["status"], 0) + 1
            if row["status"] == "source":
                source.append((row["id"], row["input"]))
            changed = False
            for field in ("error", "error_debug"):
                if row[field] is not None:
                    text = row[field]
                    normalized = text.replace(root.as_uri(), "file://${FIXTURE_ROOT}").replace(str(root), "${FIXTURE_ROOT}")
                    changed |= normalized != text
                    row[field] = normalized
            row["diagnostic_cwd_elided"] = changed
        if not seen:
            raise ValueError("reference produced no spec cases")
        if completed.returncode == 0:
            if len(orders) != 1 or rows[-1]["row"] != "orders":
                raise ValueError("reference did not complete its ordering observations")
            if source != [("d317-reparse", "git+http://a%00/foo")]:
                raise ValueError("unexpected source-failure observations")
            if any(status not in {"ok", "error", "source"} for status in statuses):
                raise ValueError("unexpected reference status")
            for pair in orders[0]["pairs"]:
                if pair["left"] not in seen or pair["right"] not in seen:
                    raise ValueError("ordering refers to an absent case")
                if pair["ordering"] not in {-1, 0, 1} or pair["equal"] != (pair["ordering"] == 0):
                    raise ValueError("ordering/equality observations disagree")
        encoded = b"".join((json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n").encode("utf-8") for row in rows)
        destination = output if completed.returncode == 0 else output.with_suffix(output.suffix + ".failed")
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=destination.parent, prefix=".spec-reference-", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(encoded)
        temporary.replace(destination)
        print(json.dumps({"returncode": completed.returncode, "cases": len(seen),
            "statuses": statuses, "pairs": sum(len(row["pairs"]) for row in orders),
            "bytes": len(encoded), "sha256": hashlib.sha256(encoded).hexdigest(),
            "artifact": str(destination), "stderr": completed.stderr.decode("utf-8")}))
        if completed.returncode:
            raise SystemExit(completed.returncode)


if __name__ == "__main__":
    main()
