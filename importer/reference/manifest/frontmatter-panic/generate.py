#!/usr/bin/env python3
"""Regenerate the 335 frontmatter observations with an existing reference binary."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


CASE_COUNT = 335
CORPUS = Path(__file__).with_name("expansion.jsonl")


def rows(data):
    # JSONL uses LF, not every Unicode character recognized by splitlines().
    if not data.endswith(b"\n"):
        raise ValueError("JSONL must end with LF")
    return [json.loads(line.decode("utf-8")) for line in data[:-1].split(b"\n")]


def status(row):
    fields = set(row)
    if fields == {"source", "output"} and isinstance(row["output"], str):
        return "success"
    if fields == {"source", "panic"} and row["panic"] is True:
        return "panic"
    if fields == {"source", "error", "primary", "visible"}:
        if not isinstance(row["error"], str) or not isinstance(row["visible"], list):
            raise ValueError("invalid reference error observation")
        for span in [row["primary"], *row["visible"]]:
            if not isinstance(span, dict) or set(span) != {"start", "end"}:
                raise ValueError("invalid reference span")
            if any(type(span[key]) is not int for key in ("start", "end")):
                raise ValueError("noninteger reference span")
            if not 0 <= span["start"] <= span["end"]:
                raise ValueError("reversed reference span")
        return "error"
    raise ValueError("unknown reference observation shape")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--sources", type=Path, default=CORPUS)
    parser.add_argument("--output", type=Path, default=CORPUS)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    destination = args.output.resolve()

    # Only the source strings are inputs. No saved outcome enters the reference
    # invocation or supplies a newly generated result.
    inventory = rows(args.sources.read_bytes())
    if len(inventory) != CASE_COUNT:
        raise ValueError(f"expected {CASE_COUNT} source cases, got {len(inventory)}")
    sources = []
    for row in inventory:
        if not isinstance(row, dict) or not isinstance(row.get("source"), str):
            raise ValueError("source case must contain a string named source")
        sources.append(row["source"])
    request = b"".join((json.dumps(source) + "\n").encode("utf-8") for source in sources)
    completed = subprocess.run([str(binary)], input=request, capture_output=True, check=False)
    sys.stderr.buffer.write(completed.stderr)
    completed.check_returncode()

    observed = rows(completed.stdout)
    if len(observed) != len(sources):
        raise ValueError(f"reference returned {len(observed)} rows for {len(sources)} sources")
    counts = {"success": 0, "error": 0, "panic": 0}
    for source, row in zip(sources, observed):
        if not isinstance(row, dict) or row.get("source") != source:
            raise ValueError("reference changed source identity or result order")
        counts[status(row)] += 1
    encoded = b"".join(
        (json.dumps(row, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n")
        .encode("utf-8")
        for row in observed
    )

    # A failed process or incomplete/malformed output cannot replace the corpus.
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            dir=destination.parent, prefix=".frontmatter-reference-", delete=False
        ) as stream:
            temporary = Path(stream.name)
            stream.write(encoded)
        temporary.replace(destination)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    print(json.dumps({
        "cases": len(observed),
        "counts": counts,
        "bytes": len(encoded),
        "sha256": hashlib.sha256(encoded).hexdigest(),
        "artifact": str(destination),
    }, sort_keys=True))


if __name__ == "__main__":
    main()
