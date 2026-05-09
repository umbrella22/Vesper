#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

repo_root="$VESPER_REPO_ROOT"
crate_dir="${repo_root}/crates/ffi/player-ffi"
config_path="${crate_dir}/cbindgen.toml"
lockfile_path="${repo_root}/Cargo.lock"
header_path="${repo_root}/include/player_ffi.h"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/player_ffi.XXXXXX")"
tmp_header="${tmp_dir}/player_ffi.h"

cleanup() {
  rm -rf "${tmp_dir}"
}
trap cleanup EXIT

if ! command -v cbindgen >/dev/null 2>&1; then
  echo "cbindgen is required to sync include/player_ffi.h." >&2
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

if [[ -f "${header_path}" ]] && cmp -s "${header_path}" "${tmp_header}"; then
  echo "include/player_ffi.h is up to date."
  exit 0
fi

cp "${tmp_header}" "${header_path}"
echo "Synced ${header_path}"
