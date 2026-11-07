#!/usr/bin/env bash
# Run every rustlings solution and exercise under rawr and under cargo, the
# way rustlings runs them, and compare them.
#
#   scripts/rustlings.sh [RUSTLINGS_DIR] [OUT_DIR]
#
# RUSTLINGS_DIR is a checkout of github.com/rust-lang/rustlings (default
# target/rustlings). OUT_DIR (default target/rustlings-out) gets the
# package, every output and `results.tsv`, one row per run:
#
#   kind  file  rustc  rawr  verdict  detail
#
# The package is the one `rustlings init` makes: rustlings' own Cargo.toml,
# whose `[[bin]]`s are every exercise (`name`) and solution (`name_sol`),
# with its dependencies (tokio), lints and profiles, and the exercises'
# directories with their input files. Every `.rs` in it starts with
# `// rawr`. As rustlings does, the cargo side builds a file with `cargo
# build --bin`, runs the built binary in the file's `exercises/<dir>`, and
# tests it with `cargo test --bin` from the package. The rawr side runs the
# package's file with `rawr run` and `rawr test` from the same directories.
#
# - `solution` rows run `fn main` both ways. They match when stdout and the
#   exit status are equal. A solution that spawns threads prints in the
#   order its threads finish, and their timings, which differ between two
#   runs of one build: its stdout matches as sorted lines with each number
#   followed by `ms` masked.
# - `test` rows are one `#[test]` of a solution. Its tests run once each
#   way, and each test's result in libtest's report (`ok`, `FAILED` or
#   `ignored`) is compared. A test whose build fails has the build's error
#   code (`build:CODE`).
# - `exercise` rows run an unsolved exercise both ways. Each side's outcome
#   is its first error code, else the program's exit status, else the
#   outcome of its tests (`tests-build:CODE` or `tests-fail`). They match
#   when both fail to compile with the same first error's code (`error`
#   for one with none), or both run alike.
#
# `rawr` is the one on PATH. RAWR_BIN names a development build's directory
# (with `rawr-import` and `rawr-import-driver`) to put first on PATH.
# RUSTC_TOOLCHAIN (default stable) picks cargo's toolchain; RAWR_TIMEOUT
# (default 120) bounds each run in seconds. RUSTLINGS_KINDS (default
# `solutions exercises`) picks which to run.
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
rustlings=${1:-$root/target/rustlings}
out=${2:-$root/target/rustlings-out}
toolchain=${RUSTC_TOOLCHAIN:-stable}
limit=${RAWR_TIMEOUT:-120}
kinds=${RUSTLINGS_KINDS:-solutions exercises}
[ -n "${RAWR_BIN:-}" ] && PATH=$RAWR_BIN:$PATH

[ -f "$rustlings/dev-Cargo.toml" ] || {
    echo "no rustlings checkout at $rustlings" >&2
    exit 2
}
rustlings=$(cd "$rustlings" && pwd)
rm -rf "$out"
mkdir -p "$out"
# Runs change directory, so every path is absolute.
out=$(cd "$out" && pwd)
results=$out/results.tsv
printf 'kind\tfile\trustc\trawr\tverdict\tdetail\n' >"$results"

# The package, as `rustlings init` writes it: dev-Cargo.toml without its
# first line, its paths from the package's own directory. It sits in this
# repository's tree, so it is a workspace of its own; the table goes last,
# after the manifest's top-level `bin` array.
package=$out/package
manifest=$package/Cargo.toml
mkdir -p "$package"
cp -R "$rustlings/exercises" "$rustlings/solutions" "$package/"
for source in "$package"/exercises/*/*.rs "$package"/solutions/*/*.rs; do
    { echo '// rawr'; cat "$source"; } >"$source.marked"
    mv "$source.marked" "$source"
done
{
    sed -e '1d' -e 's;"\.\./;";' "$rustlings/dev-Cargo.toml"
    echo
    echo '[workspace]'
} >"$manifest"
export CARGO_TARGET_DIR=$package/target
cargo=(cargo "+$toolchain")

row() { printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$@" >>"$results"; }

# The code of the first error in a file (`E0308`, or rawr's own such as
# `RAWR-RESOLVE-…`), else `error` when that error has none, else nothing.
# rawr stops at its first error, so a later one of rustc's never counts.
code() {
    local first
    first=$(grep -m1 '^error' "$1")
    case $first in
        "") ;;
        error\[*)
            first=${first#error[}
            echo "${first%%]*}"
            ;;
        *) echo error ;;
    esac
}

# The first error line (else the first stderr line), on one line.
first_error() {
    { grep -m1 '^error' "$1" || head -1 "$1"; } | cut -c1-200 | tr '\t' ' '
}

# Build the bin `$1` of the package, cargo's output to `$2`.
native_build() {
    "${cargo[@]}" build -q --manifest-path "$manifest" --bin "$1" 2>"$2"
}

# Run a command with the time limit; stdout to `$1.out`, stderr to
# `$1.err`. Prints the exit status.
run() {
    local prefix=$1
    shift
    timeout "$limit" "$@" >"$prefix.out" 2>"$prefix.err" </dev/null
    echo $?
}

# `cargo test --bin $2` from the package, as rustlings runs it, at `$1`,
# with libtest's report of each test, which `-q` would leave out. Prints
# the exit status.
native_tests() {
    (cd "$package" && run "$1" "${cargo[@]}" test -q --bin "$2" -- --test-threads=1 --format pretty)
}

# Whether the outputs `$1` and `$2` of the solution `$3` match: equal, or
# for one that spawns threads, equal as sorted lines with timings masked,
# or for one that prints the clock's time, equal with its numbers masked:
# two runs of one build differ there.
same_output() {
    cmp -s "$1" "$2" && return
    if grep -q 'SystemTime::now' "$3"; then
        local numbers='s/[0-9][0-9]*/<n>/g'
        cmp -s <(sed "$numbers" "$1") <(sed "$numbers" "$2")
        return
    fi
    grep -q 'thread::spawn' "$3" || return
    local masked='s/[0-9][0-9]*ms/<n>ms/g'
    cmp -s <(sed "$masked" "$1" | sort) <(sed "$masked" "$2" | sort)
}

# The `#[test]` functions of a file as `module::name`.
tests_of() {
    perl -ne '
        $module = $1 if $cfg && /^\s*mod\s+(\w+)/;
        $cfg = /#\[cfg\(test\)\]/;
        $test = 1 if /#\[test\]/;
        if ($test && /^\s*fn\s+(\w+)/) { print "$module\::$1\n"; $test = 0 }
    ' "$1"
}

# The result of the test `$2` (as `module::name`) in the libtest report
# `$1`: `ok`, `FAILED` or `ignored`, else nothing.
outcome() {
    sed -n "s/^test ${2#::}\( - should panic\)\{0,1\} \.\.\. \([a-zA-Z]*\).*/\2/p" "$1" | head -1
}

