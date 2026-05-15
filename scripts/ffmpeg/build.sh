#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$SCRIPT_DIR/lib/common.sh"
source "$SCRIPT_DIR/lib/ffmpeg.sh"
source "$SCRIPT_DIR/lib/ffmpeg-profile.sh"
source "$SCRIPT_DIR/lib/ffmpeg-validate.sh"

usage() {
  cat <<EOF >&2
Usage: scripts/vesper ffmpeg --platform android|ios|all [options]

Options:
  --profile <name>                 FFmpeg profile name (default: default)
  --platform <android|ios|all>      Required build platform
  --list-profiles                  List declared profiles
  --dry-run                        Print resolved profile and build arguments
  --verify-only                    Validate existing artifacts without building
  --output-dir <path>              Override FFmpeg prebuilt output directory
  --android-artifact <kind>        runtime-aar or prebuilts (default: runtime-aar)
  --abi <abi>                      Android ABI, repeatable or comma-separated
  --slice <slice>                  iOS slice, repeatable or comma-separated
  --extra-libraries <csv>          Add FFmpeg libraries
  --extra-demuxers <csv>           Add FFmpeg demuxers
  --extra-muxers <csv>             Add FFmpeg muxers
  --extra-protocols <csv>          Add FFmpeg protocols
  --extra-parsers <csv>            Add FFmpeg parsers
  --extra-bsfs <csv>               Add FFmpeg bitstream filters
  --extra-configure-arg <arg>      Add raw FFmpeg configure argument
  --tls-backend <backend>          Override TLS backend
  --force                          Rebuild even when metadata matches
  --acknowledge-gpl-nonfree        Acknowledge GPL/nonfree configure flags
EOF
}

append_csv_to_array() {
  local target="$1"
  local csv="$2"
  local token

  csv="${csv//,/ }"
  for token in $csv; do
    [[ -n "$token" ]] || continue
    eval "$target+=(\"\$token\")"
  done
}

