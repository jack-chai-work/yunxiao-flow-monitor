<#
=============================================================================
 云效流水线实时监控 —— Windows 编译脚本

 用法（在项目根目录打开 PowerShell）：
   .\scripts\build-windows.ps1                       # 默认产出 NSIS 安装包
   .\scripts\build-windows.ps1 -Bundles nsis,msi     # 同时产出 NSIS + MSI
   .\scripts\build-windows.ps1 -Debug                # 调试构建
   .\scripts\build-windows.ps1 -Features secure-store  # 令牌存 Windows 凭据管理器
   .\scripts\build-windows.ps1 -SkipDeps             # 跳过依赖自动安装

 产物目录：src-tauri\target\release\bundle\
=============================================================================
#>
# 注意：这里不要加 [CmdletBinding()]。
# 它会自动注入名为 -Debug 的通用参数，和下面的 -Debug 开关重名，
# 运行时会报「已为命令多次定义名为"Debug"的参数」。
param(
  [string]$Bundles = 'nsis',
  [switch]$Debug,
  [string]$Features = '',
  [switch]$SkipDeps
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Write-Log  { param([string]$m) Write-Host "[build] $m" -ForegroundColor Cyan }
function Write-Warn { param([string]$m) Write-Host "[warn ] $m" -ForegroundColor Yellow }
function Throw-Err  { param([string]$m) Write-Host "[error] $m" -ForegroundColor Red; throw $m }

# --- 0. 执行策略提醒 ---------------------------------------------------------
# 若脚本被策略拦截，可在当前会话临时放开：
#   Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass

# --- 1. 依赖检查与安装 -------------------------------------------------------
function Test-Command { param([string]$Name) return [bool](Get-Command $Name -ErrorAction SilentlyContinue) }

function Install-Deps {
  if ($SkipDeps) { Write-Log '已跳过依赖安装（-SkipDeps）'; return }

  if (-not (Test-Command 'node')) {
    Write-Warn '未找到 Node.js，尝试用 winget 安装 LTS 版本…'
    if (Test-Command 'winget') {
      winget install --id OpenJS.NodeJS.LTS -e --accept-source-agreements --accept-package-agreements
      Throw-Err 'Node.js 已安装，请关闭并重新打开 PowerShell 后再次运行本脚本'
    } else {
      Throw-Err '请手动安装 Node.js 20+：https://nodejs.org/zh-cn/download'
    }
  }

  if (-not (Test-Command 'cargo')) {
    Write-Warn '未找到 Rust 工具链，尝试用 winget 安装 rustup…'
    if (Test-Command 'winget') {
      winget install --id Rustlang.Rustup -e --accept-source-agreements --accept-package-agreements
      Throw-Err 'Rustup 已安装，请执行 `rustup default stable` 后重新打开 PowerShell 再运行本脚本'
    } else {
      Throw-Err '请手动安装 Rust：https://rustup.rs'
    }
  }

  # Tauri 在 Windows 上需要 MSVC 链接器（link.exe），来自 VS 2022 生成工具
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  $hasMsvc = $false
  if (Test-Path $vswhere) {
    $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if ($vsPath) { $hasMsvc = $true }
  }
  if (-not $hasMsvc) {
    Write-Warn '未检测到 MSVC C++ 生成工具。Tauri 编译需要它。'
    Write-Warn '安装方式：winget install Microsoft.VisualStudio.2022.BuildTools '
    Write-Warn '          并勾选「使用 C++ 的桌面开发」工作负载'
    if (Test-Command 'winget') {
      $ans = Read-Host '是否现在用 winget 自动安装 VS 生成工具？(y/N)'
      if ($ans -match '^[yY]') {
        winget install --id Microsoft.VisualStudio.2022.BuildTools -e `
          --override '--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended' `
          --accept-source-agreements --accept-package-agreements
        Throw-Err 'VS 生成工具安装完成，请重新打开 PowerShell 后再次运行本脚本'
      }
    }
  }

  # WebView2 运行时：Win11 与近期 Win10 已内置，老系统需要装
  $wvKey = 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
  if (-not (Test-Path $wvKey)) {
    Write-Warn '未检测到 WebView2 运行时，尝试安装 Evergreen Bootstrapper…'
    $tmp = Join-Path $env:TEMP 'MicrosoftEdgeWebview2Setup.exe'
    try {
      Invoke-WebRequest -Uri 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $tmp -UseBasicParsing
      Start-Process -FilePath $tmp -ArgumentList '/silent /install' -Wait
      Write-Log 'WebView2 运行时安装完成'
    } catch {
      Write-Warn "WebView2 自动安装失败：$($_.Exception.Message)"
      Write-Warn '请手动安装：https://developer.microsoft.com/microsoft-edge/webview2/'
    }
  } else {
    Write-Log 'WebView2 运行时就绪'
  }
}

# --- 2. 工具链信息 -----------------------------------------------------------
function Show-Toolchain {
  $nodeVer = (node -v)
  $major = [int]($nodeVer -replace '^v(\d+).*', '$1')
  if ($major -lt 20) { Throw-Err "Node.js 版本过低（当前 $nodeVer），Tauri v2 需要 20+" }
  Write-Log "工具链：node $nodeVer / $(cargo --version)"
}

# --- 3. 前端依赖 -------------------------------------------------------------
function Install-NodeDeps {
  if (Test-Path 'node_modules') {
    Write-Log 'node_modules 已存在，跳过安装'
  } else {
    Write-Log '安装前端依赖：npm install'
    npm install
    if ($LASTEXITCODE -ne 0) { Throw-Err 'npm install 失败' }
  }
}

# --- 4. 图标 -----------------------------------------------------------------
function Ensure-Icons {
  if ((Test-Path 'src-tauri\icons\icon.ico') -and (Test-Path 'src-tauri\icons\128x128.png')) {
    Write-Log '图标已存在，跳过生成'
    return
  }
  Write-Log '生成应用图标'
  npm run icon:build
  if ($LASTEXITCODE -ne 0) { Throw-Err '图标生成失败' }
}

# --- 5. 编译 -----------------------------------------------------------------
function Invoke-Build {
  $profileName = if ($Debug) { 'debug' } else { 'release' }
  $tauriArgs = @('build')
  if ($Debug) { $tauriArgs += '--debug' }
  if ($Bundles) { $tauriArgs += @('--bundles', $Bundles) }
  if ($Features) { $tauriArgs += @('--features', $Features) }

  Write-Log "开始编译（profile=$profileName bundles=$Bundles features=$(if ($Features) { $Features } else { '默认' })）"
  Write-Log '首次编译需下载并构建全部 Rust 依赖，通常 5–15 分钟，请耐心等待'

  # 注意：不能直接写 `npm run tauri -- @tauriArgs`。
  # PowerShell 会把命令行里裸写的 `--` 当成「参数结束标记」自己吃掉，
  # npm 收不到 `--`，就会把 `--bundles` 解析成 npm 自己的参数、只剩 `nsis` 传给 tauri，
  # 编译时报 `error: unexpected argument 'nsis' found`。
  # 把 `--` 作为数组元素再整体展开，PowerShell 才会原样传递给 npm。
  $npmArgs = @('run', 'tauri', '--') + $tauriArgs
  npm @npmArgs
  if ($LASTEXITCODE -ne 0) { Throw-Err "tauri build 失败，退出码 $LASTEXITCODE" }

  $bundleDir = "src-tauri\target\$profileName\bundle"
  Write-Log '编译完成，产物：'
  Get-ChildItem -Path $bundleDir -Recurse -Include *.exe, *.msi -ErrorAction SilentlyContinue |
    ForEach-Object { Write-Host "  $($_.FullName)" -ForegroundColor Green }
}

# --- main --------------------------------------------------------------------
Write-Log "项目根目录：$Root"
Install-Deps
Show-Toolchain
Install-NodeDeps
Ensure-Icons
Invoke-Build
Write-Log '全部完成 ✅'
