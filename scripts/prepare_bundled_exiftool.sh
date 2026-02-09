#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF' >&2
usage: scripts/prepare_bundled_exiftool.sh <os-name> [exiftool-version]

examples:
  scripts/prepare_bundled_exiftool.sh macos-latest 13.50
  scripts/prepare_bundled_exiftool.sh windows-latest
EOF
}

normalize_os_name() {
  local os_name="$1"
  case "$os_name" in
    macos | macos-latest)
      echo "macos"
      ;;
    windows | windows-latest)
      echo "windows"
      ;;
    linux | ubuntu-latest)
      echo "linux"
      ;;
    *)
      echo "unsupported os name: ${os_name}" >&2
      exit 2
      ;;
  esac
}

resolve_exiftool_version() {
  local explicit_version="${1:-}"
  if [[ -n "$explicit_version" ]]; then
    echo "$explicit_version"
    return
  fi

  if [[ -n "${EXIFTOOL_VERSION:-}" ]]; then
    echo "$EXIFTOOL_VERSION"
    return
  fi

  curl -fsSL https://exiftool.org/ver.txt | tr -d '\r\n'
}

clean_bundle_dir() {
  local bundle_dir="$1"
  mkdir -p "$bundle_dir"
  shopt -s dotglob nullglob
  local entry
  for entry in "$bundle_dir"/*; do
    if [[ "$(basename "$entry")" == "README.md" ]]; then
      continue
    fi
    chmod -R u+w "$entry" 2>/dev/null || true
    rm -rf "$entry"
  done
  shopt -u dotglob nullglob
}

cleanup_temp_dir() {
  local dir_path="$1"
  if [[ ! -d "$dir_path" ]]; then
    return
  fi
  chmod -R u+w "$dir_path" 2>/dev/null || true
  rm -rf "$dir_path" 2>/dev/null || true
}

extract_zip_archive() {
  local archive_path="$1"
  local dest_dir="$2"

  if command -v unzip >/dev/null 2>&1; then
    unzip -q "$archive_path" -d "$dest_dir"
    return
  fi

  if command -v bsdtar >/dev/null 2>&1; then
    bsdtar -xf "$archive_path" -C "$dest_dir"
    return
  fi

  tar -xf "$archive_path" -C "$dest_dir"
}

prepare_unix_bundle() {
  local os_name="$1"
  local version="$2"
  local repo_root="$3"
  local destination_dir="$repo_root/crates/gui/src-tauri/resources/bin/${os_name}"
  local archive_url="https://exiftool.org/Image-ExifTool-${version}.tar.gz"
  local work_dir
  work_dir="$(mktemp -d)"

  clean_bundle_dir "$destination_dir"

  curl -fsSL "$archive_url" -o "$work_dir/exiftool.tar.gz"
  tar -xzf "$work_dir/exiftool.tar.gz" -C "$work_dir"

  local extracted_dir
  extracted_dir="$(find "$work_dir" -mindepth 1 -maxdepth 1 -type d -name 'Image-ExifTool-*' | head -n 1)"
  if [[ -z "$extracted_dir" ]]; then
    echo "failed to locate extracted ExifTool directory for ${os_name}" >&2
    cleanup_temp_dir "$work_dir"
    exit 1
  fi

  if [[ ! -f "$extracted_dir/exiftool" ]]; then
    echo "missing exiftool executable in archive for ${os_name}" >&2
    cleanup_temp_dir "$work_dir"
    exit 1
  fi

  if [[ ! -d "$extracted_dir/lib" ]]; then
    echo "missing lib directory in archive for ${os_name}" >&2
    cleanup_temp_dir "$work_dir"
    exit 1
  fi

  cp "$extracted_dir/exiftool" "$destination_dir/exiftool"
  cp -R "$extracted_dir/lib" "$destination_dir/lib"
  chmod +x "$destination_dir/exiftool"
  cleanup_temp_dir "$work_dir"
}

prepare_windows_bundle() {
  local version="$1"
  local repo_root="$2"
  local destination_dir="$repo_root/crates/gui/src-tauri/resources/bin/windows"
  local archive_url="https://exiftool.org/exiftool-${version}_64.zip"
  local work_dir
  work_dir="$(mktemp -d)"

  clean_bundle_dir "$destination_dir"

  curl -fsSL "$archive_url" -o "$work_dir/exiftool.zip"
  extract_zip_archive "$work_dir/exiftool.zip" "$work_dir"

  local source_file
  source_file="$(find "$work_dir" -type f \( -name 'exiftool(-k).exe' -o -name 'exiftool.exe' \) | head -n 1)"
  if [[ -z "$source_file" ]]; then
    echo "failed to locate Windows ExifTool executable in archive" >&2
    cleanup_temp_dir "$work_dir"
    exit 1
  fi

  local runtime_dir
  runtime_dir="$(find "$work_dir" -type d -name 'exiftool_files' | head -n 1)"
  if [[ -z "$runtime_dir" ]]; then
    echo "failed to locate Windows ExifTool runtime directory in archive" >&2
    cleanup_temp_dir "$work_dir"
    exit 1
  fi

  cp "$source_file" "$destination_dir/exiftool.exe"
  cp -R "$runtime_dir" "$destination_dir/exiftool_files"
  cleanup_temp_dir "$work_dir"
}

if [[ $# -lt 1 ]]; then
  usage
  exit 2
fi

target_os="$(normalize_os_name "$1")"
exiftool_version="$(resolve_exiftool_version "${2:-}")"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "Preparing bundled ExifTool ${exiftool_version} for ${target_os}..."

case "$target_os" in
  macos | linux)
    prepare_unix_bundle "$target_os" "$exiftool_version" "$repo_root"
    ;;
  windows)
    prepare_windows_bundle "$exiftool_version" "$repo_root"
    ;;
esac

echo "Bundled ExifTool prepared for ${target_os}."
