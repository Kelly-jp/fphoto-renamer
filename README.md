# fphoto-renamer

撮影日時・カメラ・レンズなどの情報を使い、JPG/JPEGのファイル名を一括変更するツールです。macOS・Windows向けのGUIと、Linuxでも使えるCLIを提供します。

- ファイル名の書式をテンプレートで指定
- JPGのEXIFに加え、同名のXMP・RAWから情報を取得
- 元ファイルのバックアップと、直近1回のファイル名変更の取り消しに対応
- 必要に応じてContent Credentials（C2PA）を削除

## インストール

配布ファイルは、このリポジトリの **Releases** から取得してください。

| 環境 | GUI | CLI |
| --- | --- | --- |
| macOS | `.dmg` | `fphoto-renamer-cli-macos-universal.tar.gz`（Intel・Apple Silicon共通） |
| Windows | `.msi` | `fphoto-renamer-cli-windows-x86_64.exe` |
| Linux | — | `fphoto-renamer-cli-linux-x86_64.tar.gz` |

GUIインストーラにはExifToolを同梱し、Windows版にはWebView2 Runtimeも同梱します。CLIでExifToolを使う場合は、別途用意してください。ExifToolが見つからない場合、EXIFの読み取りは内蔵ライブラリに切り替わります。ただし、Content Credentialsの削除にはExifToolが必要です。

macOS・LinuxのCLIは、アーカイブを展開して実行します。以下はmacOSの例です（Linuxではファイル名を置き換えてください）。

```bash
tar -xzf fphoto-renamer-cli-macos-universal.tar.gz
./fphoto-renamer-cli --version
```

WindowsではPowerShellから実行できます。

```powershell
.\fphoto-renamer-cli-windows-x86_64.exe --version
```

## GUIの使い方

1. 「JPGフォルダ」を選択します。ドラッグ＆ドロップでも指定できます。
2. 必要に応じて「RAWフォルダ」を指定します。
3. ファイル名の書式を入力し、出力サンプルを確認します。トークンボタンで項目を挿入できます。
4. 必要に応じて削除文字列やバックアップを設定し、「変換」を押します。
5. 直近のファイル名変更を取り消すには「元に戻す」を押します。

GUIの入力はフォルダ単位です。フォルダ欄はクリアでき、削除文字列はチップの `×` で削除できます。

## CLIの使い方

以下はmacOS・Linuxの例です。Windowsでは `./fphoto-renamer-cli` を `.\fphoto-renamer-cli-windows-x86_64.exe` に置き換えてください。パスに空白がある場合は引用符で囲みます。

### 変更内容を確認してから適用する

**既定はdry-runです。`--apply` を付けるまでファイルは変更されません。**

```bash
# 変更予定を表示
./fphoto-renamer-cli rename --jpg-input /path/to/jpg

# 同じ条件で適用（元ファイルもバックアップ）
./fphoto-renamer-cli rename --jpg-input /path/to/jpg --apply --backup-originals

# 直近1回のファイル名変更を取り消す
./fphoto-renamer-cli undo
```

### ファイルや書式を指定する

`--jpg-input` はフォルダまたは単一のJPG/JPEGファイルを受け付けます。複数指定する場合は、フォルダではなくファイルを指定してください。異なるフォルダのファイルもまとめて処理できます。

```bash
./fphoto-renamer-cli rename \
  --jpg-input /path/to/a/DSC00001.JPG \
  --jpg-input /path/to/b/DSC00002.JPG \
  --template "{date}_{camera_model}_{orig_name}"
```

RAWフォルダと削除文字列を指定する例:

```bash
./fphoto-renamer-cli rename \
  --jpg-input /path/to/jpg \
  --raw-input /path/to/raw \
  --template "{date}_{camera_model}_{orig_name}" \
  --exclude "-強化-NR" \
  --exclude "-DxO_DeepPRIME XD3"
```

### 主なオプション

