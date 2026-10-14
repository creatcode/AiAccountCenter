@echo off
setlocal EnableDelayedExpansion
cd /d "%~dp0"
set "PROJECT_ROOT=%~dp0"

echo ============================================================
echo  AiAccountCenter one-click packaging
echo  Platforms: Windows desktop (NSIS) / Windows Web / Linux Web
echo ============================================================
echo.

REM ---- Environment root (same rule as scripts/dev-env.ps1) ----
set "ENV_ROOT=%~1"
if not defined ENV_ROOT set "ENV_ROOT=%AI_ACCOUNT_CENTER_ENV_ROOT%"

REM ---- 1. Node.js / npm ----
where npm.cmd >nul 2>nul
if errorlevel 1 (
  call :warn_missing "Node.js (npm)" "winget install OpenJS.NodeJS.LTS" "https://nodejs.org/"
  exit /b 1
)

REM ---- 2. Rust / Cargo ----
set "CARGO_BIN="
if defined ENV_ROOT (
  if not exist "%ENV_ROOT%\" (
    echo [ERROR] Environment root not found: %ENV_ROOT%
    echo         Pass a valid path:  build.bat <env-root>
    echo         or set env var     AI_ACCOUNT_CENTER_ENV_ROOT
    exit /b 1
  )
  set "RUSTUP_HOME=%ENV_ROOT%\rustup"
  set "CARGO_HOME=%ENV_ROOT%\cargo"
  set "CARGO_BIN=%ENV_ROOT%\cargo\bin"
) else (
  for /f "delims=" %%i in ('where cargo.exe 2^>nul') do if not defined CARGO_ON_PATH set "CARGO_ON_PATH=%%i"
  if defined CARGO_ON_PATH for %%i in ("%CARGO_ON_PATH%") do set "CARGO_BIN=%%~dpi"
)
if defined CARGO_BIN if not exist "%CARGO_BIN%cargo.exe" set "CARGO_BIN="
if not defined CARGO_BIN (
  call :warn_missing "Rust (cargo)" "winget install Rustlang.Rustup" "https://rustup.rs/"
  exit /b 1
)
set "PATH=%CARGO_BIN%;%PATH%"

REM ---- 3. MSVC (needed for native desktop; server also links on Windows) ----
call :locate_msvc
if not defined LINK_EXE (
  call :warn_missing "MSVC C++ build tools (link.exe)" "winget install Microsoft.VisualStudio.2022.BuildTools --override \"--add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.26100 --passive --wait\"" "https://visualstudio.microsoft.com/visual-cpp-build-tools/"
  exit /b 1
)

set "CARGO_TARGET_DIR=%PROJECT_ROOT%build\cargo"
set "PATH=%MSVC_BIN%;%SDK_BIN%\x64;%CARGO_BIN%;%PATH%"
set "INCLUDE=%MSVC_INC%;%SDK_INC%\ucrt;%SDK_INC%\shared;%SDK_INC%\um;%SDK_INC%\winrt;%SDK_INC%\cppwinrt"
set "LIB=%MSVC_LIB%;%SDK_LIB%\ucrt\x64;%SDK_LIB%\um\x64"
set "npm_config_cache=%PROJECT_ROOT%.cache\npm"
set "VSCMD_SKIP_SENDTELEMETRY=1"

echo [OK] cargo = %CARGO_BIN%cargo.exe
echo [OK] link  = %LINK_EXE%
echo.

REM ---- 4. Platform selection (arg or interactive menu) ----
set "ARG=%~2"
if /i "%ARG%"=="" goto :menu
goto :apply_arg

:menu
echo  Choose what to package (default: all):
echo    [1] Windows desktop installer   (NSIS)
echo    [2] Windows desktop installer   (MSI)
echo    [3] Windows Web service         (ai-account-center-server.exe)
echo    [4] Linux Web service           (musl, needs cross toolchain)
echo    [a] All platforms               (1 + 3 + 4)
echo    [q] Quit
choice /c 1234aq /n /m "Select [1/2/3/4/a/q]: "
if errorlevel 6 exit /b 0
if errorlevel 5 goto :sel_all
if errorlevel 4 goto :sel_linux
if errorlevel 3 goto :sel_win_web
if errorlevel 2 goto :sel_msi
if errorlevel 1 goto :sel_nsis
goto :eof

:apply_arg
if /i "%ARG%"=="all"        goto :sel_all
if /i "%ARG%"=="nsis"       goto :sel_nsis
if /i "%ARG%"=="desktop"    goto :sel_nsis
if /i "%ARG%"=="msi"        goto :sel_msi
if /i "%ARG%"=="server-win" goto :sel_win_web
if /i "%ARG%"=="web-win"    goto :sel_win_web
if /i "%ARG%"=="server-linux" goto :sel_linux
if /i "%ARG%"=="web-linux"  goto :sel_linux
echo [ERROR] Unknown target: %ARG%
echo         Usage: build.bat [all^|nsis^|msi^|server-win^|server-linux]
exit /b 1

:sel_nsis
  call :do_desktop nsis
  goto :done
:sel_msi
  call :do_desktop msi
  goto :done
:sel_win_web
  call :do_server_win
  goto :done
:sel_linux
  call :do_server_linux
  goto :done
:sel_all
  call :do_desktop nsis
  call :do_server_win
  call :do_server_linux
  goto :done

