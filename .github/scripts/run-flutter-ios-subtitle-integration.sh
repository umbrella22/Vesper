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
max_attempts=3
mkdir -p "$evidence_directory" "$log_directory"
rm -f \
  "$evidence_directory/subtitle-positive.json" \
  "$evidence_directory/subtitle-positive.png" \
  "$evidence_directory/subtitle-lifecycle.json" \
  "$evidence_directory/subtitle-lifecycle.png"

cleanup_application() {
  xcrun simctl terminate "$simulator_id" "$bundle_identifier" >/dev/null 2>&1 || true
  xcrun simctl uninstall "$simulator_id" "$bundle_identifier" >/dev/null 2>&1 || true
}

# shellcheck disable=SC2329
on_signal() {
  cleanup_application
  exit 143
}
trap on_signal INT TERM
trap cleanup_application EXIT

cleanup_stale_flutter_processes() {
  # A step-level timeout can leave xcodebuild and Flutter children alive. They
  # must not share the next attempt's Flutter.framework output directory.
  pkill -TERM -f "$host_directory" >/dev/null 2>&1 || true
  sleep 2
  pkill -KILL -f "$host_directory" >/dev/null 2>&1 || true
}

reset_debug_build_outputs() {
  rm -rf \
    "$host_directory/build/ios/Debug-iphonesimulator" \
    "$host_directory/build/ios/iphonesimulator"
}

cleanup_application
cleanup_stale_flutter_processes
xcrun simctl bootstatus "$simulator_id" -b

status=0

run_subtitle_integration() {
  local evidence_name="$1"
  local target="$2"
  local attempt_number
  local -a pipeline_status
  local command_status
  local tee_status
  local evidence_ready=0

  for attempt_number in $(seq 1 "$max_attempts"); do
    evidence_ready=0
    rm -f \
      "$evidence_directory/$evidence_name.json" \
      "$evidence_directory/$evidence_name.png"
    cleanup_application
    if [[ "$attempt_number" -gt 1 ]]; then
      cleanup_stale_flutter_processes
      reset_debug_build_outputs
      xcrun simctl bootstatus "$simulator_id" -b
    fi

    set +e
    (
      cd "$host_directory"
      VESPER_SUBTITLE_EVIDENCE_DIR="$evidence_directory" \
      VESPER_SUBTITLE_EVIDENCE_NAME="$evidence_name" \
        flutter drive \
          --verbose \
          --no-pub \
          --no-dds \
          --keep-app-running \
          --driver=test_driver/subtitle_integration_test.dart \
          --target="$target" \
          --device-id "$simulator_id"
    ) 2>&1 | tee "$log_directory/$evidence_name-$attempt-$attempt_number.log"
    pipeline_status=("${PIPESTATUS[@]}")
    command_status="${pipeline_status[0]}"
    tee_status="${pipeline_status[1]}"
    set -e

    cleanup_application
    case "$evidence_name" in
      subtitle-positive)
        [[ -s "$evidence_directory/subtitle-positive.json" && \
          -s "$evidence_directory/subtitle-positive.png" ]] && evidence_ready=1
        ;;
      subtitle-lifecycle)
        [[ -s "$evidence_directory/subtitle-lifecycle.json" ]] && evidence_ready=1
        ;;
      *)
        echo "Unknown subtitle evidence name: $evidence_name" >&2
        status=1
        return 1
        ;;
    esac

    if [[ "$command_status" -eq 0 && "$tee_status" -eq 0 && "$evidence_ready" -eq 1 ]]; then
      echo "Subtitle integration $evidence_name passed on attempt $attempt_number."
      return
    fi

    echo "Subtitle integration $evidence_name failed on attempt $attempt_number/$max_attempts; retrying if available." >&2
  done

  status=1
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
