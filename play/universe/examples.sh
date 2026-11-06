#!/bin/sh
# The universe's programs (#1000): each iced example `examples.txt` names,
# from iced at the universe's pinned rev, and each of ?play's own in
# `programs/`, as a module of the universe program (`src/examples/`). A
# module with its own `main` is a program of its own to the runner, so a
# snippet that makes the native calls one of them makes runs as it does.
#
# The image's universe stage runs it before it builds the runner. Locally:
#   sh play/universe/examples.sh
#   RAWR_UNIVERSE_RUNNER=1 rawr-import run play/universe
set -eu

universe="$(cd "$(dirname "$0")" && pwd)"

# iced's checkout: the directory whose `widget/Cargo.toml` is iced_widget's,
# as Cargo resolved the universe's iced.
manifest="$(cargo metadata --format-version 1 --manifest-path "$universe/Cargo.toml" \
  | grep -o '"manifest_path":"[^"]*/widget/Cargo.toml"' | head -n 1)"
iced="${manifest#\"manifest_path\":\"}"
iced="${iced%/widget/Cargo.toml\"}"
if [ ! -d "$iced/examples" ]; then
  echo "examples.sh: no iced checkout with examples at '$iced'" >&2
  exit 1
fi

out="$universe/src/examples"
rm -rf "$out"
mkdir -p "$out"
{
  echo "//! The universe's programs, written by examples.sh: each module is"
  echo "//! iced's example of its name, or ?play's own (programs/)."
} > "$universe/src/examples.rs"

add() {
  echo "mod $1;" >> "$universe/src/examples.rs"
}

# One name per line; `#` starts a comment.
sed -e 's/#.*//' -e 's/[[:space:]]//g' "$universe/examples.txt" | while read -r name; do
  [ -n "$name" ] || continue
  source="$iced/examples/$name/src"
  if [ ! -f "$source/main.rs" ]; then
    echo "examples.sh: iced has no example '$name'" >&2
    exit 1
  fi
  cp "$source/main.rs" "$out/$name.rs"
  # An example's other files are its main module's, below its name.
  for file in "$source"/*; do
    case "$file" in
      "$source/main.rs") ;;
      *)
        mkdir -p "$out/$name"
        cp -R "$file" "$out/$name/"
        ;;
    esac
  done
  add "$name"
done

for program in "$universe"/programs/*.rs; do
  name="$(basename "$program" .rs)"
  cp "$program" "$out/$name.rs"
  add "$name"
done
