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

category_label_en() {
  local category="$1"

  case "$category" in
    "Mobile Platform Kits")
      echo "Mobile Platform Kits"
      ;;
    "Desktop Runtime & Demo")
      echo "Desktop Runtime & Demo"
      ;;
    "Core Runtime & FFI")
      echo "Core Runtime & FFI"
      ;;
    "Media Pipeline")
      echo "Media Pipeline"
      ;;
    "CI & Release Tooling")
      echo "CI & Release Tooling"
      ;;
    "Docs & Planning")
      echo "Docs & Planning"
      ;;
    *)
      echo "Other Changes"
      ;;
  esac
}

category_label_zh() {
  local category="$1"

  case "$category" in
    "Mobile Platform Kits")
      echo "移动端平台套件"
      ;;
    "Desktop Runtime & Demo")
      echo "桌面运行时与示例"
      ;;
    "Core Runtime & FFI")
      echo "核心运行时与 FFI"
      ;;
    "Media Pipeline")
      echo "媒体管线"
      ;;
    "CI & Release Tooling")
      echo "CI 与发布工具"
      ;;
    "Docs & Planning")
      echo "文档与规划"
      ;;
    *)
      echo "其他变更"
      ;;
  esac
}

translate_commit_subject() {
  local subject="$1"

  case "$subject" in
    "fix: add error codes to VesperPlayerError for better error handling fix: reject insecure HTTP URLs in VesperForegroundDownloadExecutor")
      echo "为 VesperPlayerError 补齐错误码以改进错误处理，并在 VesperForegroundDownloadExecutor 中拒绝不安全的 HTTP URL"
      ;;
    "refactor: rename error ordinal functions for consistency with JNI terminology")
      echo "重命名错误序号相关函数，使其与 JNI 术语保持一致"
      ;;
    "Add scripts for synchronizing and verifying VesperPlayerKit bridge shim")
      echo "新增 VesperPlayerKit bridge shim 同步与校验脚本"
      ;;
    "fix: reorder ffmpeg command arguments for consistency and clarity")
      echo "调整 FFmpeg 命令参数顺序，使脚本更一致、更清晰"
      ;;
    "Refactor Android build scripts to improve Gradle resolution and add sample APK staging")
      echo "重构 Android 构建脚本，改进 Gradle 解析并新增示例 APK 暂存"
      ;;
    "feat(download): implement download types and structures for asset management")
      echo "实现下载资产管理所需的类型与数据结构"
      ;;
    "feat(dlna): refactor DLNA session methods for improved async handling and error management")
      echo "重构 DLNA 会话方法，改进异步处理与错误管理"
      ;;
    "feat: Add external playback support and AirPlay integration")
      echo "新增外部播放支持与 AirPlay 集成"
      ;;
    "feat(dash): enhance DASH support with remote media references and request headers")
      echo "增强 DASH 对远程媒体引用和请求头的支持"
      ;;
    "feat(dash): add support for remote media references in DASH resource resolvers and tests")
      echo "在 DASH 资源解析器与测试中加入远程媒体引用支持"
      ;;
    "Enhance DASH handling with support for SegmentBase and byte range requests")
      echo "增强 DASH 处理能力，支持 SegmentBase 与字节范围请求"
      ;;
    "feat(relay): add prewarm functionality and validation for DASH sources in VesperRelayServer")
      echo "在 VesperRelayServer 中为 DASH 源加入预热能力和校验"
      ;;
    "feat(dlna): add refresh functionality for external routes and improve diagnostic logging")
      echo "为外部路由加入刷新能力，并改进诊断日志"
      ;;
    "Add support for local DASH sources and enhance DASH parsing")
      echo "新增本地 DASH 源支持并增强 DASH 解析"
      ;;
    "feat(dlna): add asynchronous methods for playback control and loading media")
      echo "为播放控制和媒体加载加入异步 DLNA 方法"
      ;;
    "feat(external-playback): enhance diagnostic messages with HTTP status in VesperRelayServer")
      echo "在 VesperRelayServer 外部播放诊断信息中加入 HTTP 状态"
      ;;
    "feat(dlna): improve error handling in VesperDlnaSoapClient and add detailed failure messages")
      echo "改进 VesperDlnaSoapClient 错误处理并加入更详细的失败信息"
      ;;
    "feat(dlna): enhance DLNA device route matching and discovery handling")
      echo "增强 DLNA 设备路由匹配与发现处理"
      ;;
    "refactor!(mobile): release version 0.3.0, refactor module structure and FFmpeg build process")
      echo "发布 0.3.0 移动端结构，重构模块划分与 FFmpeg 构建流程"
      ;;
    *)
      echo "$subject"
      ;;
  esac
}

