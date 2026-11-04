#!/bin/sh
# Writes what rustc's build of each playground example prints, which
# play/tests/cli.rs compares rawr's run with: NAME.stdout and NAME.status
# for a program that builds, NAME.stderr and status 1 for one rustc refuses.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
build="$here/../../../../../target/play-expected"
mkdir -p "$build"
cd "$here/.."
for source in *.rs; do
    name=${source%.rs}
    rm -f "$here/$name.stdout" "$here/$name.stderr"
    if rustc --edition 2024 -o "$build/$name" "$source" 2>"$build/$name.build"; then
        status=0
        "$build/$name" >"$here/$name.stdout" || status=$?
    else
        status=1
        cp "$build/$name.build" "$here/$name.stderr"
    fi
    echo "$status" >"$here/$name.status"
done
rm -rf "$build"
