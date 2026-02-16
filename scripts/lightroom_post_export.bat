@echo off
setlocal EnableExtensions EnableDelayedExpansion

rem Lightroom 後処理用: 1つ以上の書き出し先（ファイルまたはフォルダ）が渡される想定

rem この .bat ファイルが置かれているディレクトリ
set "SCRIPT_DIR=%~dp0"
if "%SCRIPT_DIR:~-1%"=="\" set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"

rem リポジトリのルートディレクトリ（scripts の1つ上）
for %%I in ("%SCRIPT_DIR%\..") do set "PROJECT_ROOT=%%~fI"

rem 実行する CLI バイナリ（release）
set "CLI_BIN=%PROJECT_ROOT%\target\release\fphoto-renamer-cli.exe"

rem ---- 設定（必要に応じて編集） ----
rem リネームに使うテンプレート文字列
set "TEMPLATE={year}{month}{day}_{hour}{minute}{second}_{camera_maker}_{camera_model}_{lens_maker}_{lens_model}_{film_sim}_{orig_name}"

rem 1: RAWフォルダ未指定時に JPG 親フォルダを RAW 探索ルートとして使う / 0: 使わない
set "USE_RAW_PARENT_IF_MISSING=1"

rem 1: カメラメーカー名とレンズメーカー名が同じ場合は重複を除去 / 0: 除去しない
set "DEDUPE_SAME_MAKER=1"

rem 1: 変換前に backup フォルダへバックアップを作成 / 0: 作成しない
set "BACKUP_ORIGINALS=0"

rem ファイル名から削除したい文字列（--exclude として複数指定）
set "EXCLUDE1=-強化-NR"
set "EXCLUDE2=-DxO_DeepPRIME XD2s_XD"
set "EXCLUDE3=-DxO_DeepPRIME 3"
set "EXCLUDE4=-DxO_DeepPRIME XD3 X-Trans"

rem ExifTool の実行パスを固定したい場合のみ有効化
rem set "FPHOTO_EXIFTOOL_PATH=C:\tools\exiftool\exiftool.exe"
rem -----------------------------------

if "%~1"=="" (
  echo Usage: %~nx0 ^<exported_path...^>
  exit /b 2
)

rem Lightroom から渡される引数（書き出し先ファイル/フォルダ）
set "JPG_INPUT_ARGS="
set "OPEN_PATH="
set "HAS_DIRECTORY_INPUT=0"
set "INPUT_COUNT=0"

:collect_inputs
if "%~1"=="" goto inputs_ready

set /a INPUT_COUNT+=1
set "INPUT_PATH=%~1"

if exist "%INPUT_PATH%\*" (
  if not "!INPUT_COUNT!"=="1" (
    echo Folder input cannot be combined with other inputs: "%INPUT_PATH%"
    exit /b 3
  )
  set "HAS_DIRECTORY_INPUT=1"
  set "JPG_INPUT_ARGS=!JPG_INPUT_ARGS! --jpg-input ""%INPUT_PATH%"""
  set "OPEN_PATH=%INPUT_PATH%"
) else (
  if exist "%INPUT_PATH%" (
    if "!HAS_DIRECTORY_INPUT!"=="1" (
      echo Folder input cannot be combined with file inputs: "%INPUT_PATH%"
      exit /b 3
    )
    set "JPG_INPUT_ARGS=!JPG_INPUT_ARGS! --jpg-input ""%INPUT_PATH%"""
    if not defined OPEN_PATH for %%I in ("%INPUT_PATH%") do set "OPEN_PATH=%%~dpI"
  ) else (
    echo Input path not found: "%INPUT_PATH%"
    exit /b 3
  )
)

shift
goto collect_inputs

:inputs_ready
if not exist "%CLI_BIN%" (
  echo Release CLI not found. Building...
  pushd "%PROJECT_ROOT%" || exit /b 4
  cargo build -p fphoto-renamer-cli --release
  if errorlevel 1 (
    popd
    exit /b 5
  )
  popd
)

set "RAW_PARENT_ARG="
if "%USE_RAW_PARENT_IF_MISSING%"=="1" set "RAW_PARENT_ARG=--raw-parent-if-missing"

set "DEDUPE_ARG="
if "%DEDUPE_SAME_MAKER%"=="0" set "DEDUPE_ARG=--dedupe-same-maker=false"

set "BACKUP_ARG="
if "%BACKUP_ORIGINALS%"=="1" set "BACKUP_ARG=--backup-originals"

set "EXCLUDE_ARGS="
if defined EXCLUDE1 set "EXCLUDE_ARGS=!EXCLUDE_ARGS! --exclude ""!EXCLUDE1!"""
if defined EXCLUDE2 set "EXCLUDE_ARGS=!EXCLUDE_ARGS! --exclude ""!EXCLUDE2!"""
if defined EXCLUDE3 set "EXCLUDE_ARGS=!EXCLUDE_ARGS! --exclude ""!EXCLUDE3!"""
if defined EXCLUDE4 set "EXCLUDE_ARGS=!EXCLUDE_ARGS! --exclude ""!EXCLUDE4!"""

echo Running:
echo "%CLI_BIN%" rename !JPG_INPUT_ARGS! --template "%TEMPLATE%" --output table --apply %RAW_PARENT_ARG% %DEDUPE_ARG% %BACKUP_ARG% %EXCLUDE_ARGS%
echo.

call "%CLI_BIN%" rename ^
  !JPG_INPUT_ARGS! ^
  --template "%TEMPLATE%" ^
  --output table ^
  --apply ^
  %RAW_PARENT_ARG% ^
  %DEDUPE_ARG% ^
  %BACKUP_ARG% ^
  %EXCLUDE_ARGS%

if errorlevel 1 (
  echo.
  echo Rename failed.
  pause
  exit /b 10
)

echo.
echo Done.
start "" "%OPEN_PATH%"
endlocal
