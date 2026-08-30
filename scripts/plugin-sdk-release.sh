#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly REPO_ROOT
readonly CRATES_IO_API="https://crates.io/api/v1/crates"
readonly CRATES_IO_USER_AGENT="vesper-player-sdk-release (https://github.com/umbrella22/Vesper)"
readonly RUST_TOOLCHAIN="1.98.0"

readonly PACKAGES=(
  vesper-player-plugin-abi
  vesper-player-plugin-macros
  vesper-player-plugin-wasm
  vesper-player-platform-process
  vesper-player-plugin
  vesper-player-plugin-package
  vesper-player-plugin-wasm-host
  vesper-player-plugin-loader
  vesper-player-cli
)

PACKAGE_PATCH_ARGS=()

usage() {
  cat <<'USAGE'
Usage:
  scripts/plugin-sdk-release.sh verify [VERSION|plugin-sdk-vVERSION]
  scripts/plugin-sdk-release.sh status [VERSION|plugin-sdk-vVERSION]
  scripts/plugin-sdk-release.sh publish [VERSION|plugin-sdk-vVERSION]

verify builds, tests, and packages the public Rust plugin SDK and CLI from a
temporary source snapshot. status checks that every package version is visible
on crates.io. publish requires a clean main checkout synchronized with its
upstream and uses Cargo's configured crates.io credential; it never accepts a
token on the command line.
USAGE
}

workspace_version() {
  awk '
    $0 == "[workspace.package]" { in_workspace_package = 1; next }
    /^\[/ && in_workspace_package { exit }
    in_workspace_package && $1 == "version" && $2 == "=" {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "$REPO_ROOT/Cargo.toml"
}

normalize_version() {
  local requested="${1:-}"
  local version

  if [[ -z "$requested" ]]; then
    version="$(workspace_version)"
  elif [[ "$requested" == plugin-sdk-v* ]]; then
    version="${requested#plugin-sdk-v}"
  else
    version="$requested"
  fi

  if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
    echo "Invalid plugin SDK version: $version" >&2
    exit 2
  fi

  local current
  current="$(workspace_version)"
  if [[ "$version" != "$current" ]]; then
    echo "Requested version $version does not match workspace version $current." >&2
    exit 2
  fi

  printf '%s\n' "$version"
}

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Required command is unavailable: $command_name" >&2
    exit 2
  fi
}

run_cargo() {
  rustup run "$RUST_TOOLCHAIN" cargo "$@"
}

add_package_patch() {
  local package="$1"
  local path="$2"
  PACKAGE_PATCH_ARGS+=(
    --config
    "patch.crates-io.${package}.path=\"${path}\""
  )
}

resolve_package_patches() {
  local package="$1"
  PACKAGE_PATCH_ARGS=()

  case "$package" in
    vesper-player-plugin)
      add_package_patch vesper-player-plugin-abi crates/plugin/player-plugin-abi
      add_package_patch vesper-player-plugin-macros crates/plugin/player-plugin-macros
      ;;
    vesper-player-plugin-package)
      add_package_patch vesper-player-plugin crates/plugin/player-plugin
      add_package_patch vesper-player-plugin-abi crates/plugin/player-plugin-abi
      ;;
    vesper-player-plugin-wasm-host)
      add_package_patch vesper-player-plugin crates/plugin/player-plugin
      ;;
    vesper-player-plugin-loader)
      add_package_patch vesper-player-plugin crates/plugin/player-plugin
      add_package_patch vesper-player-plugin-abi crates/plugin/player-plugin-abi
      add_package_patch vesper-player-plugin-package crates/plugin/player-plugin-package
      add_package_patch vesper-player-plugin-wasm-host crates/plugin/player-plugin-wasm-host
      ;;
    vesper-player-cli)
      add_package_patch vesper-player-platform-process \
        crates/platform/common/player-platform-process
      add_package_patch vesper-player-plugin crates/plugin/player-plugin
      add_package_patch vesper-player-plugin-abi crates/plugin/player-plugin-abi
      add_package_patch vesper-player-plugin-loader crates/plugin/player-plugin-loader
      add_package_patch vesper-player-plugin-package crates/plugin/player-plugin-package
      add_package_patch vesper-player-plugin-wasm-host crates/plugin/player-plugin-wasm-host
      ;;
  esac
}

