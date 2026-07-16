#!/usr/bin/env bash
#
# Fetch a pinned mp4-fixtures release and extract it for the real-file tests.
#
# The real-file integration tests (e.g. tests/fragmented.rs) read their inputs
# from $MP4_FIXTURES_DIR. This script downloads a *pinned* release of
# video-commander/mp4-fixtures so those inputs stay byte-identical until the pin
# is bumped deliberately — never the local working copy, which drifts on every
# regeneration.
#
# Usage:
#   scripts/fetch-fixtures.sh                       # default pin (see FIXTURES_TAG)
#   FIXTURES_TAG=fixtures-v3 scripts/fetch-fixtures.sh
#   MP4_FIXTURES_DIR=/tmp/fx scripts/fetch-fixtures.sh
#
# Then run the tests with the same dir exported:
#   export MP4_FIXTURES_DIR="$PWD/target/fixtures"
#   cargo test
#
set -euo pipefail

REPO="video-commander/mp4-fixtures"
# Pinned tag. Bump deliberately and refresh the ground-truth constants in the
# tests when you do (the fixture bytes change between tags).
TAG="${FIXTURES_TAG:-fixtures-v2}"
ASSET="mp4-fixtures-${TAG}.tar.zst"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
dest="${MP4_FIXTURES_DIR:-$repo_root/target/fixtures}"
stamp="$dest/.fixtures-tag"

# Idempotent: skip the download when the pinned tag is already extracted.
if [[ -f "$stamp" && "$(cat "$stamp")" == "$TAG" ]]; then
  echo "fixtures already present for $TAG at $dest"
  exit 0
fi

if command -v shasum >/dev/null 2>&1; then
  sha() { shasum -a 256 "$@"; }
elif command -v sha256sum >/dev/null 2>&1; then
  sha() { sha256sum "$@"; }
else
  echo "error: need shasum or sha256sum on PATH" >&2
  exit 1
fi

command -v zstd >/dev/null 2>&1 || { echo "error: need zstd on PATH" >&2; exit 1; }

base="https://github.com/$REPO/releases/download/$TAG"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "downloading $ASSET ($TAG) ..."
curl -fSL --retry 3 -o "$tmp/$ASSET" "$base/$ASSET"
curl -fSL --retry 3 -o "$tmp/checksums.txt" "$base/checksums.txt"

echo "verifying checksum ..."
( cd "$tmp" && grep -F "  $ASSET" checksums.txt | sha -c - )

echo "extracting to $dest ..."
mkdir -p "$dest"
# Clear any stale contents but keep the dir itself (it may be cargo-cached).
find "$dest" -mindepth 1 -delete
zstd -dc "$tmp/$ASSET" | tar -x -C "$dest"

echo "$TAG" > "$stamp"
echo "fixtures ready at $dest ($TAG)"
