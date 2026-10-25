#!/bin/sh
# The checks pre-commit and pre-push share (D507). Sourced by them with
# CHECK_FILES set to the files to check (added files for the layout rules)
# and CHECK_RUST set when Rust code changed.
#
#   RAWR_CLIPPY_TOOLCHAIN  toolchain for Clippy (default: the pinned nightly)
#   RAWR_FMT_TOOLCHAIN     toolchain for rustfmt (default: stable)

clippy_toolchain=${RAWR_CLIPPY_TOOLCHAIN:-nightly-2026-07-11}
fmt_toolchain=${RAWR_FMT_TOOLCHAIN:-stable}
export CARGO_INCREMENTAL=0

# Advisories cargo audit may report without failing. Each needs a reason and
# goes when its reason does. None at present.
audit_ignore=""

check_audit() {
    if ! cargo audit --version >/dev/null 2>&1; then
        echo "cargo-audit is not installed. Installing it: cargo install cargo-audit --locked"
        cargo install cargo-audit --locked || {
            echo "ERROR: could not install cargo-audit."
            return 1
        }
    fi
    echo "Running cargo audit..."
    set --
    for id in $audit_ignore; do
        set -- "$@" --ignore "$id"
    done
    # Its warnings (unmaintained, yanked) print only when it fails.
    if ! report=$(cargo audit "$@" 2>&1); then
        printf '%s\n' "$report"
        echo "ERROR: cargo audit found vulnerabilities. Update the affected dependencies."
        return 1
    fi
}

check_layout() {
    # No new mod.rs: use foo.rs beside foo/. Vendored copies keep their own.
    bad=$(printf '%s\n' "$CHECK_FILES" | grep -E '(^|/)mod\.rs$' | grep -vE '^(toml/edit|watch/notify)/')
    if [ -n "$bad" ]; then
        echo "ERROR: new mod.rs files are not allowed. Use foo.rs beside a foo/ directory."
        printf '%s\n' "$bad" | sed 's/^/  /'
        return 1
    fi
    # Markdown at the root only from the allow list; subdirectories are free.
    allowed=" README.md CONTRIBUTING.md ARCHITECTURE.md CLAUDE.md AGENTS.md CHANGELOG.md "
    bad=""
    for f in $(printf '%s\n' "$CHECK_FILES" | grep -E '^[^/]+\.md$'); do
        case "$allowed" in
            *" $f "*) ;;
            *) bad="$bad  $f\n" ;;
        esac
    done
    if [ -n "$bad" ]; then
        echo "ERROR: only${allowed}are allowed as Markdown at the repository root."
        printf "$bad"
        return 1
    fi
}

check_rust() {
    # With FMT_FILES set (pre-commit), check only those files, so another
    # agent's unformatted work elsewhere in a shared checkout doesn't block
    # this commit. Without it (pre-push), check the whole workspace.
    if [ -n "${FMT_FILES:-}" ]; then
        echo "Running rustfmt --check on the staged files..."
        # shellcheck disable=SC2086
        if ! rustfmt "+$fmt_toolchain" --edition 2024 --check $FMT_FILES >/dev/null 2>&1; then
            echo "ERROR: staged code is not formatted. Run: rustfmt +$fmt_toolchain --edition 2024 <files>"
            # shellcheck disable=SC2086
            rustfmt "+$fmt_toolchain" --edition 2024 --check $FMT_FILES 2>&1 | grep '^Diff in' | sed 's/^/  /'
            return 1
        fi
    else
        echo "Running cargo fmt --check..."
        if ! cargo "+$fmt_toolchain" fmt --all --check >/dev/null 2>&1; then
            echo "ERROR: code is not formatted. Run: cargo +$fmt_toolchain fmt --all"
            cargo "+$fmt_toolchain" fmt --all --check 2>&1 | grep '^Diff in' | sed 's/^/  /'
            return 1
        fi
    fi
    # Clippy compiles the working tree as a whole, so another agent's
    # unfinished code in a shared checkout can still fail it.
    echo "Running cargo clippy $* ..."
    if ! cargo "+$clippy_toolchain" clippy -q "$@" --all-targets -- -D warnings; then
        echo "ERROR: Clippy found issues (D114: fix them; only narrow, named, documented exceptions)."
        return 1
    fi
}

