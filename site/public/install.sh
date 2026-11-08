#!/bin/sh
# Installs rawr's prebuilt binaries: rawr, cargo-rawr, rawr-import and
# rawr-import-driver.
#
#   curl -LsSf https://rawr.airstrike.fyi/install.sh | sh
#   curl -LsSf https://rawr.airstrike.fyi/install.sh | sh -s -- --version 0.3.115
#
# They go to $RAWR_INSTALL_DIR, else $CARGO_HOME/bin, else ~/.cargo/bin.
# Then `rawr-import cache warm` installs the Rust nightly it reads crate
# APIs with through rustup and compiles rawr's runtime, so the first run
# with dependencies does not wait for them.
set -eu

repository="https://github.com/airstrike/rawr"
# A mirror of the releases, laid out as GitHub lays them out.
releases="${RAWR_RELEASES:-$repository/releases}"
version="${RAWR_VERSION:-}"

while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="${2:?--version needs a version such as 0.3.115}"; shift 2 ;;
    --version=*) version="${1#--version=}"; shift ;;
    -h | --help)
      echo "usage: install.sh [--version x.y.z]; RAWR_INSTALL_DIR picks the directory"
      exit 0 ;;
    *) echo "install.sh: unknown argument \`$1\`" >&2; exit 1 ;;
  esac
done

say() { printf 'rawr: %s\n' "$*" >&2; }
fail() { say "$*"; exit 1; }

os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin)
    # A shell under Rosetta reports x86_64 on an Apple Silicon Mac.
    if [ "$arch" = x86_64 ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = 1 ]; then
      arch=arm64
    fi
    case "$arch" in
      arm64 | aarch64) target=aarch64-apple-darwin ;;
      x86_64) target=x86_64-apple-darwin ;;
      *) fail "no prebuilt rawr for macOS on $arch" ;;
    esac ;;
  Linux)
    case "$arch" in
      x86_64 | amd64) target=x86_64-unknown-linux-gnu ;;
      aarch64 | arm64) target=aarch64-unknown-linux-gnu ;;
      *) fail "no prebuilt rawr for Linux on $arch" ;;
    esac ;;
  MINGW* | MSYS* | CYGWIN*)
    fail "on Windows, run in PowerShell: irm https://rawr.airstrike.fyi/install.ps1 | iex" ;;
  *) fail "no prebuilt rawr for $os; build it from source with: cargo install --locked --git $repository rawr-cli" ;;
esac

archive="rawr-$target.tar.gz"
if [ -n "$version" ]; then
  url="$releases/download/v${version#v}/$archive"
else
  url="$releases/latest/download/$archive"
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL --retry 2 -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q -O "$2" "$1"; }
else
  fail "install needs curl or wget"
fi

directory="${RAWR_INSTALL_DIR:-${CARGO_HOME:-$HOME/.cargo}/bin}"
staging="$(mktemp -d "${TMPDIR:-/tmp}/rawr-install.XXXXXX")"
trap 'rm -rf "$staging"' EXIT INT TERM

say "downloading $url"
fetch "$url" "$staging/$archive" || fail "could not download $url"
fetch "$url.sha256" "$staging/$archive.sha256" || fail "could not download $url.sha256"

expected="$(cut -d ' ' -f 1 < "$staging/$archive.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$staging/$archive" | cut -d ' ' -f 1)"
else
  actual="$(shasum -a 256 "$staging/$archive" | cut -d ' ' -f 1)"
fi
[ "$expected" = "$actual" ] || fail "$archive does not match its published SHA-256"

mkdir -p "$staging/files" "$directory"
tar -xzf "$staging/$archive" -C "$staging/files"
for binary in rawr cargo-rawr rawr-import rawr-import-driver; do
  [ -f "$staging/files/$binary" ] || continue
  # A rename replaces a running binary's file without touching its process.
  cp "$staging/files/$binary" "$directory/.$binary.new"
  chmod 755 "$directory/.$binary.new"
  mv -f "$directory/.$binary.new" "$directory/$binary"
done
say "installed $("$directory/rawr" --version 2>/dev/null || echo rawr) in $directory"

case ":$PATH:" in
  *":$directory:"*) ;;
  *) say "add $directory to PATH, for example: export PATH=\"$directory:\$PATH\"" ;;
esac
if ! command -v rustup >/dev/null 2>&1; then
  say "programs with dependencies also need rustup, from https://rustup.rs"
elif [ -x "$directory/rawr-import" ]; then
  # What a first run with dependencies would build once per toolchain: the
  # nightly that reads crate APIs, its driver and rawr's runtime.
  "$directory/rawr-import" cache warm || say "rawr cache warm failed; the first run builds what it needs"
fi
say "start with: rawr new hello && cd hello && rawr run"
