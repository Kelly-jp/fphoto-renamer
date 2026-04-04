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

sign_target() {
  local target="$1"

  if [[ ! -e "$target" ]]; then
    echo "Target not found: $target" >&2
    exit 1
  fi

  echo "Ad-hoc signing: $target"

  if [[ -d "$target" && "$target" == *.app ]]; then
    codesign --force --deep --sign - --timestamp=none "$target"
    codesign --verify --deep --verbose=2 "$target"
    return
  fi

  codesign --force --sign - --timestamp=none "$target"
  codesign --verify --verbose=2 "$target"
}

for target in "$@"; do
  sign_target "$target"
done
