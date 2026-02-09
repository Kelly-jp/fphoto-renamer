# fphoto-renamer

macOS/Windows 向けの JPG リネームツールです。CLI と Tauri GUI は共通 core を利用します。

## ローカルでのビルド

### 前提

- Rust (stable) がインストール済みであること
- ルートディレクトリ: `fphoto-renamer`
- GUI 実行時:
  - macOS: WebKit が利用可能な通常環境
  - Windows: WebView2 Runtime が利用可能な環境
- EXIF 取得は `exiftool`（`-stay_open`）優先
  - 同梱しない場合: `exiftool` を PATH にインストール
  - 同梱する場合: `crates/gui/src-tauri/resources/bin/<os>/` に配置
  - 同梱する場合: `crates/gui/src-tauri/resources/LICENSES/EXIFTOOL_LICENSE.txt` も必ず同梱

### Debug ビルド

```bash
cargo build --workspace
```

### Release ビルド

```bash
cargo build --workspace --release
```

生成物:

- CLI: `target/release/fphoto-renamer-cli`（Windows は `.exe`）
- GUI: `target/release/fphoto-renamer-gui`（Windows は `.exe`）

## ローカルでの実行

### 開発中に実行（cargo run）

CLI:

```bash
cargo run -p fphoto-renamer-cli -- rename --jpg-input /path/to/jpg
```

GUI:

```bash
cargo run -p fphoto-renamer-gui
```

### ビルド済みバイナリを実行

CLI:

```bash
./target/release/fphoto-renamer-cli rename --jpg-input /path/to/jpg
```

GUI:

```bash
./target/release/fphoto-renamer-gui
```

Windows (PowerShell) の例:

```powershell
.\target\release\fphoto-renamer-cli.exe rename --jpg-input C:\path\to\jpg
.\target\release\fphoto-renamer-gui.exe
```

## 機能

- JPG フォルダ必須、RAW フォルダ任意
- RAW フォルダ指定時は同名ベースで探索し、優先順位は `XMP -> DNG -> RAF`
- RAW フォルダ未指定時に、JPG フォルダの1つ上の階層を RAW 探索ルートにするオプション（CLI/GUI）
- メタデータ取得優先順位: `XMP -> RAW EXIF -> JPG EXIF`
- XMP の欠損項目は RAW EXIF で補完し、さらに不足分は JPG EXIF で補完
- 日付フォーマット: `YYYYMMDDHHMMSS`
- テンプレート入力: 例 `"{year}{month}{day}_{hour}{minute}{second}_{camera_model}_{orig_name}"`
- テンプレートに `\\ / : * ? " < > |` を含む場合はエラー
- `{camera_maker}` と `{lens_maker}` が同じ場合は `{lens_maker}` を空扱い
- 削除文字列リスト（大文字小文字非区別）
- ファイル名の処理順: `テンプレート展開 -> 削除文字列削除 -> スペースをアンダースコアへ正規化 -> 禁止文字正規化`
- 削除文字列はスペース/ハイフン/アンダースコアの揺れを吸収して削除
- Windows/macOS 禁止文字の正規化
- GUI の「バックアップ」チェックONで、適用時に `JPGフォルダ/backup` へ元ファイルをバックアップ
- GUI はフォルダ選択・ドラッグ＆ドロップ・クリアボタンに対応
- dry-run 既定、`--apply` で適用
- 直近1回の undo

## CLI

`--tokens` / `--delimiter` は廃止済みです。`--template` を使用してください。

```bash
cargo run -p fphoto-renamer-cli -- rename \
  --jpg-input /path/to/jpg \
  --raw-input /path/to/raw \
  --template "{year}{month}{day}_{hour}{minute}{second}_{camera_maker}_{camera_model}_{lens_maker}_{lens_model}_{film_sim}_{orig_name}" \
  --exclude "-強化-NR" \
  --exclude "-DxO_DeepPRIME XD2s_XD"

```

RAW フォルダを省略し、JPG 親フォルダを RAW 探索ルートとして使う場合:

```bash
cargo run -p fphoto-renamer-cli -- rename \
  --jpg-input /path/to/jpg \
  --raw-parent-if-missing
```

適用する場合:

```bash
cargo run -p fphoto-renamer-cli -- rename --jpg-input /path/to/jpg --apply
```

取り消し:

```bash
cargo run -p fphoto-renamer-cli -- undo
```

## GUI

```bash
cargo run -p fphoto-renamer-gui
```

GUI では書式テキストを入力し、トークンボタンでカーソル位置へ挿入できます。
出力サンプルはリアルタイム表示されます。
JPG/RAW フォルダは「選択」「ドラッグ＆ドロップ」「クリア」で設定できます。
削除文字列はチップとして管理し、`×` ボタンで削除できます。
GUI は Tauri + HTML/CSS/JavaScript で実装しています。

## テスト実行コマンド

リポジトリルートで実行:

```bash
# core のユニットテスト
cargo test -p fphoto_renamer_core

# CLI のユニットテスト
cargo test -p fphoto-renamer-cli

# Rust テストをまとめて実行
cargo test --workspace
```

GUI ブラウザUIテスト:

```bash
cd crates/gui
npm install
npm run test:ui:install-browser
npm run test:ui
```

## GUI ブラウザUIテスト (Playwright)

前提:

- Node.js / npm が利用可能
- Playwright の Chromium をインストール済み（`npm run test:ui:install-browser`）

実行:

```bash
cd crates/gui
npm install
npm run test:ui
```

補足:

- `crates/gui/dist` をローカルHTTP配信してテストします（Tauri本体は起動しません）。
- `window.__TAURI__` API はテスト内でモックしており、画面ロジックをブラウザ単体で検証します。
- HTTPポートを変更する場合は `PLAYWRIGHT_WEB_PORT=4455 npm run test:ui` を利用します。
- 主要な検証対象: 初期描画、変換成功（複数候補含む）、削除文字列の重複除外、テンプレート検証エラー、適用失敗、undo成功/失敗、フォルダ選択（成功/キャンセル/失敗）、JPGクリア、設定保存デバウンス、設定保存失敗、ドロップ反映、禁止文字サニタイズ、適用payload、テンプレートリセット、サンプル生成失敗。

## ExifTool の指定

- 環境変数 `FPHOTO_EXIFTOOL_PATH` を設定すると、その実行ファイルを優先使用します。
- GUI では同梱リソースを自動探索し、見つかった場合に `FPHOTO_EXIFTOOL_PATH` を自動設定します。
- 同梱も PATH も見つからない場合は、`kamadak-exif` にフォールバックします。

## ExifTool 同梱時のライセンス対応

- ExifTool 同梱時は `crates/gui/src-tauri/resources/LICENSES/EXIFTOOL_LICENSE.txt` を更新・同梱してください。
- CI では `scripts/verify_exiftool_license.sh` により、同梱バイナリがある場合にライセンス文書の存在を検証します。
