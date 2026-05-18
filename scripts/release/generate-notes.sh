#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
CURRENT_TAG="${1:-${GITHUB_REF_NAME:-}}"
OUTPUT_PATH="${2:-$ROOT_DIR/dist/release/RELEASE_NOTES.md}"

if [[ -z "$CURRENT_TAG" ]]; then
  echo "Usage: $0 <tag> [output-path]" >&2
  exit 1
fi

resolve_repository_url() {
  if [[ -n "${GITHUB_SERVER_URL:-}" && -n "${GITHUB_REPOSITORY:-}" ]]; then
    echo "${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}"
    return 0
  fi

  local origin_url
  origin_url="$(git config --get remote.origin.url 2>/dev/null || true)"
  origin_url="${origin_url%.git}"

  case "$origin_url" in
    git@github.com:*)
      echo "https://github.com/${origin_url#git@github.com:}"
      return 0
      ;;
    https://github.com/*|http://github.com/*)
      echo "$origin_url"
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

classify_commit_group() {
  local changed_paths="$1"

  if grep -Eq '^(lib/android/|lib/ios/|examples/android-compose-host/|examples/ios-swift-host/|crates/platform/mobile/|crates/platform/jni/)' <<<"$changed_paths"; then
    echo "Mobile Platform Kits"
    return 0
  fi

  if grep -Eq '^(examples/basic-player/|crates/platform/desktop/|crates/platform/common/player-platform-desktop/|crates/platform/common/player-platform-apple/)' <<<"$changed_paths"; then
    echo "Desktop Runtime & Demo"
    return 0
  fi

  if grep -Eq '^(crates/core/)' <<<"$changed_paths"; then
    echo "Core Runtime & FFI"
    return 0
  fi

  if grep -Eq '^(crates/backend/|crates/audio/|crates/render/)' <<<"$changed_paths"; then
    echo "Media Pipeline"
    return 0
  fi

  if grep -Eq '^(\.github/workflows/|scripts/)' <<<"$changed_paths"; then
    echo "CI & Release Tooling"
    return 0
  fi

  if grep -Eq '^(docs/|ROADMAP\.md$|README\.md$)' <<<"$changed_paths"; then
    echo "Docs & Planning"
    return 0
  fi

  echo "Other Changes"
}

