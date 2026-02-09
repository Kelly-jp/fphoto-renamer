#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <os-name>" >&2
  echo "example: $0 macos-latest" >&2
  exit 2
fi

os_name="$1"
license_file="crates/gui/src-tauri/resources/LICENSES/EXIFTOOL_LICENSE.txt"
exiftool_lib_marker=""

case "$os_name" in
  macos | macos-latest)
    exiftool_path="crates/gui/src-tauri/resources/bin/macos/exiftool"
    exiftool_lib_marker="crates/gui/src-tauri/resources/bin/macos/lib/Image/ExifTool.pm"
    ;;
  windows | windows-latest)
    exiftool_path="crates/gui/src-tauri/resources/bin/windows/exiftool.exe"
    exiftool_lib_marker="crates/gui/src-tauri/resources/bin/windows/exiftool_files/exiftool.pl"
    ;;
  linux | ubuntu-latest)
    exiftool_path="crates/gui/src-tauri/resources/bin/linux/exiftool"
    exiftool_lib_marker="crates/gui/src-tauri/resources/bin/linux/lib/Image/ExifTool.pm"
    ;;
  *)
    echo "unsupported os name: $os_name" >&2
    exit 2
    ;;
esac

if [[ ! -f "$exiftool_path" ]]; then
  echo "Bundled ExifTool binary is missing for ${os_name}: $exiftool_path" >&2
  exit 1
fi

if [[ "$os_name" != windows && "$os_name" != windows-latest ]] && [[ ! -x "$exiftool_path" ]]; then
  echo "Bundled ExifTool binary is not executable for ${os_name}: $exiftool_path" >&2
  exit 1
fi

if [[ -n "$exiftool_lib_marker" ]] && [[ ! -f "$exiftool_lib_marker" ]]; then
  echo "Bundled ExifTool runtime library is missing for ${os_name}: $exiftool_lib_marker" >&2
  exit 1
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
