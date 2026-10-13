#Requires -Version 7.0
param([string]$EnvironmentRoot = $env:AI_ACCOUNT_CENTER_ENV_ROOT)

$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw '此入口用于 Windows 开发环境；Linux 环境按开发文档另行验证。'
}
if (-not $EnvironmentRoot) { $EnvironmentRoot = 'E:\rustenv' }
$EnvironmentRoot = (Resolve-Path -LiteralPath $EnvironmentRoot).Path
$projectRoot = Split-Path -Parent $PSScriptRoot
$rustupRoot = Join-Path $EnvironmentRoot 'rustup'
$cargoRoot = Join-Path $EnvironmentRoot 'cargo'
if (-not (Test-Path -LiteralPath (Join-Path $cargoRoot 'bin/rustc.exe'))) {
    throw "未找到开发工具链：$cargoRoot；请先按 docs/DEVELOPMENT.md 准备缺项。"
}

# 开发组件仅影响当前进程，不修改用户 PATH、HOME 或真实客户端路径。
$env:AI_ACCOUNT_CENTER_ENV_ROOT = $EnvironmentRoot
$env:RUSTUP_HOME = $rustupRoot
$env:CARGO_HOME = $cargoRoot
$env:RUSTUP_TOOLCHAIN = '1.98.1-x86_64-pc-windows-msvc'
$env:CARGO_TARGET_DIR = Join-Path $projectRoot 'build/cargo'
$env:npm_config_cache = Join-Path $EnvironmentRoot 'cache/npm'
$env:TEMP = Join-Path $EnvironmentRoot 'tmp'
$env:TMP = $env:TEMP
$env:TMPDIR = $env:TEMP
$env:VSCMD_SKIP_SENDTELEMETRY = '1'
foreach ($directory in @($env:TEMP, $env:npm_config_cache)) {
    if (-not (Test-Path -LiteralPath $directory)) {
        New-Item -ItemType Directory -Path $directory | Out-Null
    }
}

$vsRoot = Join-Path $EnvironmentRoot 'msvc'
if (-not (Test-Path -LiteralPath (Join-Path $vsRoot 'VC/Auxiliary/Build/vcvarsall.bat'))) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) { throw '未找到 MSVC 或微软安装器查询工具。' }
    $vsRoot = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or -not $vsRoot) { throw '未找到可用的 C++ Build Tools。' }
}
$devShell = Join-Path $vsRoot 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll'
Import-Module $devShell
Enter-VsDevShell -VsInstallPath $vsRoot -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
$cargoBin = Join-Path $cargoRoot 'bin'
$env:PATH = $cargoBin + [IO.Path]::PathSeparator + (($env:PATH -split [IO.Path]::PathSeparator | Where-Object { $_ -ne $cargoBin }) -join [IO.Path]::PathSeparator)
Write-Host "环境已加载：$EnvironmentRoot；项目：$projectRoot"