release_channel() {
  local tag="$1"

  if [[ "$tag" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "stable"
  else
    echo "prerelease"
  fi
}

emit_grouped_commits() {
  local range_spec="$1"
  local temp_dir
  local category
  local sha
  local short_sha
  local subject
  local author
  local changed_paths

  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT

  while IFS= read -r sha; do
    [[ -n "$sha" ]] || continue

    short_sha="$(git rev-parse --short "$sha")"
    subject="$(git log -1 --format='%s' "$sha")"
    author="$(git log -1 --format='%an' "$sha")"
    changed_paths="$(git show --pretty='' --name-only "$sha")"
    category="$(classify_commit_group "$changed_paths")"

    printf -- '- `%s` %s (%s)\n' "$short_sha" "$subject" "$author" >>"$temp_dir/$category.txt"
  done < <(git log --no-merges --format='%H' "$range_spec" || true)

  for category in \
    "Mobile Platform Kits" \
    "Desktop Runtime & Demo" \
    "Core Runtime & FFI" \
    "Media Pipeline" \
    "CI & Release Tooling" \
    "Docs & Planning" \
    "Other Changes"
  do
    if [[ -s "$temp_dir/$category.txt" ]]; then
      echo "### $category"
      echo
      cat "$temp_dir/$category.txt"
      echo
    fi
  done
}

git rev-parse --verify "${CURRENT_TAG}^{commit}" >/dev/null

PREVIOUS_TAG="$(git describe --tags --abbrev=0 "${CURRENT_TAG}^" 2>/dev/null || true)"
RANGE_SPEC="$CURRENT_TAG"
if [[ -n "$PREVIOUS_TAG" ]]; then
  RANGE_SPEC="${PREVIOUS_TAG}..${CURRENT_TAG}"
fi

REPOSITORY_URL="$(resolve_repository_url || true)"
COMPARE_URL=""
if [[ -n "$PREVIOUS_TAG" && -n "$REPOSITORY_URL" ]]; then
  COMPARE_URL="${REPOSITORY_URL}/compare/${PREVIOUS_TAG}...${CURRENT_TAG}"
fi
RELEASE_CHANNEL="$(release_channel "$CURRENT_TAG")"

mkdir -p "$(dirname "$OUTPUT_PATH")"

contributor_lines="$(git shortlog -sne "$RANGE_SPEC" | sed 's/^/- /' || true)"
if [[ -z "$contributor_lines" ]]; then
  contributor_lines="- No contributor metadata found"
fi

{
  echo "# VesperPlayerKit ${CURRENT_TAG}"
  echo
  echo "This release packages the current VesperPlayerKit mobile integration bundles for Android and iOS."
  echo
  echo "## Download Packages"
  echo
  echo "- Android core AAR for devices: \`VesperPlayerKit-android-arm64-v8a.aar\`"
  echo "- Android Compose adapter AAR for devices: \`VesperPlayerKitCompose-android-arm64-v8a.aar\`"
  echo "- Android Compose UI AAR for devices: \`VesperPlayerKitComposeUi-android-arm64-v8a.aar\`"
  echo "- Android external playback AAR for devices: \`VesperPlayerKitExternalPlayback-android-arm64-v8a.aar\`"
  echo "- Android FFmpeg runtime AAR for devices: \`VesperPlayerKitFfmpegRuntime-android-arm64-v8a.aar\`"
  echo "- iOS device framework: \`VesperPlayerKit-ios-arm64.framework.zip\`"
  echo "- iOS simulator framework for Apple Silicon: \`VesperPlayerKit-ios-simulator-arm64.framework.zip\`"
  echo "- Combined Apple package: \`VesperPlayerKit.xcframework.zip\`"
  echo "- Optional iOS FFmpeg remux plugin: \`VesperPlayerRemuxFfmpegPlugin.xcframework.zip\`"
  echo "- Integrity checksums: \`SHA256SUMS.txt\`"
  echo
  echo "These downloads are prebuilt binary artifacts. Host applications that consume them do not need to run the repository's local JNI or FFmpeg generation tasks during their own Gradle / Xcode builds. FFmpeg-backed artifacts keep FFmpeg's license, notices, corresponding source, configure flags, and LGPL relinking boundary separate from Vesper's Apache-2.0 source license."
  echo
  echo "## Sample Apps"
  echo
  echo "- Android Compose host sample: \`VesperPlayerAndroidComposeHost-android-arm64-v8a-debug-signed.apk\`"
  echo "- Flutter Android host sample: \`VesperPlayerFlutterHost-android-arm64-v8a-debug-signed.apk\`"
  echo
  echo "The sample APKs are arm64-only, debug-signed, and published only for side-load evaluation of the SDK demos. They are not production-signed app-store artifacts and should not be used as a host application's release signing model."
  echo
  echo "## Release Details"
  echo
  if [[ -n "$PREVIOUS_TAG" ]]; then
    echo "- Previous version: \`${PREVIOUS_TAG}\`"
  else
    echo "- Previous version: first tagged VesperPlayerKit release"
  fi
  echo "- Release tag: \`${CURRENT_TAG}\`"
  echo "- Release channel: ${RELEASE_CHANNEL}"
  if [[ -n "$COMPARE_URL" ]]; then
    echo "- Compare changes: [\`${PREVIOUS_TAG}...${CURRENT_TAG}\`](${COMPARE_URL})"
  fi
  echo
  echo "## Included Changes"
  echo
  if git log --no-merges --format='%H' "$RANGE_SPEC" | grep -q .; then
    emit_grouped_commits "$RANGE_SPEC"
  else
    echo "- Initial tagged VesperPlayerKit release"
    echo
  fi
  echo
  echo "## Release Contributors"
  echo
  printf '%s\n' "$contributor_lines"
} >"$OUTPUT_PATH"

echo "Generated VesperPlayerKit release notes at:"
echo "  $OUTPUT_PATH"
