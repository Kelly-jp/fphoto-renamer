#!/bin/bash
set -euo pipefail

# Lightroom post-process helper for fphoto-renamer CLI.
# Lightroom passes an exported folder path as the first argument.

# この .command ファイルが置かれているディレクトリ
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# リポジトリのルートディレクトリ（scripts の1つ上）
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
# 実行する CLI バイナリ（release）
CLI_BIN="${PROJECT_ROOT}/target/release/fphoto-renamer-cli"

# ---- Settings (edit as needed) ----
# リネームに使うテンプレート文字列
TEMPLATE="{year}{month}{day}_{hour}{minute}{second}_{camera_maker}_{camera_model}_{lens_maker}_{lens_model}_{film_sim}_{orig_name}"
# 1: RAWフォルダ未指定時に JPG 親フォルダを RAW 探索ルートとして使う / 0: 使わない
USE_RAW_PARENT_IF_MISSING=1
# 1: カメラメーカー名とレンズメーカー名が同じ場合は重複を除去 / 0: 除去しない
DEDUPE_SAME_MAKER=1
# 1: 変換前に backup フォルダへバックアップを作成 / 0: 作成しない
BACKUP_ORIGINALS=1
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
  echo "Usage: $0 <exported_folder>"
  exit 2
fi

# Lightroom から渡される引数（書き出し先のファイルまたはフォルダ）
INPUT_PATH="$1"
if [[ -f "${INPUT_PATH}" ]]; then
  # ファイルが渡された場合は、その親フォルダを JPG 入力にする
  JPG_INPUT="$(dirname "${INPUT_PATH}")"
else
  # フォルダが渡された場合は、そのまま JPG 入力にする
  JPG_INPUT="${INPUT_PATH}"
fi
JPG_INPUT="${JPG_INPUT%/}"

if [[ ! -d "${JPG_INPUT}" ]]; then
  echo "Input folder not found: ${JPG_INPUT}"
  exit 3
fi

if [[ ! -x "${CLI_BIN}" ]]; then
  echo "Release CLI not found. Building..."
  (cd "${PROJECT_ROOT}" && cargo build -p fphoto-renamer-cli --release)
fi

CMD=(
  "${CLI_BIN}"
  rename
  "--jpg-input" "${JPG_INPUT}"
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
open "$INPUT_PATH"