# `$1` is `solutions/<dir>/<name>.rs`, the bin `<name>_sol`, run in
# `exercises/<dir>`.
solution() {
    local source=$package/$1 name=$2 dir=$out/solutions/$2
    local bin=${name##*/}_sol cwd=$package/exercises/${name%%/*}
    mkdir -p "$dir"
    local native rawr verdict detail=
    if native_build "$bin" "$dir/native.build"; then
        native=$(cd "$cwd" && run "$dir/native" "$CARGO_TARGET_DIR/debug/$bin")
    else
        native="build:$(code "$dir/native.build")"
    fi
    rawr=$(cd "$cwd" && run "$dir/rawr" rawr run "$source")
    if [ "$native" = "$rawr" ] && same_output "$dir/native.out" "$dir/rawr.out" "$source"; then
        verdict=pass
    else
        verdict=fail
        if [ "$native" = "$rawr" ]; then
            detail="stdout differs"
        else
            detail=$(first_error "$dir/rawr.err")
        fi
    fi
    row solution "$name" "$native" "$rawr" "$verdict" "$detail"

    grep -q '#\[test\]' "$source" || return
    local native_build_code= rawr_code
    native_tests "$dir/tests" "$bin" >/dev/null
    grep -q '^test result:' "$dir/tests.out" || native_build_code="build:$(code "$dir/tests.err")"
    (cd "$package" && run "$dir/rawr-tests" rawr test "$source") >/dev/null
    rawr_code=$(code "$dir/rawr-tests.err")
    local test
    for test in $(tests_of "$source"); do
        native=${native_build_code:-$(outcome "$dir/tests.out" "$test")}
        rawr=$(outcome "$dir/rawr-tests.out" "$test")
        [ -z "$rawr" ] && rawr="build:${rawr_code:-none}"
        if [ "$native" = "$rawr" ]; then
            verdict=pass detail=
        else
            verdict=fail detail=$(first_error "$dir/rawr-tests.err")
        fi
        row test "$name::${test##*::}" "$native" "$rawr" "$verdict" "$detail"
    done
}

# The outcome of `$1`'s tests under `rawr test`, at `$2`: nothing when
# they pass, `tests-build:CODE` when they fail to build, else `tests-fail`.
rawr_tests() {
    local status
    status=$(cd "$package" && run "$2" rawr test "$1")
    [ "$status" = 0 ] && return
    local found
    found=$(code "$2.err")
    if [ -n "$found" ] && ! grep -q '^test result:' "$2.out"; then
        echo "tests-build:$found"
    else
        echo tests-fail
    fi
}

# `$1` is `exercises/<dir>/<name>.rs`, the bin `<name>`, run in its
# directory.
exercise() {
    local source=$package/$1 name=$2 dir=$out/exercises/$2
    local bin=${name##*/} cwd=$package/exercises/${name%%/*}
    mkdir -p "$dir"
    local native rawr verdict detail=
    if ! native_build "$bin" "$dir/native.build"; then
        native="build:$(code "$dir/native.build")"
    else
        native=$(cd "$cwd" && run "$dir/native" "$CARGO_TARGET_DIR/debug/$bin")
        if [ "$native" = 0 ] && grep -q '#\[test\]' "$source"; then
            if [ "$(native_tests "$dir/tests" "$bin")" != 0 ]; then
                if grep -q '^test result:' "$dir/tests.out"; then
                    native=tests-fail
                else
                    native="tests-build:$(code "$dir/tests.err")"
                fi
            fi
        fi
    fi
    rawr=$(cd "$cwd" && run "$dir/rawr" rawr run "$source")
    local rawr_code
    rawr_code=$(code "$dir/rawr.err")
    [ "$rawr" != 0 ] && [ -n "$rawr_code" ] && rawr="build:$rawr_code"
    if [ "$rawr" = 0 ] && grep -q '#\[test\]' "$source"; then
        local tests
        tests=$(rawr_tests "$source" "$dir/rawr-tests")
        [ -n "$tests" ] && rawr=$tests
    fi
    case $native in
        "$rawr") verdict=match ;;
        build:* | tests-build:*)
            case $rawr in
                build:* | tests-build:*) verdict=code-differs ;;
                *) verdict=rawr-accepts ;;
            esac
            ;;
        *) verdict=differs ;;
    esac
    [ "$verdict" = match ] || detail=$(first_error "$dir/rawr.err")
    row exercise "$name" "$native" "$rawr" "$verdict" "$detail"
}

cd "$package" || exit 2
for source in solutions/*/*.rs; do
    [[ " $kinds " == *" solutions "* ]] || break
    name=$(basename "$(dirname "$source")")/$(basename "$source" .rs)
    echo "solution $name" >&2
    solution "$source" "$name"
done
for source in exercises/*/*.rs; do
    [[ " $kinds " == *" exercises "* ]] || break
    name=$(basename "$(dirname "$source")")/$(basename "$source" .rs)
    echo "exercise $name" >&2
    exercise "$source" "$name"
done

awk -F'\t' '
    NR > 1 { total[$1]++; if ($5 == "pass" || $5 == "match") good[$1]++ }
    # A solution matches whole when its main and every one of its tests do.
    $1 == "solution" { whole[$2] = ($5 == "pass") }
    $1 == "test" && $5 != "pass" { name = $2; sub(/::[^:]*$/, "", name); whole[name] = 0 }
    END {
        for (name in whole) if (whole[name]) wholes++
        printf "solutions: %d of %d run and match\n", good["solution"], total["solution"]
        printf "solutions with main and all tests matching: %d of %d\n", wholes, total["solution"]
        printf "tests:     %d of %d run and match\n", good["test"], total["test"]
        printf "exercises: %d of %d fail as rustc fails\n", good["exercise"], total["exercise"]
    }
' "$results"
