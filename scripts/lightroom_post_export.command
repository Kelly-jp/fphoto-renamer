#!/bin/bash
set -euo pipefail

# Lightroom post-process helper for fphoto-renamer CLI.
# Lightroom passes one or more exported file/folder paths.

# この .command ファイルが置かれているディレクトリ
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# リポジトリのルートディレクトリ（scripts の1つ上）
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
# 実行する CLI バイナリ（release）
CLI_BIN="${SCRIPT_DIR}/fphoto-renamer-cli"

# ---- Settings (edit as needed) ----
# リネームに使うテンプレート文字列
TEMPLATE="{year}{month}{day}_{hour}{minute}{second}_{camera_maker}_{camera_model}_{lens_maker}_{lens_model}_{film_sim}_{orig_name}"
# 1: RAWフォルダ未指定時に JPG 親フォルダを RAW 探索ルートとして使う / 0: 使わない
USE_RAW_PARENT_IF_MISSING=1
# 1: カメラメーカー名とレンズメーカー名が同じ場合は重複を除去 / 0: 除去しない
DEDUPE_SAME_MAKER=1
# 1: 変換前に backup フォルダへバックアップを作成 / 0: 作成しない
BACKUP_ORIGINALS=0
# ファイル名から削除したい文字列（--exclude として複数指定）
EXCLUDES=(
  "-強化-NR"
  "-DxO_DeepPRIME XD2s_XD"
  "-DxO_DeepPRIME 3"
  "-DxO_DeepPRIME XD3 X-Trans"
)
# ExifTool の実行パスを固定したい場合のみ有効化
# export FPHOTO_EXIFTOOL_PATH="/opt/homebrew/bin/exiftool"
# -----------------------------------

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <exported_path...>"
  exit 2
fi

# Lightroom から渡される引数（書き出し先のファイルまたはフォルダ）
JPG_INPUT_ARGS=()
OPEN_PATH=""
HAS_DIRECTORY_INPUT=0
for INPUT_PATH in "$@"; do
  if [[ -d "${INPUT_PATH}" ]]; then
    if [[ $# -gt 1 ]]; then
      echo "Folder input cannot be combined with other inputs: ${INPUT_PATH}"
      exit 3
    fi
    JPG_INPUT_ARGS+=("--jpg-input" "${INPUT_PATH}")
    OPEN_PATH="${INPUT_PATH}"
    HAS_DIRECTORY_INPUT=1
    continue
  fi

  if [[ -f "${INPUT_PATH}" ]]; then
    if [[ "${HAS_DIRECTORY_INPUT}" -eq 1 ]]; then
      echo "Folder input cannot be combined with file inputs: ${INPUT_PATH}"
      exit 3
    fi
    JPG_INPUT_ARGS+=("--jpg-input" "${INPUT_PATH}")
    if [[ -z "${OPEN_PATH}" ]]; then
      OPEN_PATH="$(dirname "${INPUT_PATH}")"
    fi
    continue
  fi

  echo "Input path not found: ${INPUT_PATH}"
  exit 3
done

if [[ ! -x "${CLI_BIN}" ]]; then
  echo "Release CLI not found. Building..."
  (cd "${PROJECT_ROOT}" && cargo build -p fphoto-renamer-cli --release)
fi

CMD=(
  "${CLI_BIN}"
  rename
  "${JPG_INPUT_ARGS[@]}"
  "--template" "${TEMPLATE}"
  "--output" "table"
  "--apply"
)

if [[ "${USE_RAW_PARENT_IF_MISSING}" -eq 1 ]]; then
  CMD+=("--raw-parent-if-missing")
fi

if [[ "${DEDUPE_SAME_MAKER}" -eq 0 ]]; then
  CMD+=("--dedupe-same-maker=false")
fi

if [[ "${BACKUP_ORIGINALS}" -eq 1 ]]; then
  CMD+=("--backup-originals")
fi

for value in "${EXCLUDES[@]}"; do
  CMD+=("--exclude=${value}")
done

echo "Running:"
printf ' %q' "${CMD[@]}"
echo
echo

"${CMD[@]}"

echo
echo "Done."
#read -r -p "Press Enter to close..."
open "$OPEN_PATH"
