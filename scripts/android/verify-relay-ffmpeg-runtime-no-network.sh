#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
RUNTIME_MODULE_DIR="$ROOT_DIR/lib/android/vesper-player-kit-ffmpeg-runtime"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vesper-relay-ffmpeg-runtime-verify.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

consumers=("$@")
if [[ ${#consumers[@]} -eq 0 ]]; then
  consumers=(relay-remux)
fi

metadata="$("$ROOT_DIR/scripts/android/resolve-ffmpeg-runtime-requirements.sh" --print-metadata "${consumers[@]}")"
protocols="$(printf '%s\n' "$metadata" | awk -F= '$1 == "protocols" {print $2}')"
external_dependencies="$(printf '%s\n' "$metadata" | awk -F= '$1 == "external_dependencies" {print $2}')"
license_flags="$(printf '%s\n' "$metadata" | awk -F= '$1 == "license_flags" {print $2}')"

contains_csv_token() {
  local csv="$1"
  local needle="$2"
  local tokenized="${csv//,/ }"
  local token
  for token in $tokenized; do
    [[ "$token" == "$needle" ]] && return 0
  done
  return 1
}

for forbidden_protocol in http https tcp tls rtmp rtmps rtmpt rtmpts; do
  if contains_csv_token "$protocols" "$forbidden_protocol"; then
    echo "relay-remux FFmpeg runtime must not enable network protocol: $forbidden_protocol" >&2
    echo "$metadata" >&2
    exit 1
  fi
done

if contains_csv_token "$external_dependencies" openssl || contains_csv_token "$license_flags" openssl; then
  echo "relay-remux FFmpeg runtime must not depend on OpenSSL." >&2
  echo "$metadata" >&2
  exit 1
fi

unexpected_crypto="$(
  find "$RUNTIME_MODULE_DIR/src/main" "$RUNTIME_MODULE_DIR/build/outputs/aar" -type f \
    \( -name 'libssl*.so' -o -name 'libcrypto*.so' \) \
    -print -quit 2>/dev/null || true
)"
if [[ -n "$unexpected_crypto" ]]; then
  echo "relay-remux FFmpeg runtime packaged OpenSSL payload:" >&2
  echo "  $unexpected_crypto" >&2
  exit 1
fi

while IFS= read -r metadata_file; do
  if ! grep -q -- '--disable-network' "$metadata_file"; then
    echo "FFmpeg metadata does not include --disable-network: $metadata_file" >&2
    exit 1
  fi
  if ! grep -q -- '--disable-openssl' "$metadata_file"; then
    echo "FFmpeg metadata does not include --disable-openssl: $metadata_file" >&2
    exit 1
  fi
  if grep -Eq -- '--enable-network|--enable-openssl|protocols=.*(http|https|tcp|tls)' "$metadata_file"; then
    echo "FFmpeg metadata includes forbidden network/OpenSSL capability: $metadata_file" >&2
    exit 1
  fi
done < <(find "$RUNTIME_MODULE_DIR/src/main/assets" -type f -name '*metadata.txt' 2>/dev/null | sort)

while IFS= read -r aar_path; do
  unpack_dir="$TMP_DIR/$(basename "$aar_path" .aar)"
  mkdir -p "$unpack_dir"
  unzip -q "$aar_path" -d "$unpack_dir"
  unexpected_crypto="$(
    find "$unpack_dir" -type f \( -name 'libssl*.so' -o -name 'libcrypto*.so' \) -print -quit
  )"
  if [[ -n "$unexpected_crypto" ]]; then
    echo "relay-remux FFmpeg runtime AAR contains OpenSSL payload:" >&2
    echo "  $unexpected_crypto" >&2
    exit 1
  fi
done < <(find "$RUNTIME_MODULE_DIR/build/outputs/aar" -type f -name '*.aar' 2>/dev/null | sort)

echo "Verified relay-remux FFmpeg runtime profile is no-network/no-OpenSSL for consumers: ${consumers[*]}"
