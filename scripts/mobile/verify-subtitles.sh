#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PLATFORM="${1:-}"
if [[ -n "$PLATFORM" ]]; then
  shift
fi

SCOPE="regression"
DEVICE_ID=""
SIMULATOR_ID=""
EVIDENCE_DIR=""
IOS_DEVELOPMENT_TEAM="${VESPER_IOS_DEVELOPMENT_TEAM:-}"
IOS_FLUTTER_HOST_BUNDLE_ID="io.github.ikaros.flutterHost"

usage() {
  cat <<EOF >&2
Usage: $(basename "$0") <ios|android> [options]

Options:
  --scope <regression|device|complete>  Verification scope (default: regression)
  --device <id>                        Physical device identifier (required for device/complete)
  --simulator <id>                     iOS Simulator identifier (auto-selected when omitted)
  --evidence-dir <path>                New evidence directory to create
  -h, --help                           Show this help

The default evidence path is:
  devnotes/evidence/subtitle/<platform>/<UTC>-<short-sha>
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --scope)
      [[ $# -ge 2 ]] || { echo "Missing value for --scope." >&2; exit 1; }
      SCOPE="$2"
      shift 2
      ;;
    --scope=*)
      SCOPE="${1#--scope=}"
      shift
      ;;
    --device)
      [[ $# -ge 2 ]] || { echo "Missing value for --device." >&2; exit 1; }
      DEVICE_ID="$2"
      shift 2
      ;;
    --device=*)
      DEVICE_ID="${1#--device=}"
      shift
      ;;
    --simulator)
      [[ $# -ge 2 ]] || { echo "Missing value for --simulator." >&2; exit 1; }
      SIMULATOR_ID="$2"
      shift 2
      ;;
    --simulator=*)
      SIMULATOR_ID="${1#--simulator=}"
      shift
      ;;
    --evidence-dir)
      [[ $# -ge 2 ]] || { echo "Missing value for --evidence-dir." >&2; exit 1; }
      EVIDENCE_DIR="$2"
      shift 2
      ;;
    --evidence-dir=*)
      EVIDENCE_DIR="${1#--evidence-dir=}"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

case "$PLATFORM" in
  ios|android)
    ;;
  *)
    usage
    exit 1
    ;;
esac

case "$SCOPE" in
  regression|device|complete)
    ;;
  *)
    echo "Unsupported subtitle verification scope: $SCOPE" >&2
    exit 1
    ;;
esac

if [[ ( "$SCOPE" == "device" || "$SCOPE" == "complete" ) && -z "$DEVICE_ID" ]]; then
  echo "--device is required for subtitle scope '$SCOPE'." >&2
  exit 1
fi

if [[ "$SCOPE" == "regression" && -n "$DEVICE_ID" ]]; then
  echo "--device is not used by subtitle scope 'regression'." >&2
  exit 1
fi

if [[ "$PLATFORM" == "android" && -n "$SIMULATOR_ID" ]]; then
  echo "--simulator is supported only for iOS subtitle verification." >&2
  exit 1
fi

if [[ "$PLATFORM" == "ios" && "$SCOPE" == "device" && -n "$SIMULATOR_ID" ]]; then
  echo "--simulator is not used by subtitle scope 'device'." >&2
  exit 1
fi

vesper_require_command git
vesper_require_command ruby

SOURCE_SHA="$(git -C "$ROOT_DIR" rev-parse HEAD)"
SHORT_SHA="$(git -C "$ROOT_DIR" rev-parse --short=12 HEAD)"
STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$SHORT_SHA"

if [[ -z "$EVIDENCE_DIR" ]]; then
  EVIDENCE_DIR="$ROOT_DIR/devnotes/evidence/subtitle/$PLATFORM/$RUN_ID"
fi

if [[ -e "$EVIDENCE_DIR" || -L "$EVIDENCE_DIR" ]]; then
  echo "Subtitle evidence directory already exists: $EVIDENCE_DIR" >&2
  exit 1
fi

mkdir -p "$(dirname "$EVIDENCE_DIR")"
mkdir "$EVIDENCE_DIR"
EVIDENCE_DIR="$(cd "$EVIDENCE_DIR" && pwd)"

LOG_DIR="$EVIDENCE_DIR/logs"
PREFLIGHT_DIR="$EVIDENCE_DIR/preflight"
XCRESULT_DIR="$EVIDENCE_DIR/xcresult"
FLUTTER_EVIDENCE_DIR="$EVIDENCE_DIR/flutter"
ATTACHMENTS_DIR="$EVIDENCE_DIR/xctest-attachments"
STEPS_FILE="$EVIDENCE_DIR/steps.tsv"
SUMMARY_FILE="$EVIDENCE_DIR/summary.md"
MANIFEST_FILE="$EVIDENCE_DIR/manifest.json"
CHECKSUM_FILE="$EVIDENCE_DIR/SHA256SUMS"
TOOLCHAIN_FILE="$EVIDENCE_DIR/toolchain.txt"
SOURCE_STATUS_FILE="$EVIDENCE_DIR/source-status.txt"
RUN_TMP="$(mktemp -d "${TMPDIR:-/tmp}/vesper-subtitle-verify.XXXXXX")"

mkdir -p "$LOG_DIR" "$PREFLIGHT_DIR" "$XCRESULT_DIR" "$FLUTTER_EVIDENCE_DIR" "$ATTACHMENTS_DIR"
: > "$STEPS_FILE"
git -C "$ROOT_DIR" status --short > "$SOURCE_STATUS_FILE"
printf '%s\n' "$SOURCE_SHA" > "$EVIDENCE_DIR/source-sha.txt"

FINALIZED=0
FINISHED_AT=""
SELECTED_SIMULATOR_ID=""
SELECTED_DEVICE_SUMMARY=""
IOS_PROJECTS_GENERATED=0
FLUTTER_DEPENDENCIES_READY=0

record_step() {
  local name="$1"
  local result="$2"
  local duration_seconds="$3"
  local log_path="$4"
  printf '%s\t%s\t%s\t%s\n' "$name" "$result" "$duration_seconds" "$log_path" >> "$STEPS_FILE"
}

