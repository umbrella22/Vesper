#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

usage() {
  echo "Usage: $0 <generate|sync|verify>" >&2
}

mode="${1:-}"
case "$mode" in
  generate|sync|verify)
    shift
    ;;
  *)
    usage
    exit 1
    ;;
esac

repo_root="$VESPER_REPO_ROOT"
crate_dir="$repo_root/crates/ffi/player-ffi"
config_path="$crate_dir/cbindgen.toml"
lockfile_path="$repo_root/Cargo.lock"
header_path="$repo_root/include/player_ffi.h"

if ! command -v cbindgen >/dev/null 2>&1; then
  echo "cbindgen is required to $mode include/player_ffi.h." >&2
  echo "Install it with: cargo install cbindgen" >&2
  exit 1
fi

generate_header() {
  local output_path="$1"

  cbindgen "$crate_dir" \
    --config "$config_path" \
    --crate player-ffi \
    --lang c \
    --lockfile "$lockfile_path" \
    --only-target-dependencies \
    --output "$output_path"
}

if [[ "$mode" == "generate" ]]; then
  generate_header "$header_path"
  echo "Generated $header_path"
  exit 0
fi

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/player_ffi.XXXXXX")"
tmp_header="$tmp_dir/player_ffi.h"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

generate_header "$tmp_header"
if [[ "$mode" == "sync" ]]; then
  if [[ -f "$header_path" ]] && cmp -s "$header_path" "$tmp_header"; then
    echo "include/player_ffi.h is up to date."
    exit 0
  fi

  cp "$tmp_header" "$header_path"
  echo "Synced $header_path"
  exit 0
fi

if ! diff -u "$header_path" "$tmp_header"; then
  echo "" >&2
  echo "include/player_ffi.h is out of date. Run scripts/vesper ffi sync." >&2
  exit 1
fi

echo "include/player_ffi.h is up to date."
