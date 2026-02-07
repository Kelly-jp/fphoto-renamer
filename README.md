# fphoto-renamer

macOS/Windows 向けの JPG リネームツールです。CLI と Tauri GUI は共通 core を利用します。

## ローカルでのビルド

### 前提

- Rust (stable) がインストール済みであること
- ルートディレクトリ: `fphoto-renamer`
- GUI 実行時:
  - macOS: WebKit が利用可能な通常環境
  - Windows: WebView2 Runtime が利用可能な環境

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
- メタデータ取得優先順位: `XMP -> RAW EXIF -> JPG EXIF`
- 日付フォーマット: `YYYYMMDDHHMMSS`
- テンプレート入力: 例 `"{date}_{camera_model}_{orig_name}"`
- `{camera_make}` と `{lens_make}` が同じ場合は `{lens_make}` を空扱い
- 除外文字列リスト（大文字小文字非区別）
- Windows/macOS 禁止文字の正規化
- dry-run 既定、`--apply` で適用
- 直近1回の undo

## CLI

```bash
cargo run -p fphoto-renamer-cli -- rename \
  --jpg-input /path/to/jpg \
  --raw-input /path/to/raw \
  --template "{date}_{camera_model}_{orig_name}" \
  --exclude "FUJIFILM"
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
GUI は Tauri + HTML/CSS/JavaScript で実装しています。
