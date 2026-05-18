#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

repo_root="$VESPER_REPO_ROOT"
shim_dir="$repo_root/lib/ios/VesperPlayerKit/Sources/VesperPlayerKitBridgeShim"
shim_c="$shim_dir/VesperPlayerKitBridgeShim.c"
shim_h="$shim_dir/include/VesperPlayerKitBridgeShim.h"
rust_ffi_ios="$repo_root/crates/ffi/player-ffi-ios/src/lib.rs"
manifest="$repo_root/scripts/ios/bridge-shim/manifest.json"

vesper_require_command clang "clang is required to verify the VesperPlayerKit bridge shim."
vesper_require_command cargo "cargo is required to generate the VesperPlayerKit bridge shim."
vesper_require_command diff "diff is required to compare the generated VesperPlayerKit bridge shim."
vesper_require_command perl "perl is required to verify VesperPlayerKit bridge symbols."

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/vesper-ios-bridge.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT

generated_shim_dir="$tmp_dir/generated"
(
  cd "$repo_root"
  cargo run --quiet -p player-ios-bridge-shim-generator -- \
  generate \
  --manifest "$manifest" \
  --out-dir "$generated_shim_dir"
)

if ! diff -u "$generated_shim_dir/include/VesperPlayerKitBridgeShim.h" "$shim_h"; then
  echo "" >&2
  echo "VesperPlayerKitBridgeShim.h is out of sync with the Rust generator." >&2
  echo "Run: ./scripts/vesper ios sync-bridge-shim" >&2
  exit 1
fi

if ! diff -u "$generated_shim_dir/VesperPlayerKitBridgeShim.c" "$shim_c"; then
  echo "" >&2
  echo "VesperPlayerKitBridgeShim.c is out of sync with the Rust generator." >&2
  echo "Run: ./scripts/vesper ios sync-bridge-shim" >&2
  exit 1
fi

clang \
  -fsyntax-only \
  -I "$shim_dir" \
  "$shim_c"

forbidden_cast_pattern='\([[:space:]]*(const[[:space:]]+)?(PlayerFfiDownload|VesperRuntimeDownload)[A-Za-z0-9_]*[[:space:]]*\*[[:space:]]*\)'
if grep -En "$forbidden_cast_pattern" "$shim_c"; then
  echo "" >&2
  echo "Download bridge DTO pointer casts are not allowed in VesperPlayerKitBridgeShim.c." >&2
  echo "Use explicit input/output conversion helpers instead." >&2
  exit 1
fi

extern_symbols="$tmp_dir/player_ffi_externs.txt"
rust_symbols="$tmp_dir/player_ffi_rust_exports.txt"
header_symbols="$tmp_dir/bridge_header_symbols.txt"
c_symbols="$tmp_dir/bridge_c_symbols.txt"

perl -0777 -ne '
  while (/extern\s+[A-Za-z_][A-Za-z0-9_\s\*]*?\s+(player_ffi_[A-Za-z0-9_]+)\s*\(/g) {
    print "$1\n";
  }
' "$shim_c" | sort -u > "$extern_symbols"

perl -ne '
  if (/pub\s+unsafe\s+extern\s+"C"\s+fn\s+(player_ffi_[A-Za-z0-9_]+)/) {
    print "$1\n";
  }
' "$rust_ffi_ios" | sort -u > "$rust_symbols"

missing_rust_exports="$(comm -23 "$extern_symbols" "$rust_symbols" || true)"
if [[ -n "$missing_rust_exports" ]]; then
  echo "VesperPlayerKitBridgeShim.c references Rust FFI symbols that are not exported by player-ffi-ios:" >&2
  echo "$missing_rust_exports" >&2
  exit 1
fi

perl -0777 -ne '
  while (/(?:^|\n)\s*(?:bool|void|uint64_t|char\s*\*)\s+((?:vesper_runtime|vesper_dash)_[A-Za-z0-9_]+)\s*\(/g) {
    print "$1\n";
  }
' "$shim_h" | sort -u > "$header_symbols"

perl -0777 -ne '
  while (/(?:^|\n)(?:bool|void|uint64_t|char\s*\*)\s+((?:vesper_runtime|vesper_dash)_[A-Za-z0-9_]+)\s*\(/g) {
    print "$1\n";
  }
' "$shim_c" | sort -u > "$c_symbols"

missing_c_wrappers="$(comm -23 "$header_symbols" "$c_symbols" || true)"
if [[ -n "$missing_c_wrappers" ]]; then
  echo "VesperPlayerKitBridgeShim.h declares bridge symbols without C implementations:" >&2
  echo "$missing_c_wrappers" >&2
  exit 1
fi

archive_paths=()
if [[ -n "${VESPER_IOS_FFI_ARCHIVE:-}" ]]; then
  if [[ ! -f "$VESPER_IOS_FFI_ARCHIVE" ]]; then
    echo "VESPER_IOS_FFI_ARCHIVE points to a missing file: $VESPER_IOS_FFI_ARCHIVE" >&2
    exit 1
  fi
  archive_paths+=("$VESPER_IOS_FFI_ARCHIVE")
else
  while IFS= read -r archive; do
    archive_paths+=("$archive")
  done < <(
    find "$repo_root/lib/ios/VesperPlayerKit/Artifacts/rust-player-ffi" \
      -name libplayer_ffi_ios.a \
      -type f \
      -print 2>/dev/null | sort
  )
fi

if ((${#archive_paths[@]} > 0)); then
  vesper_require_command nm "nm is required to verify Rust FFI archive symbols."
  vesper_require_command strings "strings is required to verify Rust FFI archive symbols."
  for archive in "${archive_paths[@]}"; do
    archive_symbols="$tmp_dir/archive_symbols_$(basename "$(dirname "$archive")").txt"
    archive_nm="$tmp_dir/archive_nm_$(basename "$(dirname "$archive")").txt"
    archive_nm_errors="$tmp_dir/archive_nm_$(basename "$(dirname "$archive")").err"
    if ! nm -gU "$archive" > "$archive_nm" 2> "$archive_nm_errors"; then
      if [[ ! -s "$archive_nm" ]]; then
        echo "nm could not read Rust FFI archive symbols: $archive" >&2
        cat "$archive_nm_errors" >&2
        exit 1
      fi
      echo "nm reported non-fatal object metadata warnings while reading: $archive" >&2
    fi
    {
      awk '{ print $NF }' "$archive_nm" | sed 's/^_//'
      strings -a "$archive" | perl -ne '
        while (/_?(player_ffi_[A-Za-z0-9_]+)/g) {
          print "$1\n";
        }
      '
    } \
      | sort -u > "$archive_symbols"
    missing_archive_symbols="$(comm -23 "$extern_symbols" "$archive_symbols" || true)"
    if [[ -n "$missing_archive_symbols" ]]; then
      echo "Rust FFI archive is missing symbols required by VesperPlayerKitBridgeShim.c: $archive" >&2
      echo "$missing_archive_symbols" >&2
      exit 1
    fi
  done
elif [[ -n "${VESPER_IOS_FFI_ARCHIVE:-}" ]]; then
  echo "No Rust FFI archive was available for symbol verification." >&2
  exit 1
else
  echo "No Rust FFI archive found; source-level bridge symbol verification only."
fi

echo "VesperPlayerKit bridge shim is valid."