run_logged() {
  local name="$1"
  local working_directory="$2"
  shift 2
  local log_path="$LOG_DIR/$name.log"
  local started_epoch
  local finished_epoch
  local status

  started_epoch="$(date +%s)"
  if (
    cd "$working_directory"
    printf 'Working directory: %s\n' "$PWD"
    printf 'Command:'
    printf ' %q' "$@"
    printf '\n\n'
    "$@"
  ) 2>&1 | tee "$log_path"; then
    status=0
  else
    status=$?
  fi
  finished_epoch="$(date +%s)"

  if [[ "$status" -eq 0 ]]; then
    record_step "$name" passed "$((finished_epoch - started_epoch))" "logs/$name.log"
  else
    record_step "$name" failed "$((finished_epoch - started_epoch))" "logs/$name.log"
  fi
  return "$status"
}

capture_output() {
  local name="$1"
  local output_path="$2"
  local working_directory="$3"
  shift 3
  local log_path="$LOG_DIR/$name.log"
  local started_epoch
  local finished_epoch
  local status

  started_epoch="$(date +%s)"
  {
    printf 'Working directory: %s\n' "$working_directory"
    printf 'Command:'
    printf ' %q' "$@"
    printf '\n\n'
  } > "$log_path"

  if (cd "$working_directory" && "$@") > "$output_path" 2>> "$log_path"; then
    status=0
  else
    status=$?
  fi
  cat "$output_path" >> "$log_path"
  printf 'Captured output: %s\n' "$output_path"
  finished_epoch="$(date +%s)"

  if [[ "$status" -eq 0 ]]; then
    record_step "$name" passed "$((finished_epoch - started_epoch))" "logs/$name.log"
  else
    record_step "$name" failed "$((finished_epoch - started_epoch))" "logs/$name.log"
  fi
  return "$status"
}

require_literal_in_file() {
  local path="$1"
  local literal="$2"
  local failure_message="$3"

  grep -F -- "$literal" "$path" >/dev/null || {
    echo "$failure_message" >&2
    return 1
  }
}

verify_flutter_evidence() {
  local output_dir="$1"
  local evidence_name="$2"

  ruby -rjson -e '
    output_dir = ARGV.fetch(0)
    evidence_name = ARGV.fetch(1)
    json_path = File.join(output_dir, "#{evidence_name}.json")
    abort("Missing Flutter subtitle evidence JSON: #{json_path}") unless File.file?(json_path) && File.size?(json_path)
    payload = JSON.parse(File.read(json_path))
    abort("Unexpected Flutter subtitle evidence name in #{json_path}") unless payload["evidenceName"] == evidence_name

    case evidence_name
    when "subtitle-positive"
      snapshot = payload.fetch("snapshot")
      frame = snapshot.fetch("frame")
      abort("Flutter subtitle evidence did not capture Subtitle B") unless snapshot["text"] == "Subtitle B"
      abort("Flutter subtitle evidence is not visibly attached") unless
        snapshot["visible"] == true && snapshot["hidden"] == false &&
          snapshot["windowAttached"] == true && snapshot.fetch("alpha").to_f > 0 &&
          frame.fetch("width").to_f > 0 && frame.fetch("height").to_f > 0
      png_name = payload["pngFile"]
      abort("Flutter subtitle evidence did not declare the expected PNG") unless png_name == "#{evidence_name}.png"
      png_path = File.join(output_dir, png_name)
      abort("Missing Flutter subtitle evidence PNG: #{png_path}") unless File.file?(png_path) && File.size?(png_path)
      abort("Invalid Flutter subtitle evidence PNG: #{png_path}") unless File.binread(png_path, 8) == "\x89PNG\r\n\x1a\n".b
    when "subtitle-lifecycle"
      scenarios = payload.fetch("scenarios")
      {
        "timeout" => "subtitle_selection_timeout",
        "sourceChange" => "subtitle_source_changed",
        "supersede" => "subtitle_selection_superseded"
      }.each do |scenario_name, expected_code|
        scenario = scenarios.fetch(scenario_name)
        error = scenario.fetch("error")
        abort("Unexpected #{scenario_name} evidence code") unless error["code"] == expected_code
        abort("Missing #{scenario_name} transaction identity") unless
          error["commandId"].is_a?(Integer) && error["commandId"] > 0 &&
            error["sourceEpoch"].is_a?(Integer)
      end
    else
      abort("Unsupported Flutter subtitle evidence name: #{evidence_name}")
    end
  ' "$output_dir" "$evidence_name"
}

verify_ios_device_attachments() {
  local attachments_dir="$1"

  ruby -rjson -e '
    attachments_dir = ARGV.fetch(0)
    manifest_path = File.join(attachments_dir, "manifest.json")
    abort("Missing XCTest attachment manifest: #{manifest_path}") unless File.file?(manifest_path) && File.size?(manifest_path)
    manifest = JSON.parse(File.read(manifest_path))
    records = manifest.flat_map { |entry| entry.fetch("attachments", []) }
    snapshot_record = records.find do |record|
      record.fetch("suggestedHumanReadableName", "").start_with?("subtitle-overlay-snapshot_")
    end
    image_record = records.find do |record|
      name = record.fetch("suggestedHumanReadableName", "")
      name.start_with?("subtitle-overlay_") && name.end_with?(".png")
    end
    abort("Missing subtitle overlay snapshot XCTest attachment") unless snapshot_record
    abort("Missing subtitle overlay PNG XCTest attachment") unless image_record

    snapshot_path = File.join(attachments_dir, snapshot_record.fetch("exportedFileName"))
    image_path = File.join(attachments_dir, image_record.fetch("exportedFileName"))
    abort("Missing exported subtitle overlay snapshot: #{snapshot_path}") unless File.file?(snapshot_path) && File.size?(snapshot_path)
    abort("Missing exported subtitle overlay PNG: #{image_path}") unless File.file?(image_path) && File.size?(image_path)

    snapshot = JSON.parse(File.read(snapshot_path))
    frame = snapshot.fetch("frame")
    abort("XCTest subtitle attachment did not capture Subtitle B") unless snapshot["text"] == "Subtitle B"
    abort("XCTest subtitle attachment is not visibly attached") unless
      snapshot["visible"] == true && snapshot["hidden"] == false &&
        snapshot["windowAttached"] == true && snapshot.fetch("alpha").to_f > 0 &&
        frame.fetch("width").to_f > 0 && frame.fetch("height").to_f > 0
    abort("Invalid XCTest subtitle overlay PNG: #{image_path}") unless File.binread(image_path, 8) == "\x89PNG\r\n\x1a\n".b
  ' "$attachments_dir"
}

