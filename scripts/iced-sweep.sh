#!/usr/bin/env bash
# Measures which of iced's examples run under the installed rawr.
#
#   scripts/iced-sweep.sh            copy the examples, then sweep them all
#   RESUME=1 scripts/iced-sweep.sh   continue a sweep, skipping examples done
#   scripts/iced-sweep.sh todos lazy sweep only these
#
# For each example it runs `rawr check`, then `rawr run` with RAWR_MCP_EVENTS
# on a FIFO. An example runs when the run reports `running` and no `failed`
# or `ended` follows within 6 s. It writes target/scratch/iced-sweep/status
# (one line per example: check exit, state) and each example's output beside
# it. The examples share one Cargo target; each example's runner state is
# removed after its run, since no other example reuses it.
set -u
repo="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"
out=$repo/target/scratch/iced-examples
logs=$repo/target/scratch/iced-sweep
if [ -z "${RESUME:-}" ]; then
    rm -rf "$logs"
    mkdir -p "$logs"
    bash "$repo/scripts/iced-examples.sh" "$@" > "$logs/setup.out" 2>&1 || {
        echo "setup failed; see $logs/setup.out" >&2
        exit 1
    }
fi
touch "$logs/status"
export CARGO_TARGET_DIR=$out/_target CARGO_INCREMENTAL=0
for dir in "$out"/*/; do
    ex=$(basename "$dir")
    [ "$ex" = _target ] && continue
    if [ "$#" -gt 0 ] && [[ " $* " != *" $ex "* ]]; then
        continue
    fi
    grep -q "^$ex	" "$logs/status" && continue
    cd "$dir" || continue
    timeout 1200 rawr check > "$logs/$ex.check" 2>&1
    check=$?
    fifo=$logs/$ex.fifo
    rm -f "$fifo"
    mkfifo "$fifo"
    exec 3<>"$fifo"
    set -m
    RAWR_MCP_EVENTS=$fifo timeout 1200 rawr run > "$logs/$ex.run" 2>&1 &
    pid=$!
    set +m
    state=none
    waited=0
    while [ "$waited" -lt 1200 ]; do
        if IFS= read -r -t 1 line <&3; then
            echo "$line" >> "$logs/$ex.events"
            case "$line" in
                *'"running"'*) state=running; break ;;
                *'"failed"'*|*'"ended"'*) state=failed; break ;;
            esac
        elif ! kill -0 "$pid" 2> /dev/null; then
            state=exited
            break
        else
            waited=$((waited + 1))
        fi
    done
    if [ "$state" = running ]; then
        while IFS= read -r -t 6 line <&3; do
            echo "$line" >> "$logs/$ex.events"
            case "$line" in *'"failed"'*|*'"ended"'*) state=failed; break ;; esac
        done
    fi
    kill -TERM -- "-$pid" 2> /dev/null
    wait "$pid" 2> /dev/null
    exec 3>&-
    rm -f "$fifo"
    rm -rf "$out/_target/rawr/runner" "$out/_target/rawr/store" "$out/_target/rawr/store.sessions"
    printf '%s\tcheck=%s\t%s\n' "$ex" "$check" "$state" >> "$logs/status"
done
total=$(wc -l < "$logs/status" | tr -d ' ')
running=$(grep -c $'\trunning$' "$logs/status")
echo "running $running of $total"
