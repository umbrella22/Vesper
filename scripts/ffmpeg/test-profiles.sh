#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$SCRIPT_DIR/lib/common.sh"
source "$SCRIPT_DIR/lib/ffmpeg.sh"
source "$SCRIPT_DIR/lib/ffmpeg-profile.sh"
source "$SCRIPT_DIR/lib/ffmpeg-validate.sh"

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

metadata_value() {
  local metadata="$1"
  local key="$2"

  awk -F= -v expected="$key" '$1 == expected { print substr($0, index($0, "=") + 1); exit }' <<<"$metadata"
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

metadata_profile_args=()
vesper_ffmpeg_profile_resolve default ios "$VESPER_REPO_ROOT/scripts/ffmpeg-profiles.toml"
while IFS= read -r arg; do
  metadata_profile_args+=("$arg")
done < <(vesper_ffmpeg_profile_emit_legacy_args)
vesper_ffmpeg_parse_common_args apple "${metadata_profile_args[@]}"

metadata_source="$tmp_dir/ffmpeg-source.tar.xz"
printf 'first source payload' >"$metadata_source"
first_metadata="$(vesper_ffmpeg_metadata_text \
  apple \
  ios-arm64 \
  8.1.2 \
  "$metadata_source" \
  https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz \
  ./configure \
  --enable-shared)"
first_source_sha256="$(metadata_value "$first_metadata" source_sha256)"
assert_eq \
  "Vesper FFmpeg build metadata v2" \
  "$(printf '%s\n' "$first_metadata" | sed -n '1p')" \
  "build metadata schema includes source provenance"
assert_eq \
  "$(vesper_ffmpeg_sha256_file "$metadata_source")" \
  "$first_source_sha256" \
  "build metadata records the source archive SHA-256"

printf 'second source payload' >"$metadata_source"
second_metadata="$(vesper_ffmpeg_metadata_text \
  apple \
  ios-arm64 \
  8.1.2 \
  "$metadata_source" \
  https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz \
  ./configure \
  --enable-shared)"
second_source_sha256="$(metadata_value "$second_metadata" source_sha256)"
if [[ "$first_source_sha256" == "$second_source_sha256" ]]; then
  fail "build metadata source SHA-256 did not change with the source archive"
fi

lgpl_shared_metadata="$tmp_dir/lgpl-shared-metadata.txt"
printf '%s\n' "$first_metadata" >"$lgpl_shared_metadata"
vesper_ffmpeg_validate_lgpl_shared_metadata_file "$lgpl_shared_metadata"

gpl_license_metadata="$tmp_dir/gpl-license-metadata.txt"
sed 's/^license_flags=$/license_flags=gpl/' "$lgpl_shared_metadata" >"$gpl_license_metadata"
if vesper_ffmpeg_validate_lgpl_shared_metadata_file "$gpl_license_metadata" >/dev/null 2>&1; then
  fail "GPL license metadata unexpectedly passed LGPL release validation"
fi

gpl_configure_metadata="$tmp_dir/gpl-configure-metadata.txt"
sed 's/^configure_line=/configure_line=--enable-gpl /' "$lgpl_shared_metadata" >"$gpl_configure_metadata"
if vesper_ffmpeg_validate_lgpl_shared_metadata_file "$gpl_configure_metadata" >/dev/null 2>&1; then
  fail "--enable-gpl metadata unexpectedly passed LGPL release validation"
fi

nonfree_configure_metadata="$tmp_dir/nonfree-configure-metadata.txt"
sed 's/^configure_line=/configure_line=--enable-nonfree /' "$lgpl_shared_metadata" >"$nonfree_configure_metadata"
if vesper_ffmpeg_validate_lgpl_shared_metadata_file "$nonfree_configure_metadata" >/dev/null 2>&1; then
  fail "--enable-nonfree metadata unexpectedly passed LGPL release validation"
fi

static_metadata="$tmp_dir/static-metadata.txt"
sed 's/--enable-shared/--disable-shared/' "$lgpl_shared_metadata" >"$static_metadata"
if vesper_ffmpeg_validate_lgpl_shared_metadata_file "$static_metadata" >/dev/null 2>&1; then
  fail "metadata without --enable-shared unexpectedly passed release validation"
fi

if vesper_ffmpeg_validate_metadata_tree "$tmp_dir/missing-metadata" >/dev/null 2>&1; then
  fail "missing metadata directory unexpectedly passed validation"
fi
empty_metadata_dir="$tmp_dir/empty-metadata"
mkdir -p "$empty_metadata_dir"
if vesper_ffmpeg_validate_metadata_tree "$empty_metadata_dir" >/dev/null 2>&1; then
  fail "empty metadata directory unexpectedly passed validation"
fi
valid_metadata_dir="$tmp_dir/valid-metadata"
mkdir -p "$valid_metadata_dir"
cp "$lgpl_shared_metadata" "$valid_metadata_dir/vesper-ffmpeg-build-metadata.txt"
vesper_ffmpeg_validate_metadata_tree "$valid_metadata_dir"

invalid_profile="$tmp_dir/invalid-profile.toml"
printf '%s\n' '[profile.invalid' >"$invalid_profile"
if vesper_ffmpeg_profile_resolve invalid ios "$invalid_profile" >/dev/null 2>&1; then
  fail "invalid TOML profile unexpectedly passed parsing"
fi

cache_dir="$tmp_dir/cache"
mkdir -p "$cache_dir"
touch \
  "$cache_dir/ffmpeg-8.1.tar.xz" \
  "$cache_dir/ffmpeg-8.1.2.tar.xz" \
  "$cache_dir/ffmpeg-8.1.10.tar.xz" \
  "$cache_dir/ffmpeg-8.2.1.tar.xz" \
  "$cache_dir/openssl-3.5.1.tar.gz" \
  "$cache_dir/openssl-3.5.7.tar.gz" \
  "$cache_dir/openssl-4.0.1.tar.gz"

VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" assert_eq \
  "8.1.10" \
  "$(VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" vesper_ffmpeg_resolve_version android)" \
  "FFmpeg resolver selects highest cached patch in the default series"
VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" VESPER_ANDROID_FFMPEG_SERIES=8.2 assert_eq \
  "8.2.1" \
  "$(VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" VESPER_ANDROID_FFMPEG_SERIES=8.2 vesper_ffmpeg_resolve_version android)" \
  "FFmpeg resolver honors platform series override"
VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" VESPER_ANDROID_FFMPEG_VERSION=8.1.2 assert_eq \
  "8.1.2" \
  "$(VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" VESPER_ANDROID_FFMPEG_VERSION=8.1.2 vesper_ffmpeg_resolve_version android)" \
  "FFmpeg resolver honors exact platform version override"
VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" VESPER_APPLE_FFMPEG_VERSION=8.1.2 assert_eq \
  "8.1.2" \
  "$(VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" VESPER_APPLE_FFMPEG_VERSION=8.1.2 vesper_ffmpeg_resolve_version apple)" \
  "FFmpeg resolver pins the exact Apple release version"
VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" assert_eq \
  "3.5.7" \
  "$(VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" vesper_openssl_resolve_version android)" \
  "OpenSSL resolver selects highest cached patch in the LTS series"
VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" VESPER_ANDROID_OPENSSL_SERIES=4.0 assert_eq \
  "4.0.1" \
  "$(VESPER_THIRD_PARTY_SOURCE_CACHE_DIR="$cache_dir" VESPER_ANDROID_OPENSSL_SERIES=4.0 vesper_openssl_resolve_version android)" \
  "OpenSSL resolver honors platform series override"

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
extra_configure_args = ["--extra-cflags=-DVESPER_LABEL=hello world", "--extra-ldflags=-Wl,-rpath,/opt/vesper sdk"]

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
assert_eq "2" "${#VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS[@]}" "configure argument count is preserved"
assert_eq "--extra-cflags=-DVESPER_LABEL=hello world" "${VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS[0]}" "configure argument spaces are preserved"
assert_eq "--extra-ldflags=-Wl,-rpath,/opt/vesper sdk" "${VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS[1]}" "configure argument commas are preserved"

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