verify_xcresult_tests() {
  local result_bundle="$1"
  local summary_path="$2"
  local tests_path="$3"
  shift 3

  if [[ ! -d "$result_bundle" || ! -s "$result_bundle/Info.plist" ]]; then
    echo "Missing XCResult bundle: $result_bundle" >&2
    return 1
  fi

  xcrun xcresulttool get test-results summary \
    --path "$result_bundle" > "$summary_path"
  xcrun xcresulttool get test-results tests \
    --path "$result_bundle" > "$tests_path"

  ruby -rjson -e '
    summary = JSON.parse(File.read(ARGV.fetch(0)))
    abort("XCResult did not pass") unless
      summary["result"] == "Passed" && summary.fetch("failedTests", 0).zero? &&
        summary.fetch("totalTestCount", 0) > 0

    tests = JSON.parse(File.read(ARGV.fetch(1)))
    nodes = []
    visit = lambda do |node|
      nodes << node
      node.fetch("children", []).each { |child| visit.call(child) }
    end
    tests.fetch("testNodes").each { |node| visit.call(node) }

    ARGV.drop(2).each do |suite_spec|
      suite_name, minimum_count_text = suite_spec.split("=", 2)
      minimum_count = Integer(minimum_count_text || "1", 10)
      suite = nodes.find do |node|
        node["nodeType"] == "Test Suite" && node["name"] == suite_name
      end
      abort("XCResult is missing expected test suite: #{suite_name}") unless suite
      suite_nodes = []
      collect = lambda do |node|
        suite_nodes << node
        node.fetch("children", []).each { |child| collect.call(child) }
      end
      collect.call(suite)
      cases = suite_nodes.select { |node| node["nodeType"] == "Test Case" }
      abort("XCResult suite executed #{cases.length} tests; expected at least #{minimum_count}: #{suite_name}") if
        cases.length < minimum_count
      abort("XCResult suite did not pass: #{suite_name}") unless
        suite["result"] == "Passed" && cases.all? { |test_case| test_case["result"] == "Passed" }
      puts "#{suite_name}=#{cases.length}"
    end
  ' "$summary_path" "$tests_path" "$@"
}

write_summary() {
  local exit_code="$1"
  local result="passed"
  local step_name
  local step_result
  local duration_seconds
  local log_path

  if [[ "$exit_code" -ne 0 ]]; then
    result="failed"
  fi

  {
    echo "# Vesper Subtitle Verification"
    echo
    echo "- Result: $result"
    echo "- Platform: $PLATFORM"
    echo "- Scope: $SCOPE"
    echo "- Run ID: $RUN_ID"
    echo "- Source SHA: $SOURCE_SHA"
    echo "- Started: $STARTED_AT"
    echo "- Finished: $FINISHED_AT"
    echo "- Device: ${DEVICE_ID:-not requested}"
    echo "- Simulator: ${SELECTED_SIMULATOR_ID:-not requested}"
    echo "- Evidence: $EVIDENCE_DIR"
    echo
    echo "## Steps"
    echo
    echo "| Step | Result | Seconds | Log |"
    echo "| --- | --- | ---: | --- |"
    while IFS=$'\t' read -r step_name step_result duration_seconds log_path; do
      printf '| `%s` | %s | %s | `%s` |\n' \
        "$step_name" "$step_result" "$duration_seconds" "$log_path"
    done < "$STEPS_FILE"
  } > "$SUMMARY_FILE"
}

write_manifest() {
  local exit_code="$1"
  VESPER_SUBTITLE_RESULT="$(if [[ "$exit_code" -eq 0 ]]; then echo passed; else echo failed; fi)" \
  VESPER_SUBTITLE_EXIT_CODE="$exit_code" \
  VESPER_SUBTITLE_PLATFORM="$PLATFORM" \
  VESPER_SUBTITLE_SCOPE="$SCOPE" \
  VESPER_SUBTITLE_RUN_ID="$RUN_ID" \
  VESPER_SUBTITLE_SOURCE_SHA="$SOURCE_SHA" \
  VESPER_SUBTITLE_STARTED_AT="$STARTED_AT" \
  VESPER_SUBTITLE_FINISHED_AT="$FINISHED_AT" \
  VESPER_SUBTITLE_DEVICE_ID="$DEVICE_ID" \
  VESPER_SUBTITLE_SIMULATOR_ID="$SELECTED_SIMULATOR_ID" \
  VESPER_SUBTITLE_STEPS_FILE="$STEPS_FILE" \
  VESPER_SUBTITLE_SOURCE_STATUS_FILE="$SOURCE_STATUS_FILE" \
  VESPER_SUBTITLE_SELECTED_DEVICE_FILE="$SELECTED_DEVICE_SUMMARY" \
  VESPER_SUBTITLE_MANIFEST_FILE="$MANIFEST_FILE" \
    ruby -rjson -e '
      steps = File.readlines(ENV.fetch("VESPER_SUBTITLE_STEPS_FILE"), chomp: true).map do |line|
        name, result, seconds, log = line.split("\t", 4)
        {"name" => name, "result" => result, "durationSeconds" => seconds.to_i, "log" => log}
      end
      selected_device_file = ENV.fetch("VESPER_SUBTITLE_SELECTED_DEVICE_FILE")
      selected_device = if !selected_device_file.empty? && File.file?(selected_device_file)
        JSON.parse(File.read(selected_device_file))
      end
      manifest = {
        "schema" => "vesper-subtitle-evidence-v1",
        "result" => ENV.fetch("VESPER_SUBTITLE_RESULT"),
        "exitCode" => ENV.fetch("VESPER_SUBTITLE_EXIT_CODE").to_i,
        "platform" => ENV.fetch("VESPER_SUBTITLE_PLATFORM"),
        "scope" => ENV.fetch("VESPER_SUBTITLE_SCOPE"),
        "runId" => ENV.fetch("VESPER_SUBTITLE_RUN_ID"),
        "sourceSha" => ENV.fetch("VESPER_SUBTITLE_SOURCE_SHA"),
        "sourceDirty" => !File.empty?(ENV.fetch("VESPER_SUBTITLE_SOURCE_STATUS_FILE")),
        "startedAt" => ENV.fetch("VESPER_SUBTITLE_STARTED_AT"),
        "finishedAt" => ENV.fetch("VESPER_SUBTITLE_FINISHED_AT"),
        "deviceId" => (ENV.fetch("VESPER_SUBTITLE_DEVICE_ID").empty? ? nil : ENV.fetch("VESPER_SUBTITLE_DEVICE_ID")),
        "simulatorId" => (ENV.fetch("VESPER_SUBTITLE_SIMULATOR_ID").empty? ? nil : ENV.fetch("VESPER_SUBTITLE_SIMULATOR_ID")),
        "selectedDevice" => selected_device,
        "steps" => steps,
        "artifacts" => {
          "summary" => "summary.md",
          "toolchain" => "toolchain.txt",
          "sourceStatus" => "source-status.txt",
          "logs" => "logs",
          "xcresults" => "xcresult",
          "flutter" => "flutter",
          "xctestAttachments" => "xctest-attachments",
          "checksums" => "SHA256SUMS"
        }
      }
      File.write(ENV.fetch("VESPER_SUBTITLE_MANIFEST_FILE"), JSON.pretty_generate(manifest) + "\n")
    '
}

