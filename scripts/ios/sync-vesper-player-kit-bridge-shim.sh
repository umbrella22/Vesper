#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

repo_root="$VESPER_REPO_ROOT"
manifest="$repo_root/scripts/ios/bridge-shim/manifest.json"
shim_dir="$repo_root/lib/ios/VesperPlayerKit/Sources/VesperPlayerKitBridgeShim"

vesper_require_command cargo "cargo is required to generate the VesperPlayerKit bridge shim."

(
  cd "$repo_root"
  cargo run --quiet -p player-ios-bridge-shim-generator -- \
  generate \
  --manifest "$manifest" \
  --out-dir "$shim_dir"
)

echo "VesperPlayerKit bridge shim synchronized."