PROFILE="default"
PLATFORM=""
LIST_PROFILES=0
DRY_RUN=0
VERIFY_ONLY=0
OUTPUT_DIR=""
ANDROID_ARTIFACT="runtime-aar"
FORCE=0
ACK_GPL_NONFREE=0
ANDROID_ABIS=()
IOS_SLICES=()
OVERLAY_LIBRARIES=()
OVERLAY_DEMUXERS=()
OVERLAY_MUXERS=()
OVERLAY_PROTOCOLS=()
OVERLAY_PARSERS=()
OVERLAY_BSFS=()
OVERLAY_EXTRA_CONFIGURE_ARGS=()
OVERLAY_TLS_BACKEND=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      [[ -n "${2:-}" ]] || { echo "--profile requires a value." >&2; exit 1; }
      PROFILE="$2"
      shift 2
      ;;
    --profile=*)
      PROFILE="${1#*=}"
      shift
      ;;
    --platform)
      [[ -n "${2:-}" ]] || { echo "--platform requires a value." >&2; exit 1; }
      PLATFORM="$2"
      shift 2
      ;;
    --platform=*)
      PLATFORM="${1#*=}"
      shift
      ;;
    --list-profiles)
      LIST_PROFILES=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --verify-only)
      VERIFY_ONLY=1
      shift
      ;;
    --output-dir)
      [[ -n "${2:-}" ]] || { echo "--output-dir requires a value." >&2; exit 1; }
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --output-dir=*)
      OUTPUT_DIR="${1#*=}"
      shift
      ;;
    --android-artifact)
      [[ -n "${2:-}" ]] || { echo "--android-artifact requires a value." >&2; exit 1; }
      ANDROID_ARTIFACT="$2"
      shift 2
      ;;
    --android-artifact=*)
      ANDROID_ARTIFACT="${1#*=}"
      shift
      ;;
    --abi)
      [[ -n "${2:-}" ]] || { echo "--abi requires a value." >&2; exit 1; }
      append_csv_to_array ANDROID_ABIS "$2"
      shift 2
      ;;
    --abi=*)
      append_csv_to_array ANDROID_ABIS "${1#*=}"
      shift
      ;;
    --slice)
      [[ -n "${2:-}" ]] || { echo "--slice requires a value." >&2; exit 1; }
      append_csv_to_array IOS_SLICES "$2"
      shift 2
      ;;
    --slice=*)
      append_csv_to_array IOS_SLICES "${1#*=}"
      shift
      ;;
    --extra-libraries|--enable-libraries)
      [[ -n "${2:-}" ]] || { echo "$1 requires a value." >&2; exit 1; }
      append_csv_to_array OVERLAY_LIBRARIES "$2"
      shift 2
      ;;
    --extra-libraries=*|--enable-libraries=*)
      append_csv_to_array OVERLAY_LIBRARIES "${1#*=}"
      shift
      ;;
    --extra-demuxers|--enable-demuxers)
      [[ -n "${2:-}" ]] || { echo "$1 requires a value." >&2; exit 1; }
      append_csv_to_array OVERLAY_DEMUXERS "$2"
      shift 2
      ;;
    --extra-demuxers=*|--enable-demuxers=*)
      append_csv_to_array OVERLAY_DEMUXERS "${1#*=}"
      shift
      ;;
    --extra-muxers|--enable-muxers)
      [[ -n "${2:-}" ]] || { echo "$1 requires a value." >&2; exit 1; }
      append_csv_to_array OVERLAY_MUXERS "$2"
      shift 2
      ;;
    --extra-muxers=*|--enable-muxers=*)
      append_csv_to_array OVERLAY_MUXERS "${1#*=}"
      shift
      ;;
    --extra-protocols|--enable-protocols)
      [[ -n "${2:-}" ]] || { echo "$1 requires a value." >&2; exit 1; }
      append_csv_to_array OVERLAY_PROTOCOLS "$2"
      shift 2
      ;;
    --extra-protocols=*|--enable-protocols=*)
      append_csv_to_array OVERLAY_PROTOCOLS "${1#*=}"
      shift
      ;;
    --extra-parsers|--enable-parsers)
      [[ -n "${2:-}" ]] || { echo "$1 requires a value." >&2; exit 1; }
      append_csv_to_array OVERLAY_PARSERS "$2"
      shift 2
      ;;
    --extra-parsers=*|--enable-parsers=*)
      append_csv_to_array OVERLAY_PARSERS "${1#*=}"
      shift
      ;;
    --extra-bsfs|--enable-bsfs)
      [[ -n "${2:-}" ]] || { echo "$1 requires a value." >&2; exit 1; }
      append_csv_to_array OVERLAY_BSFS "$2"
      shift 2
      ;;
    --extra-bsfs=*|--enable-bsfs=*)
      append_csv_to_array OVERLAY_BSFS "${1#*=}"
      shift
      ;;
    --extra-configure-arg)
      [[ -n "${2:-}" ]] || { echo "--extra-configure-arg requires a value." >&2; exit 1; }
      OVERLAY_EXTRA_CONFIGURE_ARGS+=("$2")
      shift 2
      ;;
    --extra-configure-arg=*)
      OVERLAY_EXTRA_CONFIGURE_ARGS+=("${1#*=}")
      shift
      ;;
    --tls-backend)
      [[ -n "${2:-}" ]] || { echo "--tls-backend requires a value." >&2; exit 1; }
      OVERLAY_TLS_BACKEND="$2"
      shift 2
      ;;
    --tls-backend=*)
      OVERLAY_TLS_BACKEND="${1#*=}"
      shift
      ;;
    --force)
      FORCE=1
      shift
      ;;
    --acknowledge-gpl-nonfree)
      ACK_GPL_NONFREE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown FFmpeg option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ "$LIST_PROFILES" == "1" ]]; then
  vesper_ffmpeg_profile_list_names
  exit 0
fi