write_checksums() {
  if command -v shasum >/dev/null 2>&1; then
    (
      cd "$EVIDENCE_DIR"
      find . -type f ! -name SHA256SUMS -print | LC_ALL=C sort | (
        checksum_result=0
        while IFS= read -r path; do
          if ! shasum -a 256 "$path"; then
            checksum_result=1
          fi
        done
        exit "$checksum_result"
      )
    ) > "$CHECKSUM_FILE"
  elif command -v sha256sum >/dev/null 2>&1; then
    (
      cd "$EVIDENCE_DIR"
      find . -type f ! -name SHA256SUMS -print | LC_ALL=C sort | (
        checksum_result=0
        while IFS= read -r path; do
          if ! sha256sum "$path"; then
            checksum_result=1
          fi
        done
        exit "$checksum_result"
      )
    ) > "$CHECKSUM_FILE"
  else
    echo "Neither shasum nor sha256sum is available." >&2
    return 1
  fi
}

finalize() {
  local exit_code=$?
  local metadata_failed=0
  local checksum_failed=0

  if [[ "$FINALIZED" -eq 1 ]]; then
    exit "$exit_code"
  fi
  FINALIZED=1
  trap - EXIT
  FINISHED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  set +e
  write_summary "$exit_code" || metadata_failed=1
  write_manifest "$exit_code" || metadata_failed=1

  if [[ "$metadata_failed" -ne 0 ]]; then
    exit_code=1
    write_summary "$exit_code"
    write_manifest "$exit_code"
  fi

  if ! write_checksums; then
    checksum_failed=1
    exit_code=1
    rm -f "$CHECKSUM_FILE"
    write_summary "$exit_code"
    write_manifest "$exit_code"
    write_checksums || rm -f "$CHECKSUM_FILE"
  fi
  rm -rf "$RUN_TMP"
  set -e

  if [[ "$metadata_failed" -ne 0 || "$checksum_failed" -ne 0 ]]; then
    exit_code=1
  fi

  echo
  echo "Subtitle verification evidence: $EVIDENCE_DIR"
  exit "$exit_code"
}
trap finalize EXIT

collect_toolchain() {
  {
    echo "uname:"
    uname -a
    echo
    echo "git:"
    git --version
    echo
    echo "ruby:"
    ruby --version
    echo
    echo "rustc:"
    rustc --version
    echo
    echo "cargo:"
    cargo --version
    if command -v flutter >/dev/null 2>&1; then
      echo
      echo "flutter:"
      flutter --version
    fi
    if [[ "$PLATFORM" == "ios" ]]; then
      echo
      echo "xcodebuild:"
      xcodebuild -version
      echo
      echo "swiftc:"
      swiftc --version
      echo
      echo "devicectl:"
      xcrun devicectl --version
    else
      if command -v adb >/dev/null 2>&1; then
        echo
        echo "adb:"
        adb version
      fi
      if command -v java >/dev/null 2>&1; then
        echo
        echo "java:"
        java -version 2>&1
      fi
    fi
  } | tee "$TOOLCHAIN_FILE"
}

prepare_ios_projects() {
  if [[ "$IOS_PROJECTS_GENERATED" -eq 1 ]]; then
    return 0
  fi
  vesper_require_command xcodegen "xcodegen is required for iOS subtitle verification."
  run_logged ios-player-kit-xcodegen "$ROOT_DIR/lib/ios/VesperPlayerKit" xcodegen generate
  run_logged ios-host-xcodegen "$ROOT_DIR/examples/ios-swift-host" xcodegen generate
  IOS_PROJECTS_GENERATED=1
}

prepare_flutter_dependencies() {
  if [[ "$FLUTTER_DEPENDENCIES_READY" -eq 1 ]]; then
    return 0
  fi
  vesper_require_command flutter "Flutter is required for subtitle verification."
  run_logged flutter-host-pub-get "$ROOT_DIR/examples/flutter-host" flutter pub get
  FLUTTER_DEPENDENCIES_READY=1
}

