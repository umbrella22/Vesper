#!/usr/bin/env bash
#
# Keep the Rust build toolchain pin and the workspace MSRV in sync.
#
# The build toolchain is pinned in rust-toolchain.toml. The MSRV is declared by
# rust-version in the workspace Cargo.toml and is inherited by generated plugin
# scaffolds at build time. First-party plugin manifests under plugins/ are
# standalone packages, so they carry their own copy of the MSRV.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly SCRIPT_DIR REPO_ROOT

readonly TOOLCHAIN_FILE="$REPO_ROOT/rust-toolchain.toml"
readonly WORKSPACE_MANIFEST="$REPO_ROOT/Cargo.toml"
readonly SCAFFOLD_ASSETS="$REPO_ROOT/crates/tools/player-cli/src/plugin_scaffold_assets.rs"

GENERATED_TEMPLATES=(
  "$REPO_ROOT/templates/vesper-plugin/native/Cargo.toml.template"
  "$REPO_ROOT/templates/vesper-plugin/wasm/Cargo.toml.template"
)
readonly GENERATED_TEMPLATES

PLUGIN_MANIFESTS=(
  "$REPO_ROOT/plugins/audio-processor-diagnostic/Cargo.toml"
  "$REPO_ROOT/plugins/wasm-observer-diagnostic/Cargo.toml"
)
readonly PLUGIN_MANIFESTS

