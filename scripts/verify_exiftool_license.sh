#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <os-name>" >&2
  echo "example: $0 macos-latest" >&2
  exit 2
fi

os_name="$1"
license_file="crates/gui/src-tauri/resources/LICENSES/EXIFTOOL_LICENSE.txt"

case "$os_name" in
  macos | macos-latest)
    exiftool_path="crates/gui/src-tauri/resources/bin/macos/exiftool"
    ;;
  windows | windows-latest)
    exiftool_path="crates/gui/src-tauri/resources/bin/windows/exiftool.exe"
    ;;
  linux | ubuntu-latest)
    exiftool_path="crates/gui/src-tauri/resources/bin/linux/exiftool"
    ;;
  *)
    echo "unsupported os name: $os_name" >&2
    exit 2
    ;;
esac

if [[ ! -f "$exiftool_path" ]]; then
  echo "No bundled ExifTool binary found for ${os_name}; skip license bundle check."
  exit 0
fi

if [[ ! -s "$license_file" ]]; then
  echo "Bundled ExifTool binary exists, but license file is missing or empty: $license_file" >&2
  exit 1
fi

if ! grep -qi "exiftool" "$license_file"; then
  echo "License file does not appear to mention ExifTool: $license_file" >&2
  exit 1
fi

echo "ExifTool bundle license check passed."