| オプション | 内容 |
| --- | --- |
| `--jpg-input PATH` | 対象のフォルダまたはJPG/JPEGファイル（必須） |
| `--raw-input PATH` | XMP・RAWを探すフォルダ |
| `--raw-parent-if-missing` | RAWフォルダ未指定時、JPGがあるフォルダの1つ上を探索先にする |
| `--template TEXT` | ファイル名の書式 |
| `--exclude TEXT` | ファイル名から削除する文字列（複数指定可） |
| `--dedupe-same-maker=false` | カメラとレンズのメーカー名が同じでも両方残す（既定は重複を省略） |
| `--apply` | 変更を適用する |
| `--backup-originals` | 適用前のファイルを対象JPGフォルダの `backup` に保存する |
| `--remove-content-credentials` | 適用時にContent Credentialsを削除する（ExifTool必須） |
| `--output json` | 変更予定をJSONで出力する（既定は表形式） |

複数ファイルに `--raw-parent-if-missing` を使うと、各ファイルの所在フォルダの1つ上を個別に探索します。共通の探索先を使う場合は `--raw-input` を指定してください。

設定の確認は `./fphoto-renamer-cli config show`、全オプションの確認は `./fphoto-renamer-cli rename --help` で行えます。旧オプションの `--tokens`・`--delimiter` は廃止済みです。

## ファイル名の書式とメタデータ

### テンプレート

例: `{date}_{camera_model}_{orig_name}` → `20260927143000_X-T5_DSC00001.JPG`

拡張子は自動で保持されるため、テンプレートには含めません。

| トークン | 内容 |
| --- | --- |
| `{date}` | 撮影日時（`YYYYMMDDHHMMSS`） |
| `{year}`・`{month}`・`{day}` | 年（4桁）・月・日（各2桁） |
| `{hour}`・`{minute}`・`{second}` | 時・分・秒（各2桁） |
| `{camera_maker}`・`{camera_model}` | カメラのメーカー名・機種名 |
| `{lens_maker}`・`{lens_model}` | レンズのメーカー名・機種名 |
| `{film_sim}` | フィルムシミュレーション |
| `{orig_name}` | 元のファイル名（拡張子を除く） |

既定のテンプレート:

```text
{year}{month}{day}_{hour}{minute}{second}_{camera_maker}_{camera_model}_{lens_maker}_{lens_model}_{film_sim}_{orig_name}
```

- テンプレートに `\ / : * ? " < > |` は使用できません。
- トークン内の空白はハイフンに、テンプレートの区切り用ハイフンはアンダースコアに変換されます。
- 展開後に削除文字列を除去し、残った空白・禁止文字などを正規化します。
- 削除文字列は大文字・小文字を区別せず、空白・ハイフン・アンダースコアの表記揺れも吸収します。
- カメラとレンズのメーカー名が同じ場合、既定ではレンズのメーカー名を省略します。

### メタデータの取得順

RAW探索先から、JPGと同じベース名のXMP・DNG・RAFを探します。RAWはDNGを優先し、次にRAFを使います。

情報の優先順位は **XMP → RAW EXIF → JPG EXIF** です。上位の情報に欠損があれば、下位の情報で補完します。RAW探索先を指定しない場合はJPGのEXIFを使います。

明示したRAWフォルダが存在しない、またはフォルダでない場合はエラーになります。

## バックアップとContent Credentialsの削除

バックアップを有効にすると、適用前のファイルをJPGフォルダ内の `backup` に保存します。

Content Credentials（C2PA / JUMBF）を削除する例:

```bash
./fphoto-renamer-cli rename --jpg-input /path/to/jpg \
  --apply --remove-content-credentials --backup-originals
```

- 削除にはExifToolが必要です。dry-runではメタデータを書き換えません。
- ファイル名が変わらない写真も削除処理の対象です。
- **取り消しで戻るのはファイル名のみです。** 削除前のメタデータを残すにはバックアップを有効にしてください。
- 通常のリネームを取り消すと、その操作で作成したバックアップも削除します。Content Credentialsを削除した操作のバックアップは、取り消し後も保持します。

Lightroomの書き出し後処理には [macOS用スクリプト](scripts/lightroom_post_export.command) / [Windows用スクリプト](scripts/lightroom_post_export.bat) を利用できます。スクリプト内の `REMOVE_CONTENT_CREDENTIALS=1` で削除が有効になります（`0` で無効）。元のメタデータを残す場合は `BACKUP_ORIGINALS=1` に変更してください。

## macOSで起動できない場合

