#!/usr/bin/env python3
"""Write mutations of rawr's own programs into tests/borrowck/mutations.

    python3 -I tests/borrowck/mutate.py [out sources...]

With an output folder and sources, it mutates those programs into that
folder instead (`tests/borrowck/patterns/mutations` from the pattern
cases). For each `let mut x` in a program, and each later statement in its block
that names `x`, it writes variants that move a borrow's use past that
statement, into a branch, and across a loop's back edge, and variants
where the borrow ends first. rustc decides which are errors: run
`record.py` afterwards.
"""

import glob
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
OUT = os.path.join(HERE, "mutations")
SOURCES = sorted(
    glob.glob(os.path.join(ROOT, "play/tests/fixtures/rust/*.rs"))
    + [os.path.join(ROOT, "examples/cart.rs"), os.path.join(ROOT, "examples/nominal_counter.rs")]
    + glob.glob(os.path.join(HERE, "slice1/*.rs"))
)
HELPERS = """
fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool { false }
"""
DECLARATION = re.compile(r"^(\s*)let mut (\w+)\b[^=]*=.*;\s*$")
PER_LOCAL = 2


def statements(lines, start, indent, name):
    """The single-line statements after `start` in its block that name `name`."""
    word = re.compile(rf"\b{re.escape(name)}\b")
    for index in range(start + 1, len(lines)):
        line = lines[index]
        if line.strip() and len(line) - len(line.lstrip()) < len(indent):
            return
        stripped = line.strip()
        if (
            line.startswith(indent)
            and not line[len(indent)].isspace()
            and stripped.endswith(";")
            and not stripped.startswith(("let ", "return", "break", "continue"))
            and stripped.count("{") == stripped.count("}")
            and word.search(stripped)
        ):
            yield index


def variants(indent, name, statement):
    i = indent
    return {
        "mut_across": [f"{i}let __b = &mut {name};", statement, f"{i}__touch(__b);"],
        "mut_ended": [f"{i}let __b = &mut {name};", f"{i}__touch(__b);", statement],
        "shared_across": [f"{i}let __s = &{name};", statement, f"{i}__see(__s);"],
        "into_branch": [f"{i}let __b = &mut {name};", statement, f"{i}if __c() {{ __touch(__b); }}"],
        "other_arm": [
            f"{i}let __b = &mut {name};",
            f"{i}if __c() {{ __touch(__b); }} else {{",
            statement,
            f"{i}}}",
        ],
        "back_edge": [
            f"{i}let __b = &mut {name};",
            f"{i}for __i in 0..2 {{",
            f"{i}    __touch(__b);",
            statement,
            f"{i}}}",
        ],
        "fresh_each_iteration": [
            f"{i}for __i in 0..2 {{",
            f"{i}    let __b = &mut {name};",
            f"{i}    __touch(__b);",
            statement,
            f"{i}}}",
        ],
    }


def main():
    out, sources = OUT, SOURCES
    if len(sys.argv) > 2:
        out, sources = sys.argv[1], sorted(sys.argv[2:])
    os.makedirs(out, exist_ok=True)
    for old in glob.glob(os.path.join(out, "*.rs")):
        os.remove(old)
    count = 0
    for source in sources:
        stem = os.path.splitext(os.path.basename(source))[0]
        lines = open(source, encoding="utf-8").read().splitlines()
        for start, line in enumerate(lines):
            declared = DECLARATION.match(line)
            if not declared:
                continue
            indent, name = declared.group(1), declared.group(2)
            for target in list(statements(lines, start, indent, name))[:PER_LOCAL]:
                for kind, replacement in variants(indent, name, lines[target]).items():
                    mutated = lines[:target] + replacement + lines[target + 1 :]
                    path = os.path.join(out, f"{stem}_{name}_{target + 1}_{kind}.rs")
                    with open(path, "w", encoding="utf-8") as file:
                        file.write("\n".join(mutated) + "\n" + HELPERS)
                    count += 1
    # As the repository's other Rust files are.
    written = sorted(glob.glob(os.path.join(out, "*.rs")))
    subprocess.run(["rustfmt", "+stable", "--edition", "2024", *written], check=True)
    print(count)


if __name__ == "__main__":
    main()
