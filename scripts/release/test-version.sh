#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION_SCRIPT="$ROOT_DIR/scripts/release/version.sh"

expect_metadata() {
  local tag="$1"
  local expected_code="$2"
  local output

  output="$($VERSION_SCRIPT metadata-from-tag "$tag" --date 2026-01-01)"
  grep -qx "version=${tag#v}" <<<"$output"
  grep -qx "ios_build=$expected_code" <<<"$output"
  grep -qx "android_version_code=$expected_code" <<<"$output"
}

expect_rejected() {
  if "$@" >/dev/null 2>&1; then
    echo "Expected command to fail: $*" >&2
    exit 1
  fi
}

expect_metadata v0.3.1 301
expect_metadata v0.4.0 400
expect_metadata v1.2.34 10234

expect_rejected "$VERSION_SCRIPT" metadata-from-tag v0.100.0 \
  --android-version-code 99 --ios-build 99 --date 2026-01-01
expect_rejected "$VERSION_SCRIPT" verify 0.4.100 \
  --android-version-code 499 --ios-build 499

"$ROOT_DIR/scripts/vesper" release verify-current

echo "Release version tests passed."