if [[ -z "$PLATFORM" ]]; then
  echo "--platform is required for FFmpeg builds." >&2
  usage
  exit 1
fi

case "$PLATFORM" in
  android|ios|all)
    ;;
  *)
    echo "Unsupported FFmpeg platform: $PLATFORM" >&2
    exit 1
    ;;
esac

case "$ANDROID_ARTIFACT" in
  runtime-aar|prebuilts)
    ;;
  *)
    echo "Unsupported Android FFmpeg artifact: $ANDROID_ARTIFACT" >&2
    exit 1
    ;;
esac

resolve_platform_args() {
  local platform="$1"
  local args=()
  local resolved_arg
  local protocols_csv
  local validation_args=()
  local restore_nounset=0

  vesper_ffmpeg_profile_resolve "$PROFILE" "$platform"
  vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_LIBRARIES "$(vesper_ffmpeg_profile_join_csv ${OVERLAY_LIBRARIES[@]+"${OVERLAY_LIBRARIES[@]}"})"
  vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_DEMUXERS "$(vesper_ffmpeg_profile_join_csv ${OVERLAY_DEMUXERS[@]+"${OVERLAY_DEMUXERS[@]}"})"
  vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_MUXERS "$(vesper_ffmpeg_profile_join_csv ${OVERLAY_MUXERS[@]+"${OVERLAY_MUXERS[@]}"})"
  vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_PROTOCOLS "$(vesper_ffmpeg_profile_join_csv ${OVERLAY_PROTOCOLS[@]+"${OVERLAY_PROTOCOLS[@]}"})"
  vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_PARSERS "$(vesper_ffmpeg_profile_join_csv ${OVERLAY_PARSERS[@]+"${OVERLAY_PARSERS[@]}"})"
  vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_BSFS "$(vesper_ffmpeg_profile_join_csv ${OVERLAY_BSFS[@]+"${OVERLAY_BSFS[@]}"})"
  if [[ "$-" == *u* ]]; then
    restore_nounset=1
    set +u
  fi
  for extra_arg in "${OVERLAY_EXTRA_CONFIGURE_ARGS[@]}"; do
    VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS+=("$extra_arg")
  done
  if [[ "$restore_nounset" == "1" ]]; then
    set -u
    restore_nounset=0
  fi
  if [[ -n "$OVERLAY_TLS_BACKEND" ]]; then
    VESPER_PROFILE_RESOLVED_TLS_BACKEND="$OVERLAY_TLS_BACKEND"
  fi

  protocols_csv="$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_PROTOCOLS[@]+"${VESPER_PROFILE_RESOLVED_PROTOCOLS[@]}"})"
  validation_args=(
    "$protocols_csv"
    "$VESPER_PROFILE_RESOLVED_TLS_BACKEND"
    "${VESPER_PROFILE_VALIDATION_FORBID_NETWORK:-false}"
    "${VESPER_PROFILE_VALIDATION_FORBID_OPENSSL:-false}"
  )
  if declare -p VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS >/dev/null 2>&1; then
    if [[ "$-" == *u* ]]; then
      restore_nounset=1
      set +u
    fi
    validation_args+=("${VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS[@]}")
    if [[ "$restore_nounset" == "1" ]]; then
      set -u
    fi
  fi
  vesper_ffmpeg_validate_resolved_profile "${validation_args[@]}"

  while IFS= read -r arg; do
    args+=("$arg")
  done < <(vesper_ffmpeg_profile_emit_legacy_args)

  if [[ "$FORCE" == "1" ]]; then
    args+=(--force)
  fi
  if [[ "$ACK_GPL_NONFREE" == "1" ]]; then
    args+=(--acknowledge-gpl-nonfree)
  fi

  if [[ "$DRY_RUN" == "1" ]]; then
    echo "Resolved FFmpeg profile:"
    vesper_ffmpeg_profile_print_resolved "$PROFILE" "$platform"
    if [[ "$platform" == "ios" ]]; then
      vesper_ffmpeg_parse_common_args apple "${args[@]}"
      printf 'profile_hash=%s\n' "$(vesper_ffmpeg_profile_key apple)"
    else
      vesper_ffmpeg_parse_common_args "$platform" "${args[@]}"
      printf 'profile_hash=%s\n' "$(vesper_ffmpeg_profile_key "$platform")"
    fi
    echo "Build arguments:"
    printf '  %q\n' ${args[@]+"${args[@]}"}
  fi

  RESOLVED_ARGS=()
  for resolved_arg in ${args[@]+"${args[@]}"}; do
    RESOLVED_ARGS+=("$resolved_arg")
  done
}

