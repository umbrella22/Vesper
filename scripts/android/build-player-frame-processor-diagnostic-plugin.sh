#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"

vesper_android_build_runtime_free_plugin player-frame-processor-diagnostic "$@"