create_snapshot() {
  local snapshot_root="$1"

  git -C "$REPO_ROOT" archive --format=tar HEAD | tar -xf - -C "$snapshot_root"
  git -C "$REPO_ROOT" diff --binary HEAD -- . \
    | (cd "$snapshot_root" && git apply --whitespace=nowarn)

  while IFS= read -r -d '' path; do
    mkdir -p "$snapshot_root/$(dirname "$path")"
    cp -p "$REPO_ROOT/$path" "$snapshot_root/$path"
  done < <(git -C "$REPO_ROOT" ls-files --others --exclude-standard -z)
}

verify_snapshot() {
  local snapshot_root="$1"
  local version="$2"
  local package
  local package_args=()

  for package in "${PACKAGES[@]}"; do
    package_args+=(--package "$package")
  done

  echo "Verifying Vesper plugin SDK $version in an isolated source snapshot."
  (
    cd "$snapshot_root"
    CARGO_TARGET_DIR="$snapshot_root/target/plugin-sdk-release" \
      run_cargo metadata --locked --format-version 1 --no-deps >/dev/null
    CARGO_TARGET_DIR="$snapshot_root/target/plugin-sdk-release" \
      run_cargo test --locked --no-fail-fast "${package_args[@]}" --all-targets \
        -- --test-threads=1
  )

  package_snapshot "$snapshot_root"
}

package_snapshot() {
  local snapshot_root="$1"
  local package

  (
    cd "$snapshot_root"
    for package in "${PACKAGES[@]}"; do
      resolve_package_patches "$package"
      if (( ${#PACKAGE_PATCH_ARGS[@]} == 0 )); then
        CARGO_TARGET_DIR="$snapshot_root/target/plugin-sdk-release" \
          run_cargo package --locked --offline --no-verify --package "$package"
      else
        CARGO_TARGET_DIR="$snapshot_root/target/plugin-sdk-release" \
          run_cargo "${PACKAGE_PATCH_ARGS[@]}" package --locked --offline \
            --no-verify --package "$package"
      fi
    done
  )
}

verify_release() {
  local version="$1"
  local snapshot_root
  snapshot_root="$(mktemp -d)"
  trap 'rm -rf -- "$snapshot_root"' EXIT
  create_snapshot "$snapshot_root"
  verify_snapshot "$snapshot_root" "$version"
  rm -rf -- "$snapshot_root"
  trap - EXIT
}

crate_version_status() {
  local package="$1"
  local version="$2"
  local http_code

  if ! http_code="$(curl --silent --show-error \
    --user-agent "$CRATES_IO_USER_AGENT" \
    --output /dev/null \
    --write-out '%{http_code}' \
    "$CRATES_IO_API/$package/$version")"; then
    return 2
  fi

  case "$http_code" in
    200) return 0 ;;
    404) return 1 ;;
    *)
      echo "crates.io returned HTTP $http_code for $package $version." >&2
      return 2
      ;;
  esac
}

registry_version_checksum() {
  local package="$1"
  local version="$2"
  local response

  response="$(curl --silent --show-error --fail \
    --user-agent "$CRATES_IO_USER_AGENT" \
    "$CRATES_IO_API/$package/$version")" || return 2
  printf '%s' "$response" | jq --exit-status --raw-output \
    --arg package "$package" \
    --arg version "$version" \
    '.version
      | select(.crate == $package and .num == $version and .yanked == false)
      | .checksum
      | select(test("^[0-9a-f]{64}$"))'
}

verify_published_archive() {
  local snapshot_root="$1"
  local package="$2"
  local version="$3"
  local archive
  local local_checksum
  local registry_checksum

  archive="$snapshot_root/target/plugin-sdk-release/package/$package-$version.crate"
  if [[ ! -f "$archive" ]]; then
    echo "Local package archive is missing: $archive" >&2
    return 2
  fi

  local_checksum="$(shasum -a 256 "$archive" | awk '{print $1}')"
  registry_checksum="$(registry_version_checksum "$package" "$version")" || return 2
  if [[ "$local_checksum" != "$registry_checksum" ]]; then
    echo "Published checksum mismatch for $package $version." >&2
    echo "local:    $local_checksum" >&2
    echo "crates.io: $registry_checksum" >&2
    return 1
  fi

  echo "Verified crates.io checksum for $package $version."
}

