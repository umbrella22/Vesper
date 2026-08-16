#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
host_directory="$repository_root/examples/flutter-host"
simulator_id="${VESPER_FLUTTER_IOS_SIMULATOR:?VESPER_FLUTTER_IOS_SIMULATOR is required}"
attempt="${VESPER_SUBTITLE_ATTEMPT:-primary}"
runner_temp="${RUNNER_TEMP:?RUNNER_TEMP is required}"

if [[ ! "$attempt" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]; then
  echo "Invalid subtitle integration attempt label: $attempt" >&2
  exit 2
fi

evidence_directory="$runner_temp/flutter-subtitle-evidence"
log_directory="$runner_temp/flutter-subtitle-logs"
bundle_identifier="io.github.umbrella22.vesper.example.flutterhost"
mkdir -p "$evidence_directory" "$log_directory"
rm -f \
  "$evidence_directory/subtitle-positive.json" \
  "$evidence_directory/subtitle-positive.png" \
  "$evidence_directory/subtitle-lifecycle.json" \
  "$evidence_directory/subtitle-lifecycle.png"

# A timed-out Flutter tool can leave the app paused in the Simulator. Ensure
# each attempt starts from a clean application process while preserving the
# already-built artifacts for an incremental retry.
xcrun simctl terminate "$simulator_id" "$bundle_identifier" >/dev/null 2>&1 || true
xcrun simctl bootstatus "$simulator_id" -b

status=0

run_subtitle_integration() {
  local evidence_name="$1"
  local target="$2"
  local -a pipeline_status
  local command_status
  local tee_status

  set +e
  (
    cd "$host_directory"
    VESPER_SUBTITLE_EVIDENCE_DIR="$evidence_directory" \
    VESPER_SUBTITLE_EVIDENCE_NAME="$evidence_name" \
      flutter drive \
        --verbose \
        --no-pub \
        --keep-app-running \
        --driver=test_driver/subtitle_integration_test.dart \
        --target="$target" \
        --device-id "$simulator_id"
  ) 2>&1 | tee "$log_directory/$evidence_name-$attempt.log"
  pipeline_status=("${PIPESTATUS[@]}")
  command_status="${pipeline_status[0]}"
  tee_status="${pipeline_status[1]}"

  # Flutter 3.47 can report a passed drive as failed when its automatic
  # Simulator teardown races an app process that already exited. Own teardown
  # here so an already-stopped or already-uninstalled app remains idempotent.
  xcrun simctl terminate "$simulator_id" "$bundle_identifier" >/dev/null 2>&1 || true
  xcrun simctl uninstall "$simulator_id" "$bundle_identifier" >/dev/null 2>&1 || true
  set -e

  if [[ "$command_status" -ne 0 || "$tee_status" -ne 0 ]]; then
    status=1
  fi
}

run_subtitle_integration subtitle-positive integration_test/subtitle_contract_test.dart
run_subtitle_integration subtitle-lifecycle integration_test/subtitle_lifecycle_test.dart

required_evidence=(
  "$evidence_directory/subtitle-positive.json"
  "$evidence_directory/subtitle-positive.png"
  "$evidence_directory/subtitle-lifecycle.json"
)
for evidence_path in "${required_evidence[@]}"; do
  if [[ ! -s "$evidence_path" ]]; then
    echo "Missing required subtitle evidence: $evidence_path" >&2
    status=1
  fi
done

if ! ruby -rjson -e '
  positive = JSON.parse(File.read(ARGV.fetch(0)))
  snapshot = positive.fetch("snapshot")
  frame = snapshot.fetch("frame")
  abort("Positive subtitle evidence is not visibly attached") unless
    positive["evidenceName"] == "subtitle-positive" &&
      snapshot["text"] == "Subtitle B" && snapshot["visible"] == true &&
      snapshot["hidden"] == false && snapshot["windowAttached"] == true &&
      snapshot.fetch("alpha").to_f > 0 && frame.fetch("width").to_f > 0 &&
      frame.fetch("height").to_f > 0
  abort("Positive subtitle evidence PNG is invalid") unless
    File.binread(ARGV.fetch(1), 8) == "\x89PNG\r\n\x1a\n".b

  lifecycle = JSON.parse(File.read(ARGV.fetch(2)))
  scenarios = lifecycle.fetch("scenarios")
  {
    "timeout" => "subtitle_selection_timeout",
    "sourceChange" => "subtitle_source_changed",
    "supersede" => "subtitle_selection_superseded"
  }.each do |name, code|
    abort("Unexpected #{name} subtitle evidence") unless
      scenarios.fetch(name).fetch("error").fetch("code") == code
  end
' \
  "$evidence_directory/subtitle-positive.json" \
  "$evidence_directory/subtitle-positive.png" \
  "$evidence_directory/subtitle-lifecycle.json"; then
  status=1
fi

exit "$status"