asset_link() {
  local asset="$1"

  if [[ -n "${DOWNLOAD_BASE_URL:-}" ]]; then
    printf '[%s](%s/%s)' "$asset" "$DOWNLOAD_BASE_URL" "$asset"
  else
    printf '`%s`' "$asset"
  fi
}

emit_download_item() {
  local asset="$1"
  local label="$2"
  local link

  link="$(asset_link "$asset")"
  printf -- '- %s - %s\n' "$link" "$label"
}

emit_initial_release_en() {
  cat <<'EOF'
## Fixes

- This is the first VesperPlayerKit release candidate, so there is no prior release regression set. This cut includes pre-release stability fixes for Dart error-code construction, iOS insecure-HTTP rejection, Android / CI release dependency wiring, Android sample APK packaging, and iOS framework staging.
- FFmpeg, Gradle, bridge-shim, Android sample APK, and iOS framework release scripts were tightened so mobile binary artifacts can be generated reliably from the tag workflow.

## New Capabilities

- Android ships a core host-kit AAR, Compose binding, Compose UI package, external playback extension, and split FFmpeg runtime package for arm64-v8a devices.
- iOS ships a device framework, Apple Silicon simulator framework, combined XCFramework, and optional FFmpeg shared runtime plus remux plugin XCFrameworks.
- Flutter and Android Compose sample apps are published with the release for quick integration checks.
- Core capabilities include DASH / HLS bridging, offline download and export, remote media references, request-header forwarding, SegmentBase / byte-range handling, DLNA / AirPlay external playback, and FFmpeg remux post-processing.

## Improvements

- Android defaults to hardware decoding and the SurfaceView path, with release artifacts split by module so host apps can depend only on the capabilities they need.
- iOS keeps the SPM / XCFramework distribution path and separates the FFmpeg plugin from the main SDK, preserving FFmpeg's independent license, notices, source, and LGPL relinking boundary.
- The release flow generates checksums and verifies Android / iOS artifacts contain only the expected arm64 slices.
EOF
}

emit_initial_release_zh() {
  cat <<'EOF'
## 修复问题

- 这是 VesperPlayerKit 的首次候选发布，没有历史版本回归修复对比。本轮发布前已补齐 Dart 错误码构造、iOS 不安全 HTTP 拦截、Android / CI 发布依赖声明、Android 示例 APK 打包，以及 iOS framework 暂存等稳定性问题。
- 修正 FFmpeg、Gradle、bridge shim、Android 示例 APK 和 iOS framework 发布脚本细节，让移动端二进制产物可以由 tag 工作流稳定生成。

## 新增功能

- Android 提供核心 Host Kit AAR、Compose 绑定、Compose UI 包、外部播放扩展和 FFmpeg Runtime 拆分包，面向 arm64-v8a 设备发布。
- iOS 提供真机 framework、Apple Silicon 模拟器 framework、合并 XCFramework，以及可选的 FFmpeg shared runtime 和 remux 插件 XCFramework。
- Flutter 示例和 Android Compose 示例随 release 一起提供，方便快速验证接入效果。
- 核心能力覆盖 DASH / HLS 桥接、离线下载与导出、远程媒体引用、请求头透传、SegmentBase / byte-range 处理、DLNA / AirPlay 外部播放，以及 FFmpeg remux 后处理。

## 优化改进

- Android 默认走硬件解码和 SurfaceView 路径，发布产物按模块拆分，便于宿主应用只接入需要的能力。
- iOS 保持 SPM / XCFramework 分发路径，并把 FFmpeg shared runtime / plugin 与主 SDK 分离，保留 FFmpeg 独立许可、notice、源码和 LGPL relinking 边界。
- 发布流程会生成校验和，并校验 Android / iOS 产物只包含预期的 arm64 切片。
EOF
}