prepare_ios_simulator() {
  local simctl_json="$PREFLIGHT_DIR/simctl-devices.json"
  local selected_json="$PREFLIGHT_DIR/selected-simulator.json"
  local flutter_devices_json="$PREFLIGHT_DIR/flutter-devices-simulator.json"
  local xcode_destinations="$PREFLIGHT_DIR/xcode-destinations-simulator.txt"
  local simulator_state

  capture_output ios-simctl-devices "$simctl_json" "$ROOT_DIR" \
    xcrun simctl list devices available --json

  run_logged ios-simulator-select "$ROOT_DIR" \
    ruby -rjson -e '
      data = JSON.parse(File.read(ARGV.fetch(0)))
      requested = ARGV.fetch(1)
      devices = data.fetch("devices").flat_map do |runtime_identifier, runtime_devices|
        match = runtime_identifier.match(/SimRuntime\.iOS-(\d+)(?:-(\d+))?(?:-(\d+))?\z/)
        next [] unless match
        version = [match[1].to_i, match[2].to_s.to_i, match[3].to_s.to_i]
        next [] if (version <=> [17, 0, 0]) < 0
        runtime_devices.map do |device|
          device.merge(
            "_runtimeIdentifier" => runtime_identifier,
            "_runtimeVersion" => version
          )
        end
      end.select do |device|
        device["isAvailable"] != false && device.fetch("name", "").start_with?("iPhone")
      end
      selected = if requested.empty?
        devices.select { |device| device["state"] == "Booted" }
          .max_by { |device| device.fetch("_runtimeVersion") } ||
          devices.max_by { |device| device.fetch("_runtimeVersion") }
      else
        devices.find { |device| device["udid"] == requested }
      end
      abort("No matching available iPhone Simulator with iOS 17 or newer was found.") unless selected
      version = selected.fetch("_runtimeVersion")
      File.write(ARGV.fetch(2), JSON.pretty_generate({
        "id" => selected["udid"],
        "name" => selected["name"],
        "state" => selected["state"],
        "isAvailable" => selected.fetch("isAvailable", true),
        "runtimeIdentifier" => selected.fetch("_runtimeIdentifier"),
        "osVersion" => version.take(version[2].zero? ? 2 : 3).join(".")
      }) + "\n")
    ' "$simctl_json" "$SIMULATOR_ID" "$selected_json"

  SELECTED_SIMULATOR_ID="$(ruby -rjson -e 'puts JSON.parse(File.read(ARGV.fetch(0))).fetch("id")' "$selected_json")"

  simulator_state="$(ruby -rjson -e 'puts JSON.parse(File.read(ARGV.fetch(0))).fetch("state")' "$selected_json")"
  if [[ "$simulator_state" != "Booted" ]]; then
    run_logged ios-simulator-boot "$ROOT_DIR" xcrun simctl boot "$SELECTED_SIMULATOR_ID"
  fi
  run_logged ios-simulator-bootstatus "$ROOT_DIR" \
    xcrun simctl bootstatus "$SELECTED_SIMULATOR_ID" -b

  capture_output ios-flutter-simulator-devices "$flutter_devices_json" \
    "$ROOT_DIR/examples/flutter-host" flutter devices --machine
  run_logged ios-flutter-simulator-match "$ROOT_DIR" \
    ruby -rjson -e '
    devices = JSON.parse(File.read(ARGV.fetch(0)))
    id = ARGV.fetch(1)
    match = devices.find do |device|
      device["id"] == id && device["emulator"] == true &&
        device["targetPlatform"].to_s.start_with?("ios") && device["isSupported"] == true
    end
    abort("Flutter does not expose the requested iOS Simulator: #{id}") unless match
    ' "$flutter_devices_json" "$SELECTED_SIMULATOR_ID"

  capture_output ios-xcode-simulator-destinations "$xcode_destinations" "$ROOT_DIR" \
    xcodebuild -project examples/ios-swift-host/VesperPlayerHostDemo.xcodeproj \
      -scheme VesperPlayerHostDemo -showdestinations
  run_logged ios-xcode-simulator-destination-match "$ROOT_DIR" \
    require_literal_in_file "$xcode_destinations" "id:$SELECTED_SIMULATOR_ID" \
      "Xcode does not expose the requested iOS Simulator: $SELECTED_SIMULATOR_ID"
}

verify_ios_signing_certificate() {
  local identities_file="$1"
  local certificates_pem="$RUN_TMP/codesigning-certificates.pem"
  local selected_certificate="$PREFLIGHT_DIR/selected-signing-certificate.txt"

  security find-certificate -a -p > "$certificates_pem"
  ruby -ropenssl -rtime -e '
    team = ARGV.fetch(0)
    pem = File.read(ARGV.fetch(1))
    identities = File.read(ARGV.fetch(2))
    identity_hashes = identities.scan(/\b[0-9A-Fa-f]{40}\b/).map(&:upcase)
    now = Time.now
    certificates = pem.scan(/-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----/m).map do |entry|
      begin
        OpenSSL::X509::Certificate.new(entry)
      rescue OpenSSL::X509::CertificateError
        nil
      end
    end.compact
    selected = certificates.find do |certificate|
      ou = certificate.subject.to_a.assoc("OU")&.at(1)
      common_name = certificate.subject.to_a.assoc("CN")&.at(1).to_s
      development_identity = common_name.start_with?("Apple Development:") ||
        common_name.start_with?("iPhone Developer:")
      sha1 = OpenSSL::Digest::SHA1.hexdigest(certificate.to_der).upcase
      ou == team && development_identity &&
        certificate.not_before <= now && certificate.not_after >= now &&
        identity_hashes.include?(sha1)
    end
    abort("No currently valid development signing identity was found for Team ID #{team}.") unless selected
    common_name = selected.subject.to_a.assoc("CN")&.at(1)
    puts "teamId=#{team}"
    puts "commonName=#{common_name}"
    puts "notBefore=#{selected.not_before.utc.iso8601}"
    puts "notAfter=#{selected.not_after.utc.iso8601}"
    puts "sha1=#{OpenSSL::Digest::SHA1.hexdigest(selected.to_der).upcase}"
    puts "sha256=#{OpenSSL::Digest::SHA256.hexdigest(selected.to_der)}"
  ' "$IOS_DEVELOPMENT_TEAM" "$certificates_pem" "$identities_file" | tee "$selected_certificate"
}