run_android() {
  local args=()
  local arg

  resolve_platform_args android
  for arg in ${RESOLVED_ARGS[@]+"${RESOLVED_ARGS[@]}"}; do
    args+=("$arg")
  done
  for arg in ${ANDROID_ABIS[@]+"${ANDROID_ABIS[@]}"}; do
    args+=("$arg")
  done

  vesper_ffmpeg_profile_export_validation_env
  export VESPER_DECLARED_FFMPEG_PROFILE="$PROFILE"
  export VESPER_DECLARED_FFMPEG_PLATFORM="android"
  if [[ -n "$OUTPUT_DIR" ]]; then
    export VESPER_FFMPEG_OUTPUT_DIR="$OUTPUT_DIR"
  fi

  if [[ "$DRY_RUN" == "1" ]]; then
    return 0
  fi

  if [[ "$VERIFY_ONLY" == "1" ]]; then
    vesper_ffmpeg_validate_android_runtime_artifacts \
      "$VESPER_REPO_ROOT/lib/android/vesper-player-kit-ffmpeg-runtime" \
      "${VESPER_FFMPEG_VALIDATION_FORBID_NETWORK:-false}" \
      "${VESPER_FFMPEG_VALIDATION_FORBID_OPENSSL:-false}"
    return 0
  fi

  if [[ "$ANDROID_ARTIFACT" == "runtime-aar" ]]; then
    "$SCRIPT_DIR/android/build-ffmpeg-runtime-aar.sh" ${args[@]+"${args[@]}"}
  else
    "$SCRIPT_DIR/android/build-ffmpeg-prebuilts.sh" ${args[@]+"${args[@]}"}
  fi
}

run_ios() {
  local args=()
  local arg

  resolve_platform_args ios
  for arg in ${RESOLVED_ARGS[@]+"${RESOLVED_ARGS[@]}"}; do
    args+=("$arg")
  done
  for arg in ${IOS_SLICES[@]+"${IOS_SLICES[@]}"}; do
    args+=("$arg")
  done

  vesper_ffmpeg_profile_export_validation_env
  export VESPER_DECLARED_FFMPEG_PROFILE="$PROFILE"
  export VESPER_DECLARED_FFMPEG_PLATFORM="ios"
  if [[ -n "$OUTPUT_DIR" ]]; then
    export VESPER_FFMPEG_OUTPUT_DIR="$OUTPUT_DIR"
  fi

  if [[ "$DRY_RUN" == "1" ]]; then
    return 0
  fi

  if [[ "$VERIFY_ONLY" == "1" ]]; then
    local output_root="${OUTPUT_DIR:-$VESPER_REPO_ROOT/third_party/ffmpeg/apple}"
    vesper_ffmpeg_validate_metadata_tree \
      "$output_root" \
      "${VESPER_FFMPEG_VALIDATION_FORBID_NETWORK:-false}" \
      "${VESPER_FFMPEG_VALIDATION_FORBID_OPENSSL:-false}"
    return 0
  fi

  "$SCRIPT_DIR/apple/build-ffmpeg-prebuilts.sh" ${args[@]+"${args[@]}"}
}

case "$PLATFORM" in
  android)
    run_android
    ;;
  ios)
    run_ios
    ;;
  all)
    run_android
    run_ios
    ;;
esac