:done
if not exist "%PROJECT_ROOT%release\" mkdir "%PROJECT_ROOT%release"
for %%f in ("%CARGO_TARGET_DIR%release\bundle\nsis\*.exe" "%CARGO_TARGET_DIR%release\bundle\nsis\*.sig" "%CARGO_TARGET_DIR%release\latest.json" "%CARGO_TARGET_DIR%release\ai-account-center-server.exe" "%CARGO_TARGET_DIR%x86_64-unknown-linux-musl\release\ai-account-center-server") do (
  if exist "%%~f" copy /y "%%~f" "%PROJECT_ROOT%release\" >nul
)
echo.
echo [DONE] Artifacts copied to: %PROJECT_ROOT%release\
exit /b 0

REM ============================================================
REM  Build steps
REM ============================================================

:do_desktop
  set "TGT=%~1"
  set "KEY_FILE=%PROJECT_ROOT%tauri-signing.key"
  if not exist "%KEY_FILE%" (
    echo [STEP] No signing key; generating empty-password dev key...
    call npx tauri signer generate -w "%KEY_FILE%" --ci
    if errorlevel 1 ( echo [ERROR] key generation failed & exit /b 1 )
  )
  for /f "usebackq delims=" %%i in ("%KEY_FILE%") do set "TAURI_SIGNING_PRIVATE_KEY=%%i"
  REM 必须显式给空口令：不设时 tauri signer 会在 stdin 等口令，导致打包挂住。
  set "TAURI_SIGNING_PRIVATE_KEY_PASSWORD="
  echo [STEP] Packaging Windows desktop (%TGT%) ...
  call npx tauri build --config "{\"bundle\":{\"targets\":[\"%TGT%\"]}}"
  if errorlevel 1 ( echo [ERROR] desktop build failed & exit /b 1 )
  exit /b 0

:do_server_win
  call :ensure_frontend
  echo [STEP] Building Windows Web service ...
  "%CARGO_BIN%cargo.exe" build -p wb-switch-server --release
  if errorlevel 1 ( echo [ERROR] server(win) build failed & exit /b 1 )
  exit /b 0

:do_server_linux
  call :ensure_frontend
  set "LINUX_TARGET=x86_64-unknown-linux-musl"
  "%CARGO_BIN%rustup.exe" target list --installed 2>nul | findstr /c:"%LINUX_TARGET%" >nul
  if errorlevel 1 (
    echo [STEP] Adding Rust target %LINUX_TARGET% ...
    "%CARGO_BIN%rustup.exe" target add %LINUX_TARGET%
    if errorlevel 1 ( echo [ERROR] failed to add %LINUX_TARGET% & exit /b 1 )
  )
  echo [STEP] Building Linux Web service (musl) ...
  "%CARGO_BIN%cargo.exe" build -p wb-switch-server --release --target %LINUX_TARGET%
  if errorlevel 1 (
    echo [ERROR] Linux cross build failed. If the error mentions a missing linker,
    echo         install musl cross toolchain (e.g. musl-gcc) and re-run.
    exit /b 1
  )
  exit /b 0

:ensure_frontend
  if not exist "%PROJECT_ROOT%dist\index.html" (
    echo [STEP] Building web UI (dist) ...
    if not exist "%PROJECT_ROOT%node_modules\" (
      call npm ci
      if errorlevel 1 ( echo [ERROR] npm ci failed & exit /b 1 )
    )
    call npm run build
    if errorlevel 1 ( echo [ERROR] frontend build failed & exit /b 1 )
  )
  exit /b 0

REM ============================================================
REM  Subroutines
REM ============================================================

:locate_msvc
  set "LINK_EXE="
  set "VSROOT="
  set "VSW=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
  if exist "%VSW%" (
    for /f "usebackq delims=" %%i in (`"%VSW%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2^>nul`) do set "VSROOT=%%i"
  )
  if not defined VSROOT (
    if defined ENV_ROOT if exist "%ENV_ROOT%\msvc\" set "VSROOT=%ENV_ROOT%\msvc"
  )
  if not defined VSROOT exit /b 0

  set "MSVC_DIR="
  for /d %%d in ("%VSROOT%\VC\Tools\MSVC\*") do set "MSVC_DIR=%%d"
  if not defined MSVC_DIR exit /b 0
  set "MSVC_BIN=%MSVC_DIR%\bin\Hostx64\x64"
  set "MSVC_INC=%MSVC_DIR%\include"
  set "MSVC_LIB=%MSVC_DIR%\lib\x64"

  set "SDK_BASE=%ProgramFiles(x86)%\Windows Kits\10"
  set "SDK_VER="
  for /d %%d in ("%SDK_BASE%\Include\*") do set "SDK_VER=%%~nxd"
  if not defined SDK_VER exit /b 0
  set "SDK_INC=%SDK_BASE%\Include\%SDK_VER%"
  set "SDK_LIB=%SDK_BASE%\Lib\%SDK_VER%"
  set "SDK_BIN=%SDK_BASE%\bin\%SDK_VER%"

  set "LINK_EXE=%MSVC_BIN%\link.exe"
  if not exist "%LINK_EXE%" set "LINK_EXE="
  exit /b 0

:warn_missing
  echo.
  echo [MISSING] %~1
  echo   Install with : %~2
  echo   Or download  : %~3
  echo.
  choice /c yn /n /m "Install it now? [y/n] "
  if errorlevel 2 exit /b 1
  where winget.exe >nul 2>nul
  if not errorlevel 1 (
    for /f "tokens=1,* delims= " %%a in ("%~2") do call winget install %%b
  ) else (
    start "" "%~3"
  )
  echo.
  echo [INFO] After installing, re-run:  build.bat
  exit /b 1
