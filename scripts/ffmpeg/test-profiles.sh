#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$SCRIPT_DIR/lib/common.sh"
source "$SCRIPT_DIR/lib/ffmpeg.sh"
source "$SCRIPT_DIR/lib/ffmpeg-profile.sh"

fail() {
  echo "ffmpeg profile test failed: $*" >&2
  exit 1
}

assert_eq() {
  local expected="$1"
  local actual="$2"
  local label="$3"

  [[ "$expected" == "$actual" ]] || fail "$label: expected '$expected', got '$actual'"
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  local label="$3"

  [[ ",$haystack," == *",$needle,"* ]] || fail "$label: missing '$needle' in '$haystack'"
}

profile_csv() {
  local target="$1"
  eval "vesper_ffmpeg_profile_join_csv \"\${${target}[@]}\""
}

profile_hash_for_default_android() {
  local args=()
  local arg

  vesper_ffmpeg_profile_resolve default android "$VESPER_REPO_ROOT/scripts/ffmpeg-profiles.toml"
  while IFS= read -r arg; do
    args+=("$arg")
  done < <(vesper_ffmpeg_profile_emit_legacy_args)
  vesper_ffmpeg_parse_common_args android "${args[@]}"
  vesper_ffmpeg_profile_key android
}

vesper_ffmpeg_profile_resolve default android
assert_eq "avcodec,avformat,avutil" "$(profile_csv VESPER_PROFILE_RESOLVED_LIBRARIES)" "default libraries are deduplicated"
assert_eq "file,pipe" "$(profile_csv VESPER_PROFILE_RESOLVED_PROTOCOLS)" "default protocols stay local"
assert_contains "$(profile_csv VESPER_PROFILE_RESOLVED_DEMUXERS)" "dash" "default merges download remux demuxers"
assert_contains "$(profile_csv VESPER_PROFILE_RESOLVED_MUXERS)" "hls" "default merges relay remux muxers"
assert_eq "true" "$VESPER_PROFILE_VALIDATION_FORBID_NETWORK" "default forbids network"
assert_eq "true" "$VESPER_PROFILE_VALIDATION_FORBID_OPENSSL" "default forbids OpenSSL"

first_hash="$(profile_hash_for_default_android)"
second_hash="$(profile_hash_for_default_android)"
assert_eq "$first_hash" "$second_hash" "default Android profile hash is stable"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/vesper-ffmpeg-profile-tests.XXXXXX")"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

temp_config="$tmp_dir/ffmpeg-profiles.toml"
cat >"$temp_config" <<'EOF'
[profile.base]
libraries = ["avcodec"]
protocols = ["file"]

[profile.extra]
extends = "base"
libraries = ["avformat", "avcodec"]
protocols = ["pipe"]

[profile.multi]
extends = ["base", "extra"]
libraries = ["avutil", "avformat"]

[profile.multi.platform_overrides.ios]
demuxers = ["mov"]
protocols = ["data"]
EOF

vesper_ffmpeg_profile_resolve multi android "$temp_config"
assert_eq "avcodec,avformat,avutil" "$(profile_csv VESPER_PROFILE_RESOLVED_LIBRARIES)" "multi inheritance deduplicates libraries"
assert_eq "file,pipe" "$(profile_csv VESPER_PROFILE_RESOLVED_PROTOCOLS)" "android ignores ios override"

vesper_ffmpeg_profile_resolve multi ios "$temp_config"
assert_eq "mov" "$(profile_csv VESPER_PROFILE_RESOLVED_DEMUXERS)" "ios platform override adds demuxer"
assert_eq "file,pipe,data" "$(profile_csv VESPER_PROFILE_RESOLVED_PROTOCOLS)" "ios platform override adds protocol"

if "$VESPER_REPO_ROOT/scripts/vesper" ffmpeg \
  --platform android \
  --profile default \
  --dry-run \
  --extra-protocols http >/dev/null 2>"$tmp_dir/validation-error.txt"; then
  fail "network protocol overlay unexpectedly passed validation"
fi
grep -q "forbids network" "$tmp_dir/validation-error.txt" || fail "validation conflict did not report network policy"

if "$VESPER_REPO_ROOT/scripts/vesper" ffmpeg \
  --platform android \
  --profile default \
  --dry-run \
  --extra-configure-arg --enable-openssl >/dev/null 2>"$tmp_dir/openssl-validation-error.txt"; then
  fail "OpenSSL configure overlay unexpectedly passed validation"
fi
grep -q "forbids OpenSSL" "$tmp_dir/openssl-validation-error.txt" || fail "validation conflict did not report OpenSSL policy"

echo "FFmpeg profile tests passed."
