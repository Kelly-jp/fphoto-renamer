#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script only supports macOS." >&2
  exit 1
fi

if ! command -v codesign >/dev/null 2>&1; then
  echo "codesign not found. Install Xcode Command Line Tools first." >&2
  exit 1
fi

if [[ "$#" -eq 0 ]]; then
  echo "Usage: $0 <path> [<path> ...]" >&2
  exit 1
fi

report_target() {
  local target="$1"

  if [[ ! -e "$target" ]]; then
    echo "Target not found: $target" >&2
    exit 1
  fi

  echo "=== codesign report: $target ==="
  ls -ld "$target"
  file "$target"

  if [[ -f "$target" ]] && command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$target"
  fi

  codesign --display --verbose=4 "$target"

  if [[ -d "$target" && "$target" == *.app ]]; then
    codesign --verify --deep --verbose=2 "$target"
  else
    codesign --verify --verbose=2 "$target"
  fi
}

for target in "$@"; do
  report_target "$target"
done
