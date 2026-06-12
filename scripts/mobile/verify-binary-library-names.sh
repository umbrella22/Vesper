#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
failures=0

record_failure() {
  echo "$1" >&2
  failures=$((failures + 1))
}

verify_rust_binary_lib_names() {
  local manifest
  while IFS= read -r manifest; do
    local result
    result="$(
      awk '
        function finish_section() {
          if (in_lib && has_binary) {
            if (!has_name) {
              print "missing explicit [lib] name"
            } else if (lib_name !~ /^vesper_/) {
              print "binary [lib] name must start with vesper_: " lib_name
            }
          }
        }
        /^\[lib\]$/ {
          finish_section()
          in_lib = 1
          has_name = 0
          has_binary = 0
          lib_name = ""
          next
        }
        /^\[/ {
          finish_section()
          in_lib = 0
        }
        in_lib && /^name = "/ {
          has_name = 1
          lib_name = $0
          sub(/^name = "/, "", lib_name)
          sub(/".*/, "", lib_name)
        }
        in_lib && /crate-type/ && /(cdylib|staticlib)/ {
          has_binary = 1
        }
        END {
          finish_section()
        }
      ' "$manifest"
    )"
    if [[ -n "$result" ]]; then
      record_failure "$manifest: $result"
    fi
  done < <(find "$ROOT_DIR/crates" -name Cargo.toml -type f | sort)
}

verify_mobile_distribution_references() {
  local matches
  matches="$(
    rg -n \
      --glob '!**/build/**' \
      --glob '!**/.build/**' \
      --glob '!**/.gradle/**' \
      --glob '!**/.dart_tool/**' \
      --glob '!**/target/**' \
      'libplayer_[A-Za-z0-9_./$(){}-]*\.(so|dylib|a)|-lplayer_[A-Za-z0-9_]+' \
      "$ROOT_DIR/lib/android" \
      "$ROOT_DIR/lib/ios" \
      "$ROOT_DIR/lib/flutter" \
      "$ROOT_DIR/examples/android-compose-host" \
      "$ROOT_DIR/examples/ios-swift-host" \
      "$ROOT_DIR/examples/flutter-host" \
      "$ROOT_DIR/scripts/android" \
      "$ROOT_DIR/scripts/ios" \
      "$ROOT_DIR/scripts/mobile" \
      "$ROOT_DIR/scripts/ffi" \
      "$ROOT_DIR/README.md" \
      "$ROOT_DIR/README.zh-CN.md" || true
  )"

  if [[ -n "$matches" ]]; then
    record_failure "Found mobile distribution references to libplayer_* binaries:"
    printf '%s\n' "$matches" >&2
  fi
}

verify_mobile_distribution_files() {
  local matches
  matches="$(
    find \
      "$ROOT_DIR/lib/android" \
      "$ROOT_DIR/lib/ios" \
      "$ROOT_DIR/lib/flutter" \
      "$ROOT_DIR/examples/android-compose-host" \
      "$ROOT_DIR/examples/ios-swift-host" \
      "$ROOT_DIR/examples/flutter-host" \
      \( \
        -path '*/build' -o \
        -path '*/.build' -o \
        -path '*/.gradle' -o \
        -path '*/.dart_tool' -o \
        -path '*/target' \
      \) -prune -o \
      -type f \
      \( -name 'libplayer_*.so' -o -name 'libplayer_*.dylib' -o -name 'libplayer_*.a' \) \
      -print | sort
  )"

  if [[ -n "$matches" ]]; then
    record_failure "Found mobile distribution binary files using libplayer_* names:"
    printf '%s\n' "$matches" >&2
  fi
}

verify_rust_binary_lib_names
verify_mobile_distribution_references
verify_mobile_distribution_files

if [[ "$failures" -ne 0 ]]; then
  echo "Binary library naming verification failed with $failures issue(s)." >&2
  exit 1
fi

echo "Verified Rust and mobile distribution binary library names use libvesper_* outputs."