emit_incremental_release_en() {
  cat <<'EOF'
## Fixes

- Fixes for this version are listed in the module-grouped change summary below.

## New Capabilities

- New capabilities for this version are listed in the change summary below and reflected in the platform-specific downloads.

## Improvements

- Build, release, platform integration, and runtime improvements are listed in the change summary below.
EOF
}

emit_incremental_release_zh() {
  cat <<'EOF'
## 修复问题

- 本版本的修复项请查看下方按模块整理的变更摘要。

## 新增功能

- 本版本新增能力请查看下方变更摘要和对应平台下载产物。

## 优化改进

- 构建、发布、平台集成和运行时优化请查看下方变更摘要。
EOF
}

emit_grouped_commits_en() {
  local range_spec="$1"
  local temp_dir
  local category
  local sha
  local short_sha
  local subject
  local author
  local changed_paths

  temp_dir="$(mktemp -d)"

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
      echo "### $(category_label_en "$category")"
      echo
      cat "$temp_dir/$category.txt"
      echo
    fi
  done

  rm -rf "$temp_dir"
}

emit_grouped_commits_zh() {
  local range_spec="$1"
  local temp_dir
  local category
  local sha
  local short_sha
  local subject
  local translated_subject
  local author
  local changed_paths

  temp_dir="$(mktemp -d)"

  while IFS= read -r sha; do
    [[ -n "$sha" ]] || continue

    short_sha="$(git rev-parse --short "$sha")"
    subject="$(git log -1 --format='%s' "$sha")"
    translated_subject="$(translate_commit_subject "$subject")"
    author="$(git log -1 --format='%an' "$sha")"
    changed_paths="$(git show --pretty='' --name-only "$sha")"
    category="$(classify_commit_group "$changed_paths")"

    printf -- '- `%s` %s (%s)\n' "$short_sha" "$translated_subject" "$author" >>"$temp_dir/$category.txt"
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
      echo "### $(category_label_zh "$category")"
      echo
      cat "$temp_dir/$category.txt"
      echo
    fi
  done

  rm -rf "$temp_dir"
}

