#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

repo_root="$VESPER_REPO_ROOT"
shim_dir="$repo_root/lib/ios/VesperPlayerKit/Sources/VesperPlayerKitBridgeShim"
shim_c="$shim_dir/VesperPlayerKitBridgeShim.c"

vesper_require_command clang "clang is required to verify the VesperPlayerKit bridge shim."

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

echo "VesperPlayerKit bridge shim is valid."
