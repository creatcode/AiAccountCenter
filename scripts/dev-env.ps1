#Requires -Version 7.0
<#
.SYNOPSIS
    把 AiAccountCenter 开发环境加载到当前进程。

.DESCRIPTION
    以“项目所需工具是否已安装”为判断前提，不约定、不要求、也不探测任何固定目录名或盘符。

    环境根解析顺序：
      1. 显式 -EnvironmentRoot 参数；
      2. AI_ACCOUNT_CENTER_ENV_ROOT 环境变量；
      3. 都没有时，检测 PATH 上是否已有可用的 Rust/Cargo；有则直接复用，缓存与临时目录落在项目内 .cache/ 与 .tmp/。

    仍未检测到 Rust 或 MSVC 时抛出提示：先向用户确认安装路径，再按 docs/DEVELOPMENT.md 安装到该路径。

    本脚本只影响当前进程及其子进程，不修改系统/用户 PATH、HOME、Shell 配置、执行策略或全局包管理器配置。
#>
param(
    [string]$EnvironmentRoot = $env:AI_ACCOUNT_CENTER_ENV_ROOT,
    [string]$RustToolchain   = $env:AI_ACCOUNT_CENTER_RUST_TOOLCHAIN
)

$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw '此入口用于 Windows 开发环境；Linux 环境按开发文档另行验证。'
}

$projectRoot = Split-Path -Parent $PSScriptRoot

if ($EnvironmentRoot) {
    if (-not (Test-Path -LiteralPath $EnvironmentRoot)) {
        throw "指定的开发环境根不存在：$EnvironmentRoot；请确认实际安装路径后重新传入。"
    }
    $EnvironmentRoot = (Resolve-Path -LiteralPath $EnvironmentRoot).Path
    $env:AI_ACCOUNT_CENTER_ENV_ROOT = $EnvironmentRoot
    $env:RUSTUP_HOME = Join-Path $EnvironmentRoot 'rustup'
    $env:CARGO_HOME = Join-Path $EnvironmentRoot 'cargo'
    $env:npm_config_cache = Join-Path $EnvironmentRoot 'cache/npm'
    $env:TEMP = Join-Path $EnvironmentRoot 'tmp'
    $cargoBin = Join-Path $EnvironmentRoot 'cargo/bin'
    if (-not (Test-Path -LiteralPath (Join-Path $cargoBin 'rustc.exe'))) {
        throw "环境根内未找到 Rust 工具链：$cargoBin；请按 docs/DEVELOPMENT.md 准备缺项。"
    }
    $env:PATH = $cargoBin + [IO.Path]::PathSeparator + (($env:PATH -split [IO.Path]::PathSeparator | Where-Object { $_ -ne $cargoBin }) -join [IO.Path]::PathSeparator)
    $vsRoot = Join-Path $EnvironmentRoot 'msvc'
} elseif ((Get-Command rustc.exe -ErrorAction SilentlyContinue) -and (Get-Command cargo.exe -ErrorAction SilentlyContinue)) {
    Write-Host '未指定环境根，复用 PATH 上已有的 Rust/Cargo。'
    $env:npm_config_cache = Join-Path $projectRoot '.cache/npm'
    $env:TEMP = Join-Path $projectRoot '.tmp'
    $vsRoot = $null
} else {
    throw '未检测到可用的 Rust/Cargo。请先向用户确认开发环境安装路径，按 docs/DEVELOPMENT.md 安装后，用 -EnvironmentRoot 或 AI_ACCOUNT_CENTER_ENV_ROOT 指定该路径。'
}

$env:TMP = $env:TEMP
$env:TMPDIR = $env:TEMP
$env:CARGO_TARGET_DIR = Join-Path $projectRoot 'build/cargo'
$env:VSCMD_SKIP_SENDTELEMETRY = '1'
if ($RustToolchain) { $env:RUSTUP_TOOLCHAIN = $RustToolchain }

foreach ($directory in @($env:TEMP, $env:npm_config_cache)) {
    if (-not (Test-Path -LiteralPath $directory)) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }
}

# MSVC：优先使用环境根内安装，其次回退到系统已注册的 C++ 生成工具，都没有则提示先确认安装路径。
$hasVcTools = $vsRoot -and (Test-Path -LiteralPath (Join-Path $vsRoot 'VC/Auxiliary/Build/vcvarsall.bat'))
if (-not $hasVcTools) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) {
        throw '未检测到 MSVC（C++ 生成工具）。请先向用户确认安装路径，再按 docs/DEVELOPMENT.md 安装。'
    }
    $vsRoot = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or -not $vsRoot) {
        throw '未找到可用的 C++ 生成工具；请确认安装路径与实际安装的组件。'
    }
}

$devShell = Join-Path $vsRoot 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll'
Import-Module $devShell
Enter-VsDevShell -VsInstallPath $vsRoot -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
Write-Host "环境已加载；项目：$projectRoot；MSVC：$vsRoot"
