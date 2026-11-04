#!/usr/bin/env bash
# Run every rustlings solution and exercise under `rawr run` and under rustc,
# and compare them.
#
#   scripts/rustlings.sh [RUSTLINGS_DIR] [OUT_DIR]
#
# RUSTLINGS_DIR is a checkout of github.com/rust-lang/rustlings (default
# target/rustlings). OUT_DIR (default target/rustlings-out) gets the scratch
# copies, every output and `results.tsv`, one row per run:
#
#   kind  file  rustc  rawr  verdict  detail
#
# - `solution` rows run `fn main` both ways. They match when stdout and the
#   exit status are equal.
# - `test` rows run one `#[test]` of a solution. rawr has no test runner
#   (`rawr test` is not a command), so each test runs as a program: a copy
#   with `#[cfg(test)]`, `#[test]` and `#[should_panic]` removed, the test
#   made `pub`, and a `main` that calls it. The same copy runs natively, so
#   the two exit statuses (0 passes, 101 panics) are compared.
# - `exercise` rows run an unsolved exercise both ways. rustc's outcome is
#   its first error code, else the program's exit status, else the outcome
#   of its tests. rawr's is its first error code or its exit status. They
#   match when both fail to compile with the same code, or both run alike.
#
# Every copy starts with `// rawr`. `rawr` is the one on PATH. RAWR_BIN names
# a development build's directory (with `rawr-import` and
# `rawr-import-driver`) to put first on PATH. RUSTC_TOOLCHAIN (default
# stable) picks rustc's toolchain; RAWR_TIMEOUT (default 120) bounds each run
# in seconds.
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
rustlings=${1:-$root/target/rustlings}
out=${2:-$root/target/rustlings-out}
toolchain=${RUSTC_TOOLCHAIN:-stable}
limit=${RAWR_TIMEOUT:-120}
rustc=(rustc "+$toolchain" --edition 2024 -A warnings)
[ -n "${RAWR_BIN:-}" ] && PATH=$RAWR_BIN:$PATH

[ -d "$rustlings/solutions" ] || {
    echo "no rustlings checkout at $rustlings" >&2
    exit 2
}
rm -rf "$out"
mkdir -p "$out"
results=$out/results.tsv
printf 'kind\tfile\trustc\trawr\tverdict\tdetail\n' >"$results"

row() { printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$@" >>"$results"; }

# The first error code in a file (`E0308`, or rawr's own such as
# `RAWR-RESOLVE-…`), else `error` when it has an error line, else nothing.
code() {
    local found
    found=$(grep -o -m1 'error\[[^]]*\]' "$1" | head -1)
    if [ -n "$found" ]; then
        found=${found#error[}
        echo "${found%]}"
    elif grep -q -m1 '^error' "$1"; then
        echo error
    fi
}

# The first error line (else the first stderr line), on one line.
first_error() {
    { grep -m1 '^error' "$1" || head -1 "$1"; } | cut -c1-200 | tr '\t' ' '
}

# Compile `$1` natively to `$2` (extra rustc arguments follow).
native_build() {
    local source=$1 binary=$2
    shift 2
    "${rustc[@]}" "$@" -o "$binary" "$source" 2>"$binary.build"
}

# Run a command with the time limit; stdout to `$1.out`, stderr to
# `$1.err`. Prints the exit status.
run() {
    local prefix=$1
    shift
    timeout "$limit" "$@" >"$prefix.out" 2>"$prefix.err" </dev/null
    echo $?
}

# A copy of `$1` at `$2` that starts with `// rawr`.
marked() {
    mkdir -p "$(dirname "$2")"
    { echo '// rawr'; cat "$1"; } >"$2"
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

# `$1` with its test `$3` (as `module::name`) called from `main`, at `$2`.
test_program() {
    perl -pe '
        s/#\[cfg\(test\)\]//;
        s/#\[should_panic[^\]]*\]//;
        if (s/#\[test\]//) { $test = 1 }
        if ($test && s/^(\s*)fn\s/$1pub fn /) { $test = 0 }
        s/^fn main\(\)/#[allow(dead_code)]\nfn rustlings_main()/;
    ' "$1" >"$2"
    printf '\nfn main() {\n    %s();\n}\n' "$3" >>"$2"
}

solution() {
    local source=$1 name=$2 dir=$out/solutions/$2
    mkdir -p "$dir"
    marked "$source" "$dir/main.rs"
    local native rawr verdict detail=
    if native_build "$source" "$dir/native"; then
        native=$(run "$dir/native" "$dir/native")
    else
        native="build:$(code "$dir/native.build")"
    fi
    rawr=$(run "$dir/rawr" rawr run "$dir/main.rs")
    if [ "$native" = "$rawr" ] && cmp -s "$dir/native.out" "$dir/rawr.out"; then
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

    local test
    for test in $(tests_of "$source"); do
        local case=$dir/test-${test##*::}
        test_program "$dir/main.rs" "$case.rs" "$test"
        if native_build "$case.rs" "$case.native"; then
            native=$(run "$case.native" "$case.native")
        else
            native="build:$(code "$case.native.build")"
        fi
        rawr=$(run "$case.rawr" rawr run "$case.rs")
        if [ "$native" = "$rawr" ]; then
            verdict=pass detail=
        else
            verdict=fail detail=$(first_error "$case.rawr.err")
        fi
        row test "$name::${test##*::}" "$native" "$rawr" "$verdict" "$detail"
    done
}

exercise() {
    local source=$1 name=$2 dir=$out/exercises/$2
    mkdir -p "$dir"
    marked "$source" "$dir/main.rs"
    local native rawr verdict detail=
    if ! native_build "$source" "$dir/native"; then
        native="build:$(code "$dir/native.build")"
    else
        native=$(run "$dir/native" "$dir/native")
        if [ "$native" = 0 ] && grep -q '#\[test\]' "$source"; then
            if native_build "$source" "$dir/tests" --test; then
                [ "$(run "$dir/tests" "$dir/tests")" = 0 ] || native=tests-fail
            else
                native="build:$(code "$dir/tests.build")"
            fi
        fi
    fi
    rawr=$(run "$dir/rawr" rawr run "$dir/main.rs")
    local rawr_code
    rawr_code=$(code "$dir/rawr.err")
    [ "$rawr" != 0 ] && [ -n "$rawr_code" ] && rawr="build:$rawr_code"
    case $native in
        "$rawr") verdict=match ;;
        build:*)
            case $rawr in
                build:*) verdict=code-differs ;;
                *) verdict=rawr-accepts ;;
            esac
            ;;
        tests-fail) verdict=tests-gap ;;
        *) verdict=differs ;;
    esac
    [ "$verdict" = match ] || detail=$(first_error "$dir/rawr.err")
    row exercise "$name" "$native" "$rawr" "$verdict" "$detail"
}

for source in "$rustlings"/solutions/*/*.rs; do
    name=$(basename "$(dirname "$source")")/$(basename "$source" .rs)
    echo "solution $name" >&2
    solution "$source" "$name"
done
for source in "$rustlings"/exercises/*/*.rs; do
    name=$(basename "$(dirname "$source")")/$(basename "$source" .rs)
    echo "exercise $name" >&2
    exercise "$source" "$name"
done

awk -F'\t' '
    NR > 1 { total[$1]++; if ($5 == "pass" || $5 == "match") good[$1]++ }
    END {
        printf "solutions: %d of %d run and match\n", good["solution"], total["solution"]
        printf "tests:     %d of %d run and match\n", good["test"], total["test"]
        printf "exercises: %d of %d fail as rustc fails\n", good["exercise"], total["exercise"]
    }
' "$results"
