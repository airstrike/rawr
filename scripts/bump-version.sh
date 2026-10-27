#!/usr/bin/env bash
# Sets every rawr crate to one version, as each merge to main does.
#
# Usage: scripts/bump-version.sh [patch | <x.y.z>]
#
# With no argument or `patch`, the patch number of the root crate's version
# goes up by one. Every rawr crate's own version, the two pins between rawr
# and rawr_codegen, and the rawr crates in the iced profile's seed lock move
# to the new version, and `cargo update -w` refreshes Cargo.lock. Vendored
# forks (rawr-notify, rawr-toml-edit) keep their own versions, and fixture
# and example crates keep theirs.
set -euo pipefail

repo="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"
cd "$repo"

python3 -I - "${1:-patch}" <<'PY'
import re
import sys

CRATES = [".", "codegen", "canonical", "cli", "importer", "iced-cli", "directory",
          "toml", "import-cli", "macros", "location", "play", "iced", "watch", "web"]

def package_version(text):
    package = text[text.index("[package]"):]
    return re.search(r'^version = "(\d+)\.(\d+)\.(\d+)"$', package, re.M)

root = open("Cargo.toml").read()
match = package_version(root)
old = ".".join(match.groups())
wanted = sys.argv[1]
if wanted == "patch":
    major, minor, patch = map(int, match.groups())
    new = f"{major}.{minor}.{patch + 1}"
elif re.fullmatch(r"\d+\.\d+\.\d+", wanted):
    new = wanted
else:
    sys.exit(f"expected `patch` or a version like 0.2.1, not `{wanted}`")

for crate in CRATES:
    path = f"{crate}/Cargo.toml"
    text = open(path).read()
    head = text.index("[package]")
    found = package_version(text)
    if ".".join(found.groups()) != old:
        sys.exit(f"{path} is at {'.'.join(found.groups())}, not {old}")
    start, end = head + found.start(), head + found.end()
    text = text[:start] + f'version = "{new}"' + text[end:]
    text = text.replace(f'rawr_codegen = {{ version = "{old}", path = "codegen" }}',
                        f'rawr_codegen = {{ version = "{new}", path = "codegen" }}')
    text = text.replace(f'rawr = {{ path = "..", version = "{old}"',
                        f'rawr = {{ path = "..", version = "{new}"')
    open(path, "w").write(text)

lock = "iced/src/profile/resolution/current-v2.lock"
text = open(lock).read()
text = re.sub(rf'(name = "rawr[-_a-z]*"\nversion = )"{re.escape(old)}"', rf'\1"{new}"', text)
open(lock, "w").write(text)
print(f"{old} -> {new}")
PY

cargo update -w --quiet
