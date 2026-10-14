@echo off
setlocal EnableDelayedExpansion
cd /d "%~dp0"
set "PROJECT_ROOT=%~dp0"
set "ROOT_FWD=%PROJECT_ROOT:\=/%"

echo ============================================================
echo  AiAccountCenter - generate/rotate Tauri updater signing key
echo ============================================================
echo.

where npx.cmd >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Node.js / npx not found on PATH.
  exit /b 1
)

set "KEY_DIR=%PROJECT_ROOT%"
set "KEY_FILE=%KEY_DIR%tauri-signing.key"

set "PASSWORD=%~1"
if "%PASSWORD%"=="" (
  echo [INFO] No password given: generating an empty-password key (dev / private repo only).
  echo        For a release key, run:  gen-key.bat "your-password"
  call npx tauri signer generate -w "%KEY_FILE%" --ci -f
) else (
  echo [INFO] Generating a password-protected key...
  call npx tauri signer generate -w "%KEY_FILE%" -p "%PASSWORD%" -f
)
if errorlevel 1 (
  echo [ERROR] Key generation failed.
  exit /b 1
)

echo.
echo [OK] Private key : %KEY_FILE%
echo [OK] Public key  : %KEY_FILE%.pub

REM Auto-write the new public key into tauri.conf.json (uses node, already required).
call node -e "const fs=require('fs');const p='%ROOT_FWD%src-tauri/tauri.conf.json';const k='%ROOT_FWD%tauri-signing.key.pub';const c=JSON.parse(fs.readFileSync(p,'utf8'));c.plugins.updater.pubkey=fs.readFileSync(k,'utf8').trim();fs.writeFileSync(p,JSON.stringify(c,null,2)+'\n');"
if errorlevel 1 (
  echo [WARN] Could not auto-update tauri.conf.json. Copy the .pub content into plugins.updater.pubkey manually.
) else (
  echo [OK]  tauri.conf.json plugins.updater.pubkey updated.
)

echo.
echo [IMPORTANT] Keep the private key safe. For CI, set:
echo   TAURI_SIGNING_PRIVATE_KEY          = content of %KEY_FILE%
echo   TAURI_SIGNING_PRIVATE_KEY_PASSWORD = the password (only if you set one)
echo.
echo [NOTE] Rotation: after changing the key, rebuild and ship a build carrying the NEW
echo        public key, let users update to it, then retire the old private key.
exit /b 0
