同梱する `exiftool` のライセンス文書をこのディレクトリに配置してください。

このリポジトリでは、以下のファイルを同梱対象にしています。

- `EXIFTOOL_LICENSE.txt`

`resources/bin/<os>/exiftool`（または `exiftool.exe`）を配置した場合は、
必ず上記ファイルを最新のライセンス情報に更新してください。

CI では `scripts/prepare_bundled_exiftool.sh` で同梱素材を生成し、
`scripts/verify_exiftool_license.sh` で同梱バイナリとライセンス文書の存在を必須検証します。