prepare_ios_device() {
  local devicectl_json="$RUN_TMP/devicectl-devices.json"
  local selected_device_json="$PREFLIGHT_DIR/selected-device.json"
  local flutter_devices_json="$PREFLIGHT_DIR/flutter-devices-device.json"
  local xcode_destinations="$PREFLIGHT_DIR/xcode-destinations-device.txt"
  local identities_file="$PREFLIGHT_DIR/codesigning-identities.txt"

  if [[ -z "$IOS_DEVELOPMENT_TEAM" ]]; then
    echo "VESPER_IOS_DEVELOPMENT_TEAM is required for physical iOS subtitle verification." >&2
    return 1
  fi

  run_logged ios-devicectl-devices "$ROOT_DIR" \
    xcrun devicectl list devices --timeout 30 --json-output "$devicectl_json"

  run_logged ios-device-coredevice-match "$ROOT_DIR" \
    ruby -rjson -e '
    data = JSON.parse(File.read(ARGV.fetch(0)))
    id = ARGV.fetch(1)
    device = data.dig("result", "devices")&.find { |entry| entry.dig("hardwareProperties", "udid") == id }
    abort("CoreDevice does not expose the requested iOS device: #{id}") unless device
    pairing = device.dig("connectionProperties", "pairingState")
    tunnel_state = device.dig("connectionProperties", "tunnelState")
    developer_mode = device.dig("deviceProperties", "developerModeStatus")
    boot_state = device.dig("deviceProperties", "bootState")
    ddi_services_available = device.dig("deviceProperties", "ddiServicesAvailable")
    connect_capability = device.fetch("capabilities", []).any? do |capability|
      capability["featureIdentifier"] == "com.apple.coredevice.feature.connectdevice"
    end
    connected_and_ready = tunnel_state == "connected" && ddi_services_available == true
    abort("The requested iOS device is not paired.") unless pairing == "paired"
    abort("Developer Mode is not enabled on the requested iOS device.") unless developer_mode == "enabled"
    abort("The requested iOS device is not booted and available.") unless
      boot_state == "booted" && (connect_capability || connected_and_ready)
    selected = {
      "id" => id,
      "name" => device.dig("deviceProperties", "name"),
      "model" => device.dig("hardwareProperties", "marketingName"),
      "productType" => device.dig("hardwareProperties", "productType"),
      "osVersion" => device.dig("deviceProperties", "osVersionNumber"),
      "osBuild" => device.dig("deviceProperties", "osBuildUpdate"),
      "releaseType" => device.dig("deviceProperties", "releaseType"),
      "pairingState" => pairing,
      "developerMode" => developer_mode,
      "bootState" => boot_state,
      "transport" => device.dig("connectionProperties", "transportType"),
      "tunnelState" => tunnel_state,
      "ddiServicesAvailable" => ddi_services_available
    }
    File.write(ARGV.fetch(2), JSON.pretty_generate(selected) + "\n")
    ' "$devicectl_json" "$DEVICE_ID" "$selected_device_json"
  SELECTED_DEVICE_SUMMARY="$selected_device_json"

  capture_output ios-flutter-device-list "$flutter_devices_json" \
    "$ROOT_DIR/examples/flutter-host" flutter devices --machine
  run_logged ios-flutter-device-match "$ROOT_DIR" \
    ruby -rjson -e '
    devices = JSON.parse(File.read(ARGV.fetch(0)))
    id = ARGV.fetch(1)
    match = devices.find do |device|
      device["id"] == id && device["emulator"] == false &&
        device["targetPlatform"].to_s.start_with?("ios") && device["isSupported"] == true
    end
    abort("Flutter does not expose the requested physical iOS device: #{id}") unless match
    ' "$flutter_devices_json" "$DEVICE_ID"

  capture_output ios-xcode-device-destinations "$xcode_destinations" "$ROOT_DIR" \
    xcodebuild -project examples/ios-swift-host/VesperPlayerHostDemo.xcodeproj \
      -scheme VesperPlayerHostDemo -showdestinations
  run_logged ios-xcode-device-destination-match "$ROOT_DIR" \
    require_literal_in_file "$xcode_destinations" "id:$DEVICE_ID" \
      "Xcode does not expose the requested physical iOS device: $DEVICE_ID"

  capture_output ios-codesigning-identities "$identities_file" "$ROOT_DIR" \
    security find-identity -v -p codesigning
  run_logged ios-signing-certificate "$ROOT_DIR" \
    verify_ios_signing_certificate "$identities_file"
  run_logged ios-codesigning-identity-match "$ROOT_DIR" \
    ruby -e '
      identities = File.read(ARGV.fetch(0))
      certificate = File.read(ARGV.fetch(1))
      counts = identities.lines.map do |line|
        match = line.match(/^\s*(\d+) valid identities found\s*$/)
        match && match[1].to_i
      end.compact
      abort("No valid code-signing identity was reported by the security tool.") unless counts.last.to_i > 0
      sha1_line = certificate.lines.find { |line| line.start_with?("sha1=") }
      abort("The selected signing certificate did not report its SHA-1 identity.") unless sha1_line
      sha1 = sha1_line.split("=", 2).last.strip
      abort("The selected Team certificate has no matching private-key identity.") unless identities.include?(sha1)
    ' "$identities_file" "$PREFLIGHT_DIR/selected-signing-certificate.txt"
}

terminate_ios_flutter_host_processes() {
  local target_device="$1"
  local evidence_name="$2"
  local apps_json="$RUN_TMP/ios-flutter-apps-$evidence_name.json"
  local processes_json="$RUN_TMP/ios-flutter-processes-$evidence_name.json"
  local pids_file="$RUN_TMP/ios-flutter-pids-$evidence_name.txt"
  local pid

  xcrun devicectl device info apps \
    --device "$target_device" \
    --json-output "$apps_json" \
    --quiet || return $?
  xcrun devicectl device info processes \
    --device "$target_device" \
    --json-output "$processes_json" \
    --quiet || return $?

  ruby -rjson -e '
    apps = JSON.parse(File.read(ARGV.fetch(0))).dig("result", "apps") || []
    processes = JSON.parse(File.read(ARGV.fetch(1))).dig("result", "runningProcesses") || []
    bundle_id = ARGV.fetch(2)
    matches = apps.select { |app| app["bundleIdentifier"] == bundle_id }
    abort("Multiple installed apps reported bundle identifier #{bundle_id}.") if matches.length > 1
    exit 0 if matches.empty?

    app_url = matches.first.fetch("url")
    unless app_url.start_with?("file:///private/var/containers/Bundle/Application/") &&
        app_url.end_with?(".app/")
      abort("Refusing to terminate a process for unexpected app URL: #{app_url}")
    end
    app_directory = File.basename(app_url.chomp("/"))
    executable_url = "#{app_url}#{app_directory.delete_suffix(".app")}"
    processes.each do |process|
      next unless process["executable"] == executable_url
      pid = process["processIdentifier"]
      abort("Invalid process identifier for #{bundle_id}: #{pid.inspect}") unless
        pid.is_a?(Integer) && pid.positive?
      puts pid
    end
  ' "$apps_json" "$processes_json" "$IOS_FLUTTER_HOST_BUNDLE_ID" > "$pids_file" || return $?

  if [[ ! -s "$pids_file" ]]; then
    echo "No running iOS Flutter host process requires cleanup."
    return 0
  fi

  while IFS= read -r pid; do
    local current_processes_json="$RUN_TMP/ios-flutter-processes-$evidence_name-$pid-current.json"
    local verification_status=0

    case "$pid" in
      ''|*[!0-9]*)
        echo "Invalid iOS Flutter host process identifier: $pid" >&2
        return 1
        ;;
    esac

    xcrun devicectl device info processes \
      --device "$target_device" \
      --json-output "$current_processes_json" \
      --quiet || return $?
    if ruby -rjson -e '
      apps = JSON.parse(File.read(ARGV.fetch(0))).dig("result", "apps") || []
      processes = JSON.parse(File.read(ARGV.fetch(1))).dig("result", "runningProcesses") || []
      bundle_id = ARGV.fetch(2)
      expected_pid = Integer(ARGV.fetch(3), 10)
      app = apps.find { |entry| entry["bundleIdentifier"] == bundle_id }
      abort("Installed app metadata disappeared for #{bundle_id}.") unless app
      app_url = app.fetch("url")
      app_directory = File.basename(app_url.chomp("/"))
      executable_url = "#{app_url}#{app_directory.delete_suffix(".app")}"
      process = processes.find { |entry| entry["processIdentifier"] == expected_pid }
      exit 3 unless process
      unless process["executable"] == executable_url
        abort("Refusing to terminate reused PID #{expected_pid}: #{process["executable"].inspect}")
      end
    ' "$apps_json" "$current_processes_json" "$IOS_FLUTTER_HOST_BUNDLE_ID" "$pid"; then
      verification_status=0
    else
      verification_status=$?
    fi
    if [[ "$verification_status" -eq 3 ]]; then
      echo "iOS Flutter host process $pid exited before cleanup."
      continue
    fi
    if [[ "$verification_status" -ne 0 ]]; then
      return "$verification_status"
    fi

    echo "Terminating stale iOS Flutter host process: bundle=$IOS_FLUTTER_HOST_BUNDLE_ID pid=$pid"
    xcrun devicectl device process terminate \
      --device "$target_device" \
      --pid "$pid" \
      --timeout 15 || return $?
  done < "$pids_file"
}

