#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

repo_root="$VESPER_REPO_ROOT"
crate_dir="${repo_root}/crates/core/player-ffi"
config_path="${crate_dir}/cbindgen.toml"
lockfile_path="${repo_root}/Cargo.lock"
header_path="${repo_root}/include/player_ffi.h"
tmp_header="$(mktemp "${TMPDIR:-/tmp}/player_ffi.XXXXXX.h")"

cleanup() {
  rm -f "${tmp_header}"
}
trap cleanup EXIT

if ! command -v cbindgen >/dev/null 2>&1; then
  echo "cbindgen is required to verify include/player_ffi.h." >&2
  echo "Install it with: cargo install cbindgen" >&2
  exit 1
fi

cbindgen "${crate_dir}" \
  --config "${config_path}" \
  --crate player-ffi \
  --lang c \
  --lockfile "${lockfile_path}" \
  --only-target-dependencies \
  --output "${tmp_header}"

if ! diff -u "${header_path}" "${tmp_header}"; then
  echo "" >&2
  echo "include/player_ffi.h is out of date. Run scripts/vesper ffi generate." >&2
  exit 1
fi

echo "include/player_ffi.h is up to date."