配布版の `.app`・`.dmg` とmacOS CLIにはad-hoc署名を付けていますが、Developer ID署名・公証の代わりにはならないため、Gatekeeperで起動がブロックされる場合があります。

まずmacOSのセキュリティ設定から起動を許可してください。ダウンロード時の `com.apple.quarantine` 属性を削除する場合は、配布元を信頼できることを確認し、対象のパスを指定します。この操作はGatekeeperの保護を回避します。

```bash
# GUIの場合
xattr -dr com.apple.quarantine /Applications/fphoto-renamer.app

# CLIの場合
xattr -dr com.apple.quarantine /path/to/fphoto-renamer-cli
```

旧配布形式（v0.5.0以前）の生バイナリで実行権限がない場合は、`chmod +x /path/to/fphoto-renamer-cli-macos` を実行してください。

## 開発

CLIとGUI（Tauri + HTML/CSS/JavaScript）は共通のRust coreを使います。以下のコマンドは、特記のない限りリポジトリルートで実行します。

### 前提と起動

- Rust stable
- GUI実行環境: macOSはWebKit、WindowsはWebView2 Runtime
- ExifToolを使う場合: PATHに配置するか、`FPHOTO_EXIFTOOL_PATH` に実行ファイルのパスを設定
- `.app`・インストーラの生成: Tauri CLI（`cargo install tauri-cli --locked --version '^2.0'`）
- GUIブラウザUIテスト: Node.js / npm

GUIは同梱ExifToolを自動探索します。ExifToolが見つからない場合は `kamadak-exif` にフォールバックします。

```bash
cargo run -p fphoto-renamer-cli -- rename --jpg-input /path/to/jpg
cargo run -p fphoto-renamer-gui
```

### ビルド

```bash
cargo build --workspace            # Debug
cargo build --workspace --release  # Release
```

Releaseの生成物は `target/release/fphoto-renamer-cli` と `target/release/fphoto-renamer-gui` です（Windowsは `.exe`）。これは実行ファイルのビルドで、インストーラの生成とは別です。

macOSでCLI・GUI実行ファイル・`.app` をad-hoc署名付きでビルドする場合:

```bash
bash scripts/build_macos_signed.sh
```

`.app` は `target/release/bundle/macos/fphoto-renamer.app` に生成されます。既存の生成物の署名には [macos_adhoc_sign.sh](scripts/macos_adhoc_sign.sh)、署名の確認には [macos_codesign_report.sh](scripts/macos_codesign_report.sh) を使います。どちらも対象のパスを引数で指定します。

### テスト

```bash
cargo test --workspace
```

個別に実行する場合は `cargo test -p fphoto_renamer_core` または `cargo test -p fphoto-renamer-cli` を使います。

GUIブラウザUIテスト:

```bash
cd crates/gui
npm ci
npm run test:ui:install-browser
npm run test:ui
```

Playwrightで `crates/gui/dist` をHTTP配信し、画面操作・入力検証・適用や取り消し・エラー表示などを検証します。Tauri本体は起動せず、APIをモックします。ポートを変更する場合は `PLAYWRIGHT_WEB_PORT=4455 npm run test:ui` を使います。

### CI・配布とライセンス

- [Build Installers](.github/workflows/installer.yml): macOS `.dmg`・Windows `.msi` と各OSのCLIを生成します。
- [Build CLI](.github/workflows/cli-build.yml): Linux・macOS・WindowsのCLIを生成します。両ワークフローのCLI artifact名は `cli-Linux` / `cli-macOS` / `cli-Windows` です。
- macOSの `.app`・`.dmg`・CLIはad-hoc署名し、CIで署名状態を検証します。
- インストーラ用ExifToolは [prepare_bundled_exiftool.sh](scripts/prepare_bundled_exiftool.sh) で `crates/gui/src-tauri/resources/bin/<os>/` に配置します。実体は生成物として扱い、Gitには含めません。
- ExifToolを同梱する場合は、[ライセンス文書](crates/gui/src-tauri/resources/LICENSES/EXIFTOOL_LICENSE.txt) も更新・同梱してください。[verify_exiftool_license.sh](scripts/verify_exiftool_license.sh) で実体とライセンス文書の両方を検証します。
