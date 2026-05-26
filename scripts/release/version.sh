#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"

usage() {
  cat <<EOF >&2
Usage:
  scripts/release/version.sh set <version> [--ios-build <build>] [--android-version-code <code>] --date <YYYY-MM-DD>
  scripts/release/version.sh prepare-from-tag <tag> [--ios-build <build>] [--android-version-code <code>] [--date <YYYY-MM-DD>]
  scripts/release/version.sh metadata-from-tag <tag> [--ios-build <build>] [--android-version-code <code>] [--date <YYYY-MM-DD>]
  scripts/release/version.sh verify <version> [--ios-build <build>] [--android-version-code <code>]
  scripts/release/version.sh verify-current
EOF
}

require_arg() {
  local flag="$1"
  local value="${2:-}"

  if [[ -z "$value" ]]; then
    echo "$flag requires a value." >&2
    exit 1
  fi
}

version_to_android_code() {
  local version="$1"
  local major minor patch

  IFS=. read -r major minor patch <<<"$version"
  if [[ ! "$major" =~ ^[0-9]+$ || ! "$minor" =~ ^[0-9]+$ || ! "$patch" =~ ^[0-9]+$ ]]; then
    echo "Version must be numeric major.minor.patch for Android versionCode inference: $version" >&2
    exit 1
  fi

  # Preserve the existing pre-1.0 release-number convention: 0.3.0 -> 3.
  if [[ "$major" -eq 0 && "$patch" -eq 0 ]]; then
    printf '%d\n' "$minor"
    return 0
  fi

  printf '%d\n' "$((major * 10000 + minor * 100 + patch))"
}

version_from_tag() {
  local tag="$1"

  tag="${tag#refs/tags/}"
  tag="${tag#v}"

  if [[ "$tag" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)([-+].*)?$ ]]; then
    printf '%s.%s.%s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}" "${BASH_REMATCH[3]}"
    return 0
  fi

  echo "Release tag must look like vMAJOR.MINOR.PATCH or vMAJOR.MINOR.PATCH-rc.N: $1" >&2
  exit 1
}