usage() {
  cat <<'EOF'
Usage:
  scripts/bump-rust-version.sh bump <version> [--msrv]
  scripts/bump-rust-version.sh msrv <version>
  scripts/bump-rust-version.sh check

Commands:
  bump <version>   Pin the build toolchain in rust-toolchain.toml. With
                   --msrv, also raise the workspace and plugin MSRVs to the
                   same version.
  msrv <version>   Update the workspace and first-party plugin MSRVs only.
  check            Verify every pin agrees and that the build toolchain is not
                   older than the workspace MSRV.

The build toolchain and the MSRV are separate decisions. Bump the toolchain to
pick up compiler fixes or new stable releases; raise the MSRV only when a
language, library, or dependency requirement needs it.

Examples:
  scripts/bump-rust-version.sh bump 1.100.0
  scripts/bump-rust-version.sh bump 1.100.0 --msrv
  scripts/bump-rust-version.sh msrv 1.100.0
  scripts/bump-rust-version.sh check
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

toolchain_version() {
  sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$TOOLCHAIN_FILE"
}

workspace_msrv() {
  awk '
    /^\[workspace\.package\][[:space:]]*$/ { in_section = 1; next }
    /^\[/ { in_section = 0 }
    in_section && /^[[:space:]]*rust-version[[:space:]]*=/ {
      line = $0
      sub(/^[^"]*"/, "", line)
      sub(/".*$/, "", line)
      print line
      exit
    }
  ' "$WORKSPACE_MANIFEST"
}

declared_msrv() {
  sed -n 's/^[[:space:]]*rust-version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$1"
}

validate_version() {
  local value="$1"
  local label="$2"
  if [[ ! "$value" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    die "$label must be a numeric version such as 1.100.0 (got '$value')"
  fi
}

version_ge() {
  local left="$1"
  local right="$2"
  local IFS=.
  local -a left_parts=($left)
  local -a right_parts=($right)
  local index
  for index in 0 1 2; do
    if ((10#${left_parts[index]:-0} > 10#${right_parts[index]:-0})); then
      return 0
    fi
    if ((10#${left_parts[index]:-0} < 10#${right_parts[index]:-0})); then
      return 1
    fi
  done
  return 0
}

require_single_match() {
  local file="$1"
  local pattern="$2"
  local count
  count="$(grep -c -E "$pattern" "$file" || true)"
  if [[ "$count" != "1" ]]; then
    die "${file#"$REPO_ROOT"/} must contain exactly one line matching '$pattern' (found $count)"
  fi
}

replace_single_line() {
  local file="$1"
  local pattern="$2"
  local replacement="$3"
  local tmp
  tmp="$(mktemp "${TMPDIR:-/tmp}/vesper-rust-version.XXXXXX")"
  if ! awk -v pattern="$pattern" -v replacement="$replacement" '
    $0 ~ pattern {
      match($0, /^[[:space:]]*/)
      prefix = substr($0, 1, RLENGTH)
      print prefix replacement
      replaced++
      next
    }
    { print }
    END { if (replaced != 1) exit 1 }
  ' "$file" >"$tmp"; then
    rm -f "$tmp"
    die "failed to update ${file#"$REPO_ROOT"/}"
  fi
  cat "$tmp" >"$file"
  rm -f "$tmp"
}

check_pins() {
  local failures=0
  local toolchain
  local msrv
  local value
  local file

  toolchain="$(toolchain_version)"
  msrv="$(workspace_msrv)"

  if [[ -z "$toolchain" ]]; then
    echo "error: rust-toolchain.toml does not declare a channel" >&2
    failures=$((failures + 1))
  elif [[ ! "$toolchain" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: rust-toolchain.toml must pin a numeric version (got '$toolchain')" >&2
    failures=$((failures + 1))
  fi

  if [[ -z "$msrv" ]]; then
    echo "error: Cargo.toml does not declare workspace.package.rust-version" >&2
    failures=$((failures + 1))
  elif [[ ! "$msrv" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: workspace rust-version must be numeric (got '$msrv')" >&2
    failures=$((failures + 1))
  fi

  if [[ -n "$toolchain" && -n "$msrv" ]] && ! version_ge "$toolchain" "$msrv"; then
    echo "error: build toolchain $toolchain is older than workspace MSRV $msrv" >&2
    failures=$((failures + 1))
  fi

  for file in "${PLUGIN_MANIFESTS[@]}"; do
    value="$(declared_msrv "$file")"
    if [[ "$value" != "$msrv" ]]; then
      echo "error: ${file#"$REPO_ROOT"/} declares rust-version '$value', expected '$msrv'" >&2
      failures=$((failures + 1))
    fi
  done

  for file in "${GENERATED_TEMPLATES[@]}"; do
    if ! grep -q 'rust-version = "{{RUST_VERSION}}"' "$file"; then
      echo "error: ${file#"$REPO_ROOT"/} must use the {{RUST_VERSION}} placeholder" >&2
      failures=$((failures + 1))
    fi
  done

  if ! grep -q 'rust-version = "{{RUST_VERSION}}"' "$SCAFFOLD_ASSETS"; then
    echo "error: ${SCAFFOLD_ASSETS#"$REPO_ROOT"/} must use the {{RUST_VERSION}} placeholder" >&2
    failures=$((failures + 1))
  fi

  if grep -R -n -E 'dtolnay/rust-toolchain@[0-9]' "$REPO_ROOT/.github" >/dev/null 2>&1; then
    echo "error: workflows must use ./.github/actions/setup-rust instead of a direct numeric toolchain ref:" >&2
    grep -R -n -E 'dtolnay/rust-toolchain@[0-9]' "$REPO_ROOT/.github" >&2 || true
    failures=$((failures + 1))
  fi

  if ((failures > 0)); then
    echo "Rust version pins are inconsistent ($failures problem(s))." >&2
    return 1
  fi

  echo "Rust version pins are consistent."
  echo "  build toolchain:  $toolchain (rust-toolchain.toml)"
  echo "  workspace MSRV:   $msrv (Cargo.toml)"
  echo "  plugin manifests: $msrv (${#PLUGIN_MANIFESTS[@]} files)"
}

bump_toolchain() {
  local version="$1"
  local msrv
  validate_version "$version" "toolchain version"
  msrv="$(workspace_msrv)"
  validate_version "$msrv" "workspace MSRV"
  if ! version_ge "$version" "$msrv"; then
    die "toolchain $version is older than workspace MSRV $msrv; raise the MSRV first or pass --msrv"
  fi
  require_single_match "$TOOLCHAIN_FILE" '^[[:space:]]*channel[[:space:]]*='
  replace_single_line "$TOOLCHAIN_FILE" '^[[:space:]]*channel[[:space:]]*=' "channel = \"$version\""
  echo "Updated rust-toolchain.toml channel to $version."
}

bump_msrv() {
  local version="$1"
  local toolchain
  local file

  validate_version "$version" "MSRV"
  toolchain="$(toolchain_version)"
  validate_version "$toolchain" "toolchain version"
  if ! version_ge "$toolchain" "$version"; then
    die "workspace MSRV $version is newer than the pinned toolchain $toolchain; bump the toolchain first or pass --msrv"
  fi

  require_single_match "$WORKSPACE_MANIFEST" '^[[:space:]]*rust-version[[:space:]]*='
  for file in "${PLUGIN_MANIFESTS[@]}"; do
    require_single_match "$file" '^[[:space:]]*rust-version[[:space:]]*='
  done

  replace_single_line "$WORKSPACE_MANIFEST" '^[[:space:]]*rust-version[[:space:]]*=' "rust-version = \"$version\""
  for file in "${PLUGIN_MANIFESTS[@]}"; do
    replace_single_line "$file" '^[[:space:]]*rust-version[[:space:]]*=' "rust-version = \"$version\""
  done
  echo "Updated workspace and first-party plugin MSRVs to $version."
}

main() {
  local command="${1:-}"

  case "$command" in
    bump)
      shift
      local version=""
      local msrv_requested=0
      while (($# > 0)); do
        case "$1" in
          --msrv)
            msrv_requested=1
            shift
            ;;
          -h | --help)
            usage
            exit 0
            ;;
          -*)
            die "unknown option: $1"
            ;;
          *)
            if [[ -n "$version" ]]; then
              die "unexpected argument: $1"
            fi
            version="$1"
            shift
            ;;
        esac
      done
      if [[ -z "$version" ]]; then
        die "bump requires a version"
      fi
      bump_toolchain "$version"
      if ((msrv_requested)); then
        bump_msrv "$version"
      fi
      check_pins
      ;;
    msrv)
      if [[ $# -ne 2 ]]; then
        die "msrv requires exactly one version"
      fi
      bump_msrv "$2"
      check_pins
      ;;
    check | --check)
      check_pins
      ;;
    "" | -h | --help | help)
      usage
      ;;
    *)
      die "unknown command: $command"
      ;;
  esac
}

main "$@"
