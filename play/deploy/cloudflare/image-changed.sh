#!/bin/sh
# Whether the ?play image needs a new build:
#   ./image-changed.sh BASE    BASE: the commit of the running image (its tag)
# It exits 0 and says why when the image's inputs differ between BASE and
# HEAD, and exits 1 when the image built from BASE is the image HEAD would
# build. The bot stage (.github/workflows/bot.yml) skips the build then.
#
# play/Dockerfile copies the whole repository, less .dockerignore, so the
# inputs are every tracked file but the ones no build step reads (the
# pathspecs below). A file left off that list counts as an input, so a new
# directory rebuilds the image until it is listed. A release commit bumps the
# workspace's versions only (`version = "x.y.z"` in the manifests, the
# workspace's own packages in Cargo.lock); that alone changes nothing the bot
# does, so those lines are ignored. A dependency's version change in
# Cargo.lock also changes its checksum or source line, so it still counts.
set -eu
base="${1:?usage: image-changed.sh BASE}"
repo="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
cd "$repo"

if ! git cat-file -e "$base^{commit}" 2>/dev/null; then
  echo "the running image's commit $base is not in this history"
  exit 0
fi

set -- . \
  ':(exclude).github' \
  ':(exclude)site' \
  ':(exclude)plan' \
  ':(exclude)play/deploy' \
  ':(exclude)play/README.md' \
  ':(exclude)guides' \
  ':(exclude)editors' \
  ':(exclude)CHANGELOG.md' \
  ':(exclude)web/src' \
  ':(exclude)web/www' \
  ':(exclude)web/build.sh'

files="$(git diff --no-renames --name-only "$base" HEAD -- "$@")"
if [ -z "$files" ]; then
  echo "no input of the image changed since $base"
  exit 1
fi

# Every changed line outside the manifests and lock files counts. In them, a
# line counts unless it is removed and added again with only its version
# strings changed.
other="$(git diff --no-renames -U0 "$base" HEAD -- "$@" | awk '
  /^diff --git / { file = $NF; sub(/^b\//, "", file); next }
  /^(\+\+\+ b\/|--- a\/|\+\+\+ \/dev\/null$|--- \/dev\/null$)/ { next }
  /^[-+]/ {
    if (file !~ /(^|\/)Cargo\.(toml|lock)$/) { print file; next }
    line = substr($0, 2)
    gsub(/version = "[^"]*"/, "version = \"\"", line)
    count[file SUBSEP line] += (substr($0, 1, 1) == "+") ? 1 : -1
  }
  END {
    for (key in count) if (count[key] != 0) { split(key, part, SUBSEP); print part[1] }
  }
' | sort -u)"
if [ -z "$other" ]; then
  echo "only the workspace's versions changed since $base"
  exit 1
fi
echo "the image's inputs changed since $base:"
printf '%s\n' "$other" | sed 's/^/  /' | head -20
exit 0
