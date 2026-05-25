#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

require_file() {
  local path="$1"
  if [[ ! -f "$REPO_ROOT/$path" ]]; then
    echo "Missing contract source: $path" >&2
    exit 1
  fi
}

require_text() {
  local path="$1"
  local needle="$2"
  if ! grep -Fq "$needle" "$REPO_ROOT/$path"; then
    echo "Contract drift: expected '$needle' in $path" >&2
    exit 1
  fi
}

require_text_in_tree() {
  local path="$1"
  local needle="$2"
  if ! grep -FRq "$needle" "$REPO_ROOT/$path"; then
    echo "Contract drift: expected '$needle' in $path" >&2
    exit 1
  fi
}

require_flutter_models_text() {
  local needle="$1"
  if ! grep -Fq "$needle" "$REPO_ROOT/lib/flutter/vesper_player_platform_interface/lib/src/models.dart" \
    && ! grep -FRq "$needle" "$REPO_ROOT/lib/flutter/vesper_player_platform_interface/lib/src/models"; then
    echo "Contract drift: expected '$needle' in Flutter platform interface models" >&2
    exit 1
  fi
}

for fixture in \
  fixtures/contracts/player_error.json \
  fixtures/contracts/plugin_diagnostics.json \
  fixtures/contracts/download_task_snapshot.json \
  fixtures/contracts/system_playback_configuration.json; do
  require_file "$fixture"
done

PATH="$(printf '%s' "$PATH" | tr ':' '\n' | grep -Ev '^/opt/homebrew/(bin|sbin)$' | paste -sd ':' -)" \
  ruby "$SCRIPT_DIR/contract/verify-dto-drift.rb"

require_flutter_models_text "unsupported"
require_text lib/android/vesper-player-kit/src/main/java/io/github/ikaros/vesper/player/android/VesperPlayerError.kt "unsupported"
require_text lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/PlayerBridge.swift "case unsupported"
require_text_in_tree crates/ffi/player-ffi/src/c_api "PlayerFfiErrorCode::Unsupported"

require_flutter_models_text "decoderSupported"
require_flutter_models_text "frameProcessorSupported"
require_flutter_models_text "sourceNormalizerSupported"
require_flutter_models_text "VesperPluginParticipation"
require_text crates/core/player-runtime/src/lib.rs "DecoderSupported"
require_text crates/core/player-runtime/src/lib.rs "FrameProcessorSupported"
require_text crates/core/player-runtime/src/lib.rs "SourceNormalizerSupported"
require_text crates/core/player-runtime/src/lib.rs "PlayerPluginParticipation"
require_text_in_tree crates/ffi/player-ffi/src/c_api "DecoderSupported"
require_text_in_tree crates/ffi/player-ffi/src/c_api "FrameProcessorSupported"
require_text_in_tree crates/ffi/player-ffi/src/c_api "SourceNormalizerSupported"
require_text_in_tree crates/ffi/player-ffi/src/c_api "PlayerFfiPluginParticipation"
require_text fixtures/contracts/plugin_diagnostics.json '"participation": "participated"'
require_text fixtures/contracts/plugin_diagnostics.json '"participation": "available"'
require_text fixtures/contracts/plugin_diagnostics.json '"participation": "bypassed"'

require_text lib/flutter/vesper_player_platform_interface/lib/src/download_models.dart "dashSegments"
require_text lib/android/vesper-player-kit/src/main/java/io/github/ikaros/vesper/player/android/VesperDownloadManager.kt "DashSegments"
require_text lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/VesperDownloadManager.swift "case dashSegments"

require_flutter_models_text "continueAudio"
require_text lib/android/vesper-player-kit/src/main/java/io/github/ikaros/vesper/player/android/PlayerBridge.kt "ContinueAudio"
require_text lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/PlayerBridge.swift "case continueAudio"

echo "Contract fixtures match the checked Rust, Android, iOS, and Flutter wire names."
