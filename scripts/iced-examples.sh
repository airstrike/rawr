#!/usr/bin/env bash
# Copies iced master's examples into standalone scratch crates for `rawr check`.
#
# Usage: scripts/iced-examples.sh [example ...]
#
# Each example goes to target/scratch/iced-examples/<name>. Every crate root
# .rs file gets `// rawr` as its first line. iced comes from the git rev that
# rawr-iced uses, with the example's own features. Other workspace
# dependencies are resolved from iced's workspace table. Each Cargo.toml gets
# an empty `[workspace]` table. All crates share one target directory,
# target/scratch/iced-examples/_target, through CARGO_TARGET_DIR. A
# `.cargo/config.toml` beside them names the host as the build target.
#
# Check one crate:
#   cd target/scratch/iced-examples/todos
#   CARGO_TARGET_DIR=../_target rawr check
set -euo pipefail

repo="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"
out="$repo/target/scratch/iced-examples"
rev="$(sed -n 's/^iced = { git = "https:\/\/github.com\/iced-rs\/iced", rev = "\([0-9a-f]*\)".*/\1/p' "$repo/iced/Cargo.toml" | head -n 1)"
if [ -z "$rev" ]; then
    echo "could not read iced's git rev from iced/Cargo.toml" >&2
    exit 1
fi
iced_src="${ICED_SRC:-$HOME/.cargo/git/checkouts/iced-3e930cb38a75fe30/${rev:0:7}}"
if [ ! -d "$iced_src/examples" ]; then
    echo "no iced checkout at $iced_src; run cargo fetch in $repo/iced first" >&2
    exit 1
fi

mkdir -p "$out/.cargo"
# Every crate builds for the host named as a target, as Rawr builds a
# project that imports Rust, so `cargo check` and Rawr share their units.
printf '[build]\ntarget = "host-tuple"\n' > "$out/.cargo/config.toml"
if [ "$(cd "$(dirname "$0")" && pwd)" != "$out" ]; then
    cp "$0" "$out/setup.sh"
fi

python3 -I - "$iced_src" "$out" "$rev" "$@" <<'PY'
import os, re, shutil, sys, tomllib

iced_src, out, rev = sys.argv[1], sys.argv[2], sys.argv[3]
only = sys.argv[4:]
git = "https://github.com/iced-rs/iced"

with open(os.path.join(iced_src, "Cargo.toml"), "rb") as f:
    workspace = tomllib.load(f)["workspace"]["dependencies"]

def resolve(name, spec):
    """Turns a workspace or path dependency into a standalone one."""
    if not isinstance(spec, dict):
        return spec
    spec = dict(spec)
    package = spec.get("package", name)
    if spec.pop("workspace", False):
        base = workspace[name]
        base = {"version": base} if isinstance(base, str) else dict(base)
        features = base.get("features", []) + spec.pop("features", [])
        base.update(spec)
        if features:
            base["features"] = features
        spec = base
    if package == "iced" or package.startswith("iced_"):
        spec.pop("path", None)
        spec.pop("version", None)
        spec = {"git": git, "rev": rev, **spec}
    return spec

def bare(key):
    return key if re.fullmatch(r"[A-Za-z0-9_-]+", key) else '"%s"' % key.replace('"', '\\"')

def value(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return str(v)
    if isinstance(v, str):
        return '"%s"' % v.replace("\\", "\\\\").replace('"', '\\"')
    if isinstance(v, list):
        return "[" + ", ".join(value(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{ " + ", ".join("%s = %s" % (bare(k), value(x)) for k, x in v.items()) + " }"
    raise TypeError(v)

DEPS = ("dependencies", "dev-dependencies", "build-dependencies")

def emit(path, table, lines):
    scalars, tables, arrays = [], [], []
    for k, v in table.items():
        if path and path[-1] in DEPS:
            scalars.append((k, resolve(k, v)))
        elif isinstance(v, dict):
            tables.append((k, v))
        elif isinstance(v, list) and v and all(isinstance(x, dict) for x in v):
            arrays.append((k, v))
        else:
            scalars.append((k, v))
    if path and (scalars or not tables):
        lines.append("")
        lines.append("[%s]" % ".".join(bare(p) for p in path))
    for k, v in scalars:
        lines.append("%s = %s" % (bare(k), value(v)))
    for k, v in tables:
        emit(path + [k], v, lines)
    for k, v in arrays:
        for item in v:
            lines.append("")
            lines.append("[[%s]]" % ".".join(bare(p) for p in path + [k]))
            for ik, iv in item.items():
                lines.append("%s = %s" % (bare(ik), value(iv)))

examples = os.path.join(iced_src, "examples")
names = sorted(n for n in os.listdir(examples) if os.path.isfile(os.path.join(examples, n, "Cargo.toml")))
if only:
    names = [n for n in names if n in only]

for name in names:
    src = os.path.join(examples, name)
    dst = os.path.join(out, name)
    if os.path.exists(dst):
        shutil.rmtree(dst)
    shutil.copytree(src, dst)
    for root, _, files in os.walk(dst):
        for f in files:
            os.chmod(os.path.join(root, f), 0o644)

    with open(os.path.join(src, "Cargo.toml"), "rb") as f:
        manifest = tomllib.load(f)
    manifest.pop("lints", None)
    manifest.pop("workspace", None)
    lines = []
    emit([], manifest, lines)
    lines += ["", "[workspace]", ""]
    with open(os.path.join(dst, "Cargo.toml"), "w") as f:
        f.write("\n".join(lines).lstrip("\n"))

    roots = ["src/main.rs", "src/lib.rs"]
    roots += [b["path"] for b in manifest.get("bin", []) if "path" in b]
    for root in roots:
        path = os.path.join(dst, root)
        if not os.path.isfile(path):
            continue
        with open(path) as f:
            text = f.read()
        if not text.startswith("// rawr\n"):
            with open(path, "w") as f:
                f.write("// rawr\n" + text)
    print(name)
PY