run_flutter_integration() {
  local target_device="$1"
  local target_kind="$2"
  local evidence_name="$3"
  local test_target="$4"
  local output_dir="$FLUTTER_EVIDENCE_DIR/$target_kind"
  local drive_status=0
  local cleanup_status=0

  mkdir -p "$output_dir"
  if [[ "$PLATFORM" == "ios" && "$target_kind" == "device" ]]; then
    run_logged "flutter-$target_kind-$evidence_name-cleanup-before" "$ROOT_DIR" \
      terminate_ios_flutter_host_processes "$target_device" "$evidence_name-before"
    if run_logged "flutter-$target_kind-$evidence_name" "$ROOT_DIR/examples/flutter-host" \
      env DEVELOPMENT_TEAM="$IOS_DEVELOPMENT_TEAM" \
        VESPER_SUBTITLE_EVIDENCE_DIR="$output_dir" \
        VESPER_SUBTITLE_EVIDENCE_NAME="$evidence_name" \
        flutter drive \
          --no-keep-app-running \
          --device-connection attached \
          --driver=test_driver/subtitle_integration_test.dart \
          --target="$test_target" \
          --device-id "$target_device"; then
      drive_status=0
    else
      drive_status=$?
    fi
    run_logged "flutter-$target_kind-$evidence_name-cleanup-after" "$ROOT_DIR" \
      terminate_ios_flutter_host_processes "$target_device" "$evidence_name-after" || \
      cleanup_status=$?
    if [[ "$drive_status" -ne 0 ]]; then
      return "$drive_status"
    fi
    if [[ "$cleanup_status" -ne 0 ]]; then
      return "$cleanup_status"
    fi
  else
    run_logged "flutter-$target_kind-$evidence_name" "$ROOT_DIR/examples/flutter-host" \
      env VESPER_SUBTITLE_EVIDENCE_DIR="$output_dir" \
        VESPER_SUBTITLE_EVIDENCE_NAME="$evidence_name" \
        flutter drive \
          --driver=test_driver/subtitle_integration_test.dart \
          --target="$test_target" \
          --device-id "$target_device"
  fi

  run_logged "flutter-$target_kind-$evidence_name-evidence" "$ROOT_DIR" \
    verify_flutter_evidence "$output_dir" "$evidence_name"
}

run_common_contract_regression() {
  run_logged contract-verify "$ROOT_DIR" "$ROOT_DIR/scripts/vesper" contract verify
}

run_ios_regression() {
  local result_bundle="$XCRESULT_DIR/ios-simulator.xcresult"
  local derived_data="$RUN_TMP/ios-simulator-derived"

  vesper_require_command xcodebuild
  vesper_require_command xcrun
  vesper_require_command flutter
  vesper_require_command cargo

  run_common_contract_regression
  run_logged ios-rust-subtitle-tests "$ROOT_DIR" \
    cargo test -p player-ffi -p player-ffi-ios -p player-platform-ios
  run_logged ios-simulator-ffi "$ROOT_DIR" \
    env VESPER_BUILD_IOS_PLAYER_FFI_MODE=platform PLATFORM_NAME=iphonesimulator \
      "$ROOT_DIR/scripts/vesper" ios ffi debug
  prepare_ios_projects
  prepare_flutter_dependencies
  prepare_ios_simulator

  run_logged ios-simulator-xctest "$ROOT_DIR" \
    xcodebuild test \
      -project lib/ios/VesperPlayerKit/VesperPlayerKit.xcodeproj \
      -scheme VesperPlayerKit \
      -configuration Debug \
      -destination "platform=iOS Simulator,id=$SELECTED_SIMULATOR_ID" \
      -resultBundlePath "$result_bundle" \
      -derivedDataPath "$derived_data" \
      CODE_SIGNING_ALLOWED=NO \
      CODE_SIGNING_REQUIRED=NO \
      -only-testing:VesperPlayerKitTests/VesperNativeSubtitleStateTests \
      -only-testing:VesperPlayerKitTests/VesperSubtitleOverlayRendererTests
  run_logged ios-simulator-xcresult-evidence "$ROOT_DIR" \
    verify_xcresult_tests "$result_bundle" \
      "$XCRESULT_DIR/ios-simulator-summary.json" \
      "$XCRESULT_DIR/ios-simulator-tests.json" \
      VesperNativeSubtitleStateTests=40 \
      VesperSubtitleOverlayRendererTests=10

  run_logged flutter-platform-subtitle-tests \
    "$ROOT_DIR/lib/flutter/vesper_player_platform_interface" \
    flutter test test/subtitle_exception_test.dart test/subtitle_state_models_test.dart
  run_logged flutter-controller-subtitle-tests "$ROOT_DIR/lib/flutter/vesper_player" \
    flutter test test/vesper_download_manager_test.dart
  run_logged flutter-ios-channel-tests "$ROOT_DIR/lib/flutter/vesper_player_ios" \
    flutter test test/method_channel_vesper_player_ios_test.dart
  run_logged flutter-host-subtitle-evidence-test "$ROOT_DIR/examples/flutter-host" \
    flutter test test/subtitle_overlay_evidence_test.dart

  run_flutter_integration "$SELECTED_SIMULATOR_ID" simulator subtitle-positive \
    integration_test/subtitle_contract_test.dart
  run_flutter_integration "$SELECTED_SIMULATOR_ID" simulator subtitle-lifecycle \
    integration_test/subtitle_lifecycle_test.dart
}

