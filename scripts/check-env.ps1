#Requires -Version 7.0
param([string]$EnvironmentRoot = $env:AI_ACCOUNT_CENTER_ENV_ROOT)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'dev-env.ps1') -EnvironmentRoot $EnvironmentRoot
foreach ($command in @('git.exe', 'node.exe', 'npm.cmd', 'rustc.exe', 'cargo.exe', 'cl.exe', 'link.exe', 'rc.exe')) {
    $tool = Get-Command $command -ErrorAction Stop
    Write-Host "$command : $($tool.Source)"
}
& git.exe --version
& node.exe --version
if ($LASTEXITCODE -ne 0) { throw 'Node 不可执行。' }
$nodeVersion = [version]((& node.exe --version).TrimStart('v'))
if ($nodeVersion -lt [version]'22.12.0') { throw '固定前端基线需兼容 Node >=22.12.0；按锁文件核对受支持版本。' }
& npm.cmd --version
if ($LASTEXITCODE -ne 0) { throw 'npm 不可执行。' }
& rustc.exe -Vv
if ($LASTEXITCODE -ne 0) { throw 'Rust 不可执行。' }
& cargo.exe -V
if ($LASTEXITCODE -ne 0) { throw 'Cargo 不可执行。' }
& rustfmt.exe --version
if ($LASTEXITCODE -ne 0) { throw 'rustfmt 不可执行。' }
& cargo.exe clippy --version
if ($LASTEXITCODE -ne 0) { throw 'clippy 不可执行。' }

$probeRoot = Join-Path $env:AI_ACCOUNT_CENTER_ENV_ROOT 'checks/AiAccountCenter'
if (-not (Test-Path -LiteralPath $probeRoot)) {
    New-Item -ItemType Directory -Path $probeRoot | Out-Null
}
$cSource = Join-Path $probeRoot 'environment-probe.c'
$cText = "#include <windows.h>`n#include <stdio.h>`nint main(void) { printf(`"msvc-sdk-ok %lu\n`", GetCurrentProcessId()); return 0; }`n"
if (Test-Path -LiteralPath $cSource) {
    if ([IO.File]::ReadAllText($cSource) -ne $cText) { throw "不覆盖未知验证文件：$cSource" }
} else {
    [IO.File]::WriteAllText($cSource, $cText, (New-Object Text.UTF8Encoding($false)))
}
Push-Location $probeRoot
try {
    & cl.exe /nologo $cSource /Fe:msvc-probe.exe /Fo:msvc-probe.obj
    if ($LASTEXITCODE -ne 0) { throw 'MSVC/Windows SDK 编译或链接失败。' }
    & './msvc-probe.exe'
    if ($LASTEXITCODE -ne 0) { throw 'MSVC 验证程序运行失败。' }
    'fn main() { println!("rust-link-ok"); }' | & rustc.exe --crate-name environment_probe - -o rust-probe.exe
    if ($LASTEXITCODE -ne 0) { throw 'Rust/MSVC 链接失败。' }
    & './rust-probe.exe'
    if ($LASTEXITCODE -ne 0) { throw 'Rust 验证程序运行失败。' }
} finally {
    Pop-Location
}
$actualCache = & npm.cmd config get cache
if ($LASTEXITCODE -ne 0 -or $actualCache.Trim() -ne $env:npm_config_cache) { throw 'npm 缓存路径未按指定环境根生效。' }
Write-Host "环境检查通过；验证产物：$probeRoot；npm 缓存：$actualCache"