release_date_from_tag() {
  local tag="$1"
  local ref_name
  local tagger_date
  local commit_date

  ref_name="${tag#refs/tags/}"

  tagger_date="$(git for-each-ref "refs/tags/$ref_name" --format='%(taggerdate:short)' 2>/dev/null || true)"
  if [[ "$tagger_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
    printf '%s\n' "$tagger_date"
    return 0
  fi

  commit_date="$(git log -1 --format=%cd --date=format:%Y-%m-%d "$tag^{commit}" 2>/dev/null || true)"
  if [[ "$commit_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
    printf '%s\n' "$commit_date"
    return 0
  fi

  date -u +%F
}

has_release_date_arg() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --date|--date=*)
        return 0
        ;;
    esac
    shift
  done

  return 1
}

read_workspace_version() {
  awk '
    /^\[workspace\.package\]$/ { in_section = 1; next }
    /^\[/ { in_section = 0 }
    in_section && /^version = "/ {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "$ROOT_DIR/Cargo.toml"
}

read_ios_build() {
  sed -n 's/^[[:space:]]*CFBundleVersion: "\([0-9][0-9]*\)".*/\1/p' \
    "$ROOT_DIR/lib/ios/VesperPlayerKit/project.yml" \
    | head -n 1
}

read_android_version_code() {
  sed -n 's/^[[:space:]]*versionCode = \([0-9][0-9]*\).*/\1/p' \
    "$ROOT_DIR/examples/android-compose-host/app/build.gradle.kts" \
    | head -n 1
}

replace_in_file() {
  local file="$1"
  local pattern="$2"
  local replacement="$3"

  perl -0pi -e "s{$pattern}{$replacement}g" "$file"
}

update_cargo_lock_versions() {
  local version="$1"

  [[ -f "$ROOT_DIR/Cargo.lock" ]] || return 0

  perl -0pi -e 's{(\[\[package\]\]\nname = "(?:basic-player|player-[^"]+)"\nversion = ")[^"]+"}{${1}'"$version"'"}g' \
    "$ROOT_DIR/Cargo.lock"
}

update_changelog_heading() {
  local changelog="$1"
  local version="$2"
  local release_date="$3"

  [[ -f "$changelog" ]] || return 0

  if grep -q "^## $version - " "$changelog"; then
    perl -0pi -e "s{^## \Q$version\E - (?:Unreleased|[0-9]{4}-[0-9]{2}-[0-9]{2})}{## $version - $release_date}m" \
      "$changelog"
    return 0
  fi

  if grep -Eq '^## [0-9]+\.[0-9]+\.[0-9]+ - Unreleased$' "$changelog"; then
    perl -0pi -e "s{^## [0-9]+\.[0-9]+\.[0-9]+ - Unreleased$}{## $version - $release_date}m" \
      "$changelog"
    return 0
  fi

  perl -0pi -e "s{# Changelog\n\n}{# Changelog\n\n## $version - $release_date\n\n- Prepared package metadata for the $version release.\n\n}" \
    "$changelog"
}

resolve_release_metadata() {
  local version="$1"
  shift

  RESOLVED_VERSION="$version"
  RESOLVED_IOS_BUILD=""
  RESOLVED_ANDROID_VERSION_CODE=""
  RESOLVED_RELEASE_DATE=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --ios-build)
        require_arg "$1" "${2:-}"
        RESOLVED_IOS_BUILD="$2"
        shift 2
        ;;
      --ios-build=*)
        RESOLVED_IOS_BUILD="${1#*=}"
        shift
        ;;
      --android-version-code)
        require_arg "$1" "${2:-}"
        RESOLVED_ANDROID_VERSION_CODE="$2"
        shift 2
        ;;
      --android-version-code=*)
        RESOLVED_ANDROID_VERSION_CODE="${1#*=}"
        shift
        ;;
      --date)
        require_arg "$1" "${2:-}"
        RESOLVED_RELEASE_DATE="$2"
        shift 2
        ;;
      --date=*)
        RESOLVED_RELEASE_DATE="${1#*=}"
        shift
        ;;
      *)
        echo "Unexpected release metadata argument: $1" >&2
        usage
        exit 1
        ;;
    esac
  done

  [[ "$RESOLVED_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Version must be numeric major.minor.patch: $RESOLVED_VERSION" >&2; exit 1; }

  RESOLVED_ANDROID_VERSION_CODE="${RESOLVED_ANDROID_VERSION_CODE:-${VESPER_RELEASE_ANDROID_VERSION_CODE:-}}"
  RESOLVED_IOS_BUILD="${RESOLVED_IOS_BUILD:-${VESPER_RELEASE_IOS_BUILD:-${VESPER_RELEASE_BUILD:-}}}"
  RESOLVED_ANDROID_VERSION_CODE="${RESOLVED_ANDROID_VERSION_CODE:-$(version_to_android_code "$RESOLVED_VERSION")}"
  RESOLVED_IOS_BUILD="${RESOLVED_IOS_BUILD:-$RESOLVED_ANDROID_VERSION_CODE}"
  RESOLVED_RELEASE_DATE="${RESOLVED_RELEASE_DATE:-$(date -u +%F)}"

  [[ "$RESOLVED_IOS_BUILD" =~ ^[0-9]+$ ]] || { echo "iOS build must be numeric: $RESOLVED_IOS_BUILD" >&2; exit 1; }
  [[ "$RESOLVED_ANDROID_VERSION_CODE" =~ ^[0-9]+$ ]] || { echo "Android versionCode must be numeric: $RESOLVED_ANDROID_VERSION_CODE" >&2; exit 1; }
  [[ "$RESOLVED_RELEASE_DATE" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || { echo "Release date must be YYYY-MM-DD: $RESOLVED_RELEASE_DATE" >&2; exit 1; }
}

emit_release_metadata() {
  echo "version=$RESOLVED_VERSION"
  echo "ios_build=$RESOLVED_IOS_BUILD"
  echo "android_version_code=$RESOLVED_ANDROID_VERSION_CODE"
  echo "release_date=$RESOLVED_RELEASE_DATE"

  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    {
      echo "version=$RESOLVED_VERSION"
      echo "ios_build=$RESOLVED_IOS_BUILD"
      echo "android_version_code=$RESOLVED_ANDROID_VERSION_CODE"
      echo "release_date=$RESOLVED_RELEASE_DATE"
    } >>"$GITHUB_OUTPUT"
  fi

  if [[ -n "${GITHUB_ENV:-}" ]]; then
    {
      echo "VESPER_RELEASE_VERSION=$RESOLVED_VERSION"
      echo "VESPER_RELEASE_BUILD=$RESOLVED_IOS_BUILD"
      echo "VESPER_RELEASE_IOS_BUILD=$RESOLVED_IOS_BUILD"
      echo "VESPER_RELEASE_ANDROID_VERSION_CODE=$RESOLVED_ANDROID_VERSION_CODE"
      echo "VESPER_RELEASE_DATE=$RESOLVED_RELEASE_DATE"
    } >>"$GITHUB_ENV"
  fi
}

set_version() {
  local version="$1"
  shift

  resolve_release_metadata "$version" "$@"
  version="$RESOLVED_VERSION"
  local ios_build="$RESOLVED_IOS_BUILD"
  local android_version_code="$RESOLVED_ANDROID_VERSION_CODE"
  local release_date="$RESOLVED_RELEASE_DATE"

  replace_in_file "$ROOT_DIR/Cargo.toml" '(\[workspace\.package\][^\[]*?\nversion = ")[0-9]+\.[0-9]+\.[0-9]+"' "\${1}$version\""
  update_cargo_lock_versions "$version"
  replace_in_file "$ROOT_DIR/lib/android/build.gradle.kts" 'version = "[0-9]+\.[0-9]+\.[0-9]+"' "version = \"$version\""

  for pubspec in "$ROOT_DIR"/lib/flutter/*/pubspec.yaml; do
    perl -0pi -e "s{^version: [0-9]+\\.[0-9]+\\.[0-9]+}{version: $version}m" "$pubspec"
  done

  for pubspec in "$ROOT_DIR"/lib/flutter/*/pubspec.yaml; do
    for package in \
      vesper_player \
      vesper_player_android \
      vesper_player_external_playback \
      vesper_player_ios \
      vesper_player_macos \
      vesper_player_platform_interface \
      vesper_player_ui
    do
      perl -0pi -e "s{^  $package: \^[0-9]+\.[0-9]+\.[0-9]+(?:[+-][A-Za-z0-9.-]+)?\n}{  $package: ^$version\n}mg" "$pubspec"
    done
  done

  for gradle_file in \
    "$ROOT_DIR/lib/flutter/vesper_player_android/android/build.gradle" \
    "$ROOT_DIR/lib/flutter/vesper_player_external_playback/android/build.gradle"
  do
    perl -0pi -e "s{^version = \"[0-9]+\\.[0-9]+\\.[0-9]+\"}{version = \"$version\"}m" "$gradle_file"
  done

  replace_in_file "$ROOT_DIR/lib/ios/VesperPlayerKit/project.yml" 'CFBundleShortVersionString: "[0-9]+\.[0-9]+\.[0-9]+"' "CFBundleShortVersionString: \"$version\""
  replace_in_file "$ROOT_DIR/lib/ios/VesperPlayerKit/project.yml" 'CFBundleVersion: "[0-9]+"' "CFBundleVersion: \"$ios_build\""
  replace_in_file "$ROOT_DIR/lib/ios/VesperPlayerKit/Sources/Generated-Info.plist" '<key>CFBundleShortVersionString</key>\n\t<string>[0-9]+\.[0-9]+\.[0-9]+</string>' "<key>CFBundleShortVersionString</key>\n\t<string>$version</string>"
  replace_in_file "$ROOT_DIR/lib/ios/VesperPlayerKit/Sources/Generated-Info.plist" '<key>CFBundleVersion</key>\n\t<string>[0-9]+</string>' "<key>CFBundleVersion</key>\n\t<string>$ios_build</string>"

  replace_in_file "$ROOT_DIR/examples/android-compose-host/app/build.gradle.kts" 'versionCode = [0-9]+' "versionCode = $android_version_code"
  replace_in_file "$ROOT_DIR/examples/android-compose-host/app/build.gradle.kts" 'versionName = "[0-9]+\.[0-9]+\.[0-9]+"' "versionName = \"$version\""
  replace_in_file "$ROOT_DIR/examples/flutter-host/pubspec.yaml" 'version: [0-9]+\.[0-9]+\.[0-9]\+[0-9]+' "version: $version+$android_version_code"

  for changelog in \
    "$ROOT_DIR/CHANGELOG.md" \
    "$ROOT_DIR/lib/android/CHANGELOG.md" \
    "$ROOT_DIR/lib/ios/VesperPlayerKit/CHANGELOG.md" \
    "$ROOT_DIR/lib/flutter/vesper_player/CHANGELOG.md" \
    "$ROOT_DIR/lib/flutter/vesper_player_android/CHANGELOG.md" \
    "$ROOT_DIR/lib/flutter/vesper_player_external_playback/CHANGELOG.md"
  do
    update_changelog_heading "$changelog" "$version" "$release_date"
  done

  for changelog in \
    "$ROOT_DIR/lib/flutter/vesper_player_platform_interface/CHANGELOG.md" \
    "$ROOT_DIR/lib/flutter/vesper_player_ios/CHANGELOG.md" \
    "$ROOT_DIR/lib/flutter/vesper_player_macos/CHANGELOG.md" \
    "$ROOT_DIR/lib/flutter/vesper_player_ui/CHANGELOG.md"
  do
    update_changelog_heading "$changelog" "$version" "$release_date"
  done

  echo "Updated Vesper product version to $version."
}

prepare_from_tag() {
  local tag="$1"
  shift
  local metadata_args=("$@")

  if ! has_release_date_arg ${metadata_args[@]+"${metadata_args[@]}"}; then
    metadata_args+=(--date "$(release_date_from_tag "$tag")")
  fi

  resolve_release_metadata "$(version_from_tag "$tag")" "${metadata_args[@]}"
  set_version "$RESOLVED_VERSION" \
    --ios-build "$RESOLVED_IOS_BUILD" \
    --android-version-code "$RESOLVED_ANDROID_VERSION_CODE" \
    --date "$RESOLVED_RELEASE_DATE"
  verify_version "$RESOLVED_VERSION" \
    --ios-build "$RESOLVED_IOS_BUILD" \
    --android-version-code "$RESOLVED_ANDROID_VERSION_CODE"
  emit_release_metadata
}

metadata_from_tag() {
  local tag="$1"
  shift
  local metadata_args=("$@")

  if ! has_release_date_arg ${metadata_args[@]+"${metadata_args[@]}"}; then
    metadata_args+=(--date "$(release_date_from_tag "$tag")")
  fi

  resolve_release_metadata "$(version_from_tag "$tag")" "${metadata_args[@]}"
  emit_release_metadata
}

expect_line() {
  local file="$1"
  local pattern="$2"
  local message="$3"

  if ! grep -Eq "$pattern" "$file"; then
    echo "$message" >&2
    echo "  $file" >&2
    return 1
  fi
}

verify_version() {
  local version="$1"
  shift
  local ios_build=""
  local android_version_code=""
  local failures=0

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --ios-build)
        require_arg "$1" "${2:-}"
        ios_build="$2"
        shift 2
        ;;
      --ios-build=*)
        ios_build="${1#*=}"
        shift
        ;;
      --android-version-code)
        require_arg "$1" "${2:-}"
        android_version_code="$2"
        shift 2
        ;;
      --android-version-code=*)
        android_version_code="${1#*=}"
        shift
        ;;
      *)
        echo "Unexpected verify-version argument: $1" >&2
        usage
        exit 1
        ;;
    esac
  done

  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Version must be numeric major.minor.patch: $version" >&2; exit 1; }
  ios_build="${ios_build:-$(read_ios_build)}"
  android_version_code="${android_version_code:-$(read_android_version_code)}"
  [[ -n "$ios_build" ]] || { echo "Unable to resolve current iOS build version." >&2; exit 1; }
  [[ -n "$android_version_code" ]] || { echo "Unable to resolve current Android versionCode." >&2; exit 1; }
  [[ "$ios_build" =~ ^[0-9]+$ ]] || { echo "iOS build must be numeric: $ios_build" >&2; exit 1; }
  [[ "$android_version_code" =~ ^[0-9]+$ ]] || { echo "Android versionCode must be numeric: $android_version_code" >&2; exit 1; }

  expect_line "$ROOT_DIR/Cargo.toml" "^version = \"$version\"$" "Cargo workspace version mismatch." || failures=$((failures + 1))
  expect_line "$ROOT_DIR/lib/android/build.gradle.kts" "version = \"$version\"" "Android library version mismatch." || failures=$((failures + 1))
  expect_line "$ROOT_DIR/lib/ios/VesperPlayerKit/project.yml" "CFBundleShortVersionString: \"$version\"" "iOS marketing version mismatch." || failures=$((failures + 1))
  expect_line "$ROOT_DIR/lib/ios/VesperPlayerKit/project.yml" "CFBundleVersion: \"$ios_build\"" "iOS build version mismatch." || failures=$((failures + 1))
  expect_line "$ROOT_DIR/examples/android-compose-host/app/build.gradle.kts" "versionName = \"$version\"" "Android sample versionName mismatch." || failures=$((failures + 1))
  expect_line "$ROOT_DIR/examples/android-compose-host/app/build.gradle.kts" "versionCode = $android_version_code" "Android sample versionCode mismatch." || failures=$((failures + 1))
  expect_line "$ROOT_DIR/examples/flutter-host/pubspec.yaml" "version: $version\\+$android_version_code" "Flutter host version mismatch." || failures=$((failures + 1))

  for pubspec in "$ROOT_DIR"/lib/flutter/*/pubspec.yaml; do
    expect_line "$pubspec" "^version: $version$" "Flutter package version mismatch." || failures=$((failures + 1))
  done

  for gradle_file in \
    "$ROOT_DIR/lib/flutter/vesper_player_android/android/build.gradle" \
    "$ROOT_DIR/lib/flutter/vesper_player_external_playback/android/build.gradle"
  do
    expect_line "$gradle_file" "version = \"$version\"" "Flutter Android plugin Gradle version mismatch." || failures=$((failures + 1))
  done

  if [[ -f "$ROOT_DIR/Cargo.lock" ]]; then
    if awk -v version="$version" '
      /^\[\[package\]\]$/ {
        if ((name == "basic-player" || name ~ /^player-/) && package_version != version) {
          print name " " package_version
        }
        name = ""
        package_version = ""
        next
      }
      /^name = "/ {
        name = $3
        gsub(/"/, "", name)
      }
      /^version = "/ {
        package_version = $3
        gsub(/"/, "", package_version)
      }
      END {
        if ((name == "basic-player" || name ~ /^player-/) && package_version != version) {
          print name " " package_version
        }
      }
    ' "$ROOT_DIR/Cargo.lock" >/tmp/vesper-cargo-lock-version-mismatch.txt && [[ -s /tmp/vesper-cargo-lock-version-mismatch.txt ]]; then
      echo "Cargo.lock workspace package versions are not aligned with $version:" >&2
      cat /tmp/vesper-cargo-lock-version-mismatch.txt >&2
      failures=$((failures + 1))
    fi
  fi

  if rg -n --glob '!**/build/**' --glob '!**/target/**' --glob '!**/pubspec.lock' --glob '!devnotes/**' 'version: 0\.2\.0|version = "0\.2\.0"|CFBundleShortVersionString: "0\.2\.0"|<string>0\.2\.0</string>' \
    "$ROOT_DIR/Cargo.toml" \
    "$ROOT_DIR/CHANGELOG.md" \
    "$ROOT_DIR/lib/android" \
    "$ROOT_DIR/lib/flutter" \
    "$ROOT_DIR/lib/ios/VesperPlayerKit" \
    "$ROOT_DIR/examples/android-compose-host/app/build.gradle.kts" \
    "$ROOT_DIR/examples/flutter-host/pubspec.yaml" \
    "$ROOT_DIR/scripts/ios/stage-player-ffmpeg-runtime-release.sh" \
    "$ROOT_DIR/scripts/ios/stage-player-remux-ffmpeg-plugin-release.sh" \
    >/tmp/vesper-version-mismatch.txt; then
    echo "Found stale 0.2.0 product version fields:" >&2
    cat /tmp/vesper-version-mismatch.txt >&2
    failures=$((failures + 1))
  fi

  if rg -n \
    --glob '!**/build/**' \
    --glob '!**/target/**' \
    --glob '!scripts/README.md' \
    '(release (set-version|verify-version) [0-9]+\.[0-9]+\.[0-9]+|flutter (stage-pub|pub-dry-run|pub-publish) [^[:space:]]+ [0-9]+\.[0-9]+\.[0-9]+|VESPER_RELEASE_(VERSION|BUILD)="\$\{VESPER_RELEASE_[^:]+:-[0-9])' \
    "$ROOT_DIR/scripts" "$ROOT_DIR/.github/workflows" >/tmp/vesper-version-hardcode.txt; then
    echo "Found release script version hardcoding:" >&2
    cat /tmp/vesper-version-hardcode.txt >&2
    failures=$((failures + 1))
  fi

  if [[ "$failures" -ne 0 ]]; then
    echo "Version verification failed with $failures issue(s)." >&2
    exit 1
  fi

  echo "Verified Vesper product version $version."
}

verify_current() {
  local version
  local ios_build
  local android_version_code

  version="$(read_workspace_version)"
  ios_build="$(read_ios_build)"
  android_version_code="$(read_android_version_code)"

  [[ -n "$version" ]] || { echo "Unable to resolve current workspace version." >&2; exit 1; }
  [[ -n "$ios_build" ]] || { echo "Unable to resolve current iOS build version." >&2; exit 1; }
  [[ -n "$android_version_code" ]] || { echo "Unable to resolve current Android versionCode." >&2; exit 1; }

  verify_version "$version" \
    --ios-build "$ios_build" \
    --android-version-code "$android_version_code"
}

if [[ $# -lt 1 ]]; then
  usage
  exit 1
fi

command="$1"
shift

case "$command" in
  set)
    [[ $# -ge 1 ]] || { usage; exit 1; }
    version="$1"
    shift
    set_version "$version" "$@"
    ;;
  prepare-from-tag)
    [[ $# -ge 1 ]] || { usage; exit 1; }
    tag="$1"
    shift
    prepare_from_tag "$tag" "$@"
    ;;
  metadata-from-tag)
    [[ $# -ge 1 ]] || { usage; exit 1; }
    tag="$1"
    shift
    metadata_from_tag "$tag" "$@"
    ;;
  verify)
    [[ $# -ge 1 ]] || { usage; exit 1; }
    version="$1"
    shift
    verify_version "$version" "$@"
    ;;
  verify-current)
    verify_current
    ;;
  *)
    usage
    exit 1
    ;;
esac