run_ios_device() {
  local result_bundle="$XCRESULT_DIR/ios-device.xcresult"
  local derived_data="$RUN_TMP/ios-device-derived"
  local exported_attachments="$ATTACHMENTS_DIR/ios-device"

  vesper_require_command xcodebuild
  vesper_require_command xcrun
  vesper_require_command flutter
  vesper_require_command security

  prepare_ios_projects
  prepare_flutter_dependencies
  prepare_ios_device
  run_logged ios-device-ffi "$ROOT_DIR" \
    env VESPER_BUILD_IOS_PLAYER_FFI_MODE=platform PLATFORM_NAME=iphoneos \
      "$ROOT_DIR/scripts/vesper" ios ffi debug

  run_logged ios-device-xctest "$ROOT_DIR" \
    xcodebuild test \
      -project examples/ios-swift-host/VesperPlayerHostDemo.xcodeproj \
      -scheme VesperPlayerHostDemo \
      -configuration Debug \
      -destination "platform=iOS,id=$DEVICE_ID" \
      -resultBundlePath "$result_bundle" \
      -derivedDataPath "$derived_data" \
      DEVELOPMENT_TEAM="$IOS_DEVELOPMENT_TEAM" \
      CODE_SIGN_STYLE=Automatic \
      -allowProvisioningUpdates \
      -only-testing:VesperPlayerHostDemoTests/VesperSubtitleDeviceAcceptanceTests
  run_logged ios-device-xcresult-evidence "$ROOT_DIR" \
    verify_xcresult_tests "$result_bundle" \
      "$XCRESULT_DIR/ios-device-summary.json" \
      "$XCRESULT_DIR/ios-device-tests.json" \
      VesperSubtitleDeviceAcceptanceTests=3

  run_logged ios-device-xctest-attachments "$ROOT_DIR" \
    xcrun xcresulttool export attachments \
      --path "$result_bundle" \
      --output-path "$exported_attachments"
  run_logged ios-device-xctest-attachment-evidence "$ROOT_DIR" \
    verify_ios_device_attachments "$exported_attachments"

  run_flutter_integration "$DEVICE_ID" device subtitle-positive \
    integration_test/subtitle_contract_test.dart
  run_flutter_integration "$DEVICE_ID" device subtitle-lifecycle \
    integration_test/subtitle_lifecycle_test.dart
}

prepare_android_device() {
  local flutter_devices_json="$PREFLIGHT_DIR/flutter-devices-device.json"
  local selected_device_json="$PREFLIGHT_DIR/selected-device.json"
  local adb_properties="$PREFLIGHT_DIR/adb-device-properties.txt"

  vesper_require_command adb "adb is required for Android device subtitle verification."
  capture_output android-flutter-device-list "$flutter_devices_json" \
    "$ROOT_DIR/examples/flutter-host" flutter devices --machine
  run_logged android-flutter-device-match "$ROOT_DIR" \
    ruby -rjson -e '
    devices = JSON.parse(File.read(ARGV.fetch(0)))
    id = ARGV.fetch(1)
    match = devices.find do |device|
      device["id"] == id && device["targetPlatform"].to_s.start_with?("android") &&
        device["isSupported"] == true && device["emulator"] == false
    end
    abort("Flutter does not expose the requested physical Android device: #{id}") unless match
    File.write(ARGV.fetch(2), JSON.pretty_generate({
      "id" => id,
      "name" => match["name"],
      "targetPlatform" => match["targetPlatform"],
      "emulator" => match["emulator"],
      "sdk" => match["sdk"]
    }) + "\n")
    ' "$flutter_devices_json" "$DEVICE_ID" "$selected_device_json"
  SELECTED_DEVICE_SUMMARY="$selected_device_json"

  run_logged android-adb-state "$ROOT_DIR" adb -s "$DEVICE_ID" get-state
  capture_output android-adb-properties "$adb_properties" "$ROOT_DIR" \
    adb -s "$DEVICE_ID" shell getprop
  run_logged android-adb-arm64-abi "$ROOT_DIR" \
    ruby -e '
      properties = File.read(ARGV.fetch(0))
      abort("The Android subtitle gate requires an arm64-v8a device.") unless
        properties.match?(/^\[ro\.product\.cpu\.abi\]: \[arm64-v8a\]$/)
    ' "$adb_properties"
}

run_android_regression() {
  local gradle_path

  vesper_require_command cargo
  vesper_require_command flutter
  source "$ROOT_DIR/scripts/lib/android.sh"
  gradle_path="$(vesper_android_resolve_gradle "$ROOT_DIR/lib/android")"

  run_common_contract_regression
  run_logged android-rust-subtitle-tests "$ROOT_DIR" \
    cargo test -p player-ffi -p player-platform-android -p player-jni-android
  run_logged android-host-subtitle-tests "$ROOT_DIR" \
    env GRADLE_USER_HOME="$ROOT_DIR/lib/android/.gradle/gradle-user-home" \
      "$gradle_path" -p "$ROOT_DIR/lib/android" \
        -Pvesper.player.android.abis=arm64-v8a test
  run_logged flutter-platform-subtitle-tests \
    "$ROOT_DIR/lib/flutter/vesper_player_platform_interface" \
    flutter test test/subtitle_exception_test.dart test/subtitle_state_models_test.dart
  run_logged flutter-controller-subtitle-tests "$ROOT_DIR/lib/flutter/vesper_player" \
    flutter test test/vesper_download_manager_test.dart
  run_logged flutter-android-channel-tests "$ROOT_DIR/lib/flutter/vesper_player_android" \
    flutter test test/method_channel_vesper_player_android_test.dart
}

run_android_device() {
  prepare_flutter_dependencies
  prepare_android_device
  run_flutter_integration "$DEVICE_ID" device subtitle-positive \
    integration_test/subtitle_contract_test.dart
  run_flutter_integration "$DEVICE_ID" device subtitle-lifecycle \
    integration_test/subtitle_lifecycle_test.dart
}

run_logged toolchain "$ROOT_DIR" collect_toolchain

case "$PLATFORM:$SCOPE" in
  ios:regression)
    run_ios_regression
    ;;
  ios:device)
    run_ios_device
    ;;
  ios:complete)
    run_ios_regression
    run_ios_device
    ;;
  android:regression)
    run_android_regression
    ;;
  android:device)
    run_android_device
    ;;
  android:complete)
    run_android_regression
    run_android_device
    ;;
esac