check_registry_status() {
  local version="$1"
  local package
  local missing=0
  local result

  for package in "${PACKAGES[@]}"; do
    if crate_version_status "$package" "$version"; then
      if ! registry_version_checksum "$package" "$version" >/dev/null; then
        echo "Invalid or yanked crates.io record for $package $version." >&2
        return 2
      fi
      echo "available  $package $version"
    else
      result=$?
      if [[ "$result" -eq 1 ]]; then
        echo "missing    $package $version"
        missing=1
      else
        echo "Unable to query crates.io for $package $version." >&2
        return 2
      fi
    fi
  done

  return "$missing"
}

wait_for_registry() {
  local package="$1"
  local version="$2"
  local attempts_remaining=60
  local result

  while (( attempts_remaining > 0 )); do
    if crate_version_status "$package" "$version"; then
      echo "crates.io indexed $package $version."
      return 0
    else
      result=$?
      if [[ "$result" -eq 2 ]]; then
        return 2
      fi
    fi
    sleep 5
    attempts_remaining=$((attempts_remaining - 1))
  done

  echo "Timed out waiting for crates.io to index $package $version." >&2
  return 1
}

require_publish_checkout() {
  local branch
  local upstream

  if [[ -n "$(git -C "$REPO_ROOT" status --porcelain --untracked-files=normal)" ]]; then
    echo "Publishing requires a clean checkout." >&2
    exit 2
  fi

  branch="$(git -C "$REPO_ROOT" branch --show-current)"
  if [[ "$branch" != "main" ]]; then
    echo "Publishing requires the main branch; current branch is ${branch:-detached}." >&2
    exit 2
  fi

  upstream="$(git -C "$REPO_ROOT" rev-parse --abbrev-ref --symbolic-full-name '@{upstream}')"
  git -C "$REPO_ROOT" fetch --no-tags origin main
  if [[ "$(git -C "$REPO_ROOT" rev-parse HEAD)" != "$(git -C "$REPO_ROOT" rev-parse "$upstream")" ]]; then
    echo "Publishing requires HEAD to match $upstream after fetching origin/main." >&2
    exit 2
  fi
}

publish_release() {
  local version="$1"
  local snapshot_root
  local package
  local result

  require_publish_checkout
  snapshot_root="$(mktemp -d)"
  trap 'rm -rf -- "$snapshot_root"' EXIT
  git -C "$REPO_ROOT" archive --format=tar HEAD | tar -xf - -C "$snapshot_root"
  verify_snapshot "$snapshot_root" "$version"

  (
    cd "$snapshot_root"
    for package in "${PACKAGES[@]}"; do
      if crate_version_status "$package" "$version"; then
        echo "Verifying existing crate $package $version before resuming."
        CARGO_TARGET_DIR="$snapshot_root/target/plugin-sdk-release" \
          run_cargo package --locked --no-verify --package "$package"
        verify_published_archive "$snapshot_root" "$package" "$version"
        echo "Skipping byte-identical crate $package $version."
        continue
      else
        result=$?
        if [[ "$result" -eq 2 ]]; then
          exit 2
        fi
      fi

      echo "Publishing $package $version."
      CARGO_TARGET_DIR="$snapshot_root/target/plugin-sdk-release" \
        run_cargo publish --locked --registry crates-io --package "$package"
      wait_for_registry "$package" "$version"
      verify_published_archive "$snapshot_root" "$package" "$version"
    done
  )

  check_registry_status "$version"
  rm -rf -- "$snapshot_root"
  trap - EXIT
}

main() {
  local command_name="${1:-}"
  local version

  case "$command_name" in
    verify|status|publish) ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac

  require_command cargo
  require_command curl
  require_command git
  require_command jq
  require_command rustup
  require_command shasum
  require_command tar
  rustup run "$RUST_TOOLCHAIN" cargo --version >/dev/null
  version="$(normalize_version "${2:-}")"

  case "$command_name" in
    verify) verify_release "$version" ;;
    status) check_registry_status "$version" ;;
    publish) publish_release "$version" ;;
  esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