release_contributor_lines() {
  local range_spec="$1"
  local lines

  lines="$(git log --format='%ae%x09%an' "$range_spec" \
    | awk -F '\t' 'NF >= 2 && !seen[$1]++ { print "- " $2 }' || true)"

  if [[ -n "$lines" ]]; then
    printf '%s\n' "$lines"
  else
    echo "- No contributor metadata found"
  fi
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
DOWNLOAD_BASE_URL=""
if [[ -n "$REPOSITORY_URL" ]]; then
  DOWNLOAD_BASE_URL="${REPOSITORY_URL}/releases/download/${CURRENT_TAG}"
fi
RELEASE_CHANNEL="$(release_channel "$CURRENT_TAG")"

mkdir -p "$(dirname "$OUTPUT_PATH")"

contributor_lines="$(release_contributor_lines "$RANGE_SPEC")"

{
  echo "# VesperPlayerKit ${CURRENT_TAG}"
  echo
  echo "VesperPlayerKit ${CURRENT_TAG} is a release for the Android and iOS mobile SDK bundles."
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
  if [[ -z "$PREVIOUS_TAG" ]]; then
    emit_initial_release_en
  else
    emit_incremental_release_en
  fi
  echo
  echo "## Change Summary"
  echo
  if [[ -z "$PREVIOUS_TAG" ]]; then
    echo "- This is the first GitHub release. The commit history has been condensed into the capability summary above for first-time integration review."
  elif git log --no-merges --format='%H' "$RANGE_SPEC" | grep -q .; then
    emit_grouped_commits_en "$RANGE_SPEC"
  else
    echo "- No non-merge commits were found in this range."
  fi
  echo
  echo "---"
  echo
  echo "# VesperPlayerKit ${CURRENT_TAG} 中文说明"
  echo
  echo "VesperPlayerKit ${CURRENT_TAG} 是 Android 与 iOS 移动端 SDK 二进制发布包。"
  echo
  echo "## 发布信息"
  echo
  if [[ -n "$PREVIOUS_TAG" ]]; then
    echo "- 上一个版本：\`${PREVIOUS_TAG}\`"
  else
    echo "- 上一个版本：首个带标签发布版本"
  fi
  echo "- 发布标签：\`${CURRENT_TAG}\`"
  echo "- 发布通道：${RELEASE_CHANNEL}"
  if [[ -n "$COMPARE_URL" ]]; then
    echo "- 变更对比：[\`${PREVIOUS_TAG}...${CURRENT_TAG}\`](${COMPARE_URL})"
  fi
  echo
  if [[ -z "$PREVIOUS_TAG" ]]; then
    emit_initial_release_zh
  else
    emit_incremental_release_zh
  fi
  echo
  echo "## 变更摘要"
  echo
  if [[ -z "$PREVIOUS_TAG" ]]; then
    echo "- 这是首次 GitHub Release。英文提交历史已整理为上方能力摘要，方便首次接入评估。"
  elif git log --no-merges --format='%H' "$RANGE_SPEC" | grep -q .; then
    emit_grouped_commits_zh "$RANGE_SPEC"
  else
    echo "- 此范围内没有非合并提交。"
  fi
  echo
  echo "---"
  echo
  echo "## Downloads"
  echo
  echo "These downloads are prebuilt binary artifacts. Host applications do not need to run this repository's JNI or FFmpeg generation tasks during their own Gradle / Xcode builds."
  echo
  echo "### Android"
  echo
  emit_download_item "VesperPlayerKit-android-arm64-v8a.aar" "Core Android host-kit AAR"
  emit_download_item "VesperPlayerKitCompose-android-arm64-v8a.aar" "Jetpack Compose binding AAR"
  emit_download_item "VesperPlayerKitComposeUi-android-arm64-v8a.aar" "Optional Compose UI controls AAR"
  emit_download_item "VesperPlayerKitExternalPlayback-android-arm64-v8a.aar" "External playback extension AAR"
  emit_download_item "VesperPlayerKitFfmpegRuntime-android-arm64-v8a.aar" "FFmpeg runtime AAR"
  emit_download_item "VesperPlayerKitSourceNormalizerFfmpeg-android-arm64-v8a.aar" "Optional SourceNormalizer FFmpeg diagnostics/preflight plugin AAR"
  emit_download_item "VesperPlayerKitFrameProcessorDiagnostic-android-arm64-v8a.aar" "Optional FrameProcessor diagnostic plugin AAR"
  emit_download_item "VesperPlayerAndroidComposeHost-android-arm64-v8a-debug-signed.apk" "Android Compose sample APK, debug-signed for side-load evaluation only"
  emit_download_item "VesperPlayerFlutterHost-android-arm64-v8a-debug-signed.apk" "Flutter Android sample APK, debug-signed for side-load evaluation only"
  echo
  echo "### iOS"
  echo
  emit_download_item "VesperPlayerKit-ios-arm64.framework.zip" "iOS device framework"
  emit_download_item "VesperPlayerKit-ios-simulator-arm64.framework.zip" "Apple Silicon simulator framework"
  emit_download_item "VesperPlayerKit.xcframework.zip" "Combined XCFramework"
  emit_download_item "VesperPlayerFfmpegRuntime.xcframework.zip" "Optional FFmpeg shared runtime XCFramework"
  emit_download_item "VesperPlayerRemuxFfmpegPlugin.xcframework.zip" "Optional FFmpeg remux plugin XCFramework"
  emit_download_item "VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip" "Optional SourceNormalizer FFmpeg diagnostics/preflight plugin XCFramework"
  emit_download_item "VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip" "Optional FrameProcessor diagnostic plugin XCFramework"
  echo
  echo "### Checksums and Licensing"
  echo
  emit_download_item "SHA256SUMS.txt" "SHA-256 checksums for release artifacts"
  echo
  echo "FFmpeg-backed artifacts keep FFmpeg's license, notices, corresponding source, configure flags, and LGPL relinking boundary separate from Vesper's Apache-2.0 source license. SourceNormalizer mobile v1 is diagnostics/preflight only, FrameProcessor mobile v1 is diagnostics-only, and Decoder mobile artifacts remain deferred."
  echo
  echo "## Release Contributors"
  echo
  printf '%s\n' "$contributor_lines"
} >"$OUTPUT_PATH"

echo "Generated VesperPlayerKit release notes at:"
echo "  $OUTPUT_PATH"
