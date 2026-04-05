#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAURI_DIR="${ROOT_DIR}/crates/gui/src-tauri"
TAURI_CONFIG="${TAURI_DIR}/tauri.bundle.macos.adhoc.conf.json"
SIGN_SCRIPT="${ROOT_DIR}/scripts/macos_adhoc_sign.sh"
CLI_BIN="${ROOT_DIR}/target/release/fphoto-renamer-cli"
GUI_BIN="${ROOT_DIR}/target/release/fphoto-renamer-gui"
APP_BUNDLE="${ROOT_DIR}/target/release/bundle/macos/fphoto-renamer.app"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script only supports macOS." >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install Rust stable first." >&2
  exit 1
fi

if ! cargo tauri --version >/dev/null 2>&1; then
  echo "cargo tauri not found. Install it with: cargo install tauri-cli --locked --version '^2.0'" >&2
  exit 1
fi

echo "Building release CLI..."
cargo build -p fphoto-renamer-cli --release --locked
bash "$SIGN_SCRIPT" "$CLI_BIN"

echo "Building signed macOS app bundle..."
(
  cd "$TAURI_DIR"
  cargo tauri build --ci --bundles app --config "$TAURI_CONFIG"
)

bash "$SIGN_SCRIPT" "$GUI_BIN" "$APP_BUNDLE"

cat <<EOF
Signed outputs:
  CLI: ${CLI_BIN}
  GUI binary: ${GUI_BIN}
  App bundle: ${APP_BUNDLE}
EOF
