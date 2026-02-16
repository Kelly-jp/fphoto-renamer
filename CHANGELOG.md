# fphoto-renamer リリースノート

## v0.4.1 (2026-02-16)

### 変更点

- GitHub Release の `v0.4.0` で発生した immutable release 競合に伴い、リリースタグを `v0.4.1` に更新
- アプリ本体の機能変更はなし

## v0.4.0 (2026-02-16)

### 変更点

- CLI の `rename --jpg-input` が、フォルダに加えて単一JPG/JPEGファイル入力にも対応
- 単一ファイル指定時はその1枚のみをリネーム対象にし、非JPGファイルは明示エラーで拒否
- 単一ファイル指定時も `--raw-parent-if-missing` が利用でき、対象JPGの親フォルダの1つ上を RAW 探索ルートとして解決
- Lightroom 後処理スクリプト（`.bat` / `.command`）が、受け取ったファイル/フォルダのパスをそのまま CLI へ渡すよう更新
- README に単一JPG/JPEGファイル入力の実行例を追記

## v0.3.1 (2026-02-11)

### 変更点

- CLI で ExifTool の実行パス自動検出を追加し、`film_sim` 取得の安定性を改善
- Lightroom 後処理スクリプトで、リネーム完了後に JPG フォルダを Finder で開くよう修正

## v0.3.0 (2026-02-11)

### 変更点

- CLI に `--version` / `-V` オプションを追加
- README に macOS の `com.apple.quarantine` 属性による起動ブロック時の対処方法を追記
- タグ push 時に GUI/CLI の成果物を GitHub Release assets として自動添付する CI を追加
- アプリのバージョン表記を `0.3.0` に更新

## v0.2.0 (2026-02-11)

JPG 写真のファイル名を、撮影メタデータとテンプレートを使って一括整形・リネームできます。CLI と GUI の両方を提供し、同じコアロジックで動作します。

### 主な機能

- テンプレートベースの一括リネーム
  - 例: `{year}{month}{day}_{hour}{minute}{second}_{camera_model}_{orig_name}`
- メタデータ参照の優先順位
  - `XMP -> RAW EXIF -> JPG EXIF`
  - 欠損項目は上記順で補完
- RAW 探索ロジック
  - RAW フォルダ指定時: 同名ベースで `XMP -> DNG -> RAF` の順に探索
  - RAW フォルダ未指定時: JPG 親フォルダを RAW 探索ルートにできるオプションを提供
- 文字列整形と安全化
  - 削除文字列（大文字小文字非区別、区切り揺れ吸収）に対応
  - スペースのアンダースコア正規化
  - Windows/macOS 禁止文字の正規化
- 安全な実行フロー
  - 既定は dry-run（プレビューのみ）
  - 明示的な適用（`--apply` / GUI の適用操作）で実リネーム
  - 直近 1 回の undo に対応
- GUI 機能
  - フォルダ選択、ドラッグ＆ドロップ、クリア操作
  - テンプレートトークン挿入とリアルタイム出力サンプル
  - バックアップオプション（適用時に `JPGフォルダ/backup` へ退避）

### CLI 追加コマンド

- `rename`: リネーム計画の生成・表示（`--apply` 指定で適用）
- `undo`: 直近の適用を取り消し
- `config show`: 現在の設定表示

### 対応環境

- GUI: macOS / Windows
- CLI: Linux / macOS / Windows

### 配布と依存同梱

- 配布用 GUI インストーラ（macOS/Windows）は ExifTool を同梱
- Windows MSI は WebView2 Runtime をオフライン同梱
- ExifTool が利用できない環境では `kamadak-exif` にフォールバック

### 既知の制約

- リネーム対象は JPG（JPEG）ファイル
- undo は直近 1 回のみ対応
- RAW フォルダを明示指定した場合、無効パス時はエラー（JPG 側への自動フォールバックなし）
