#!/usr/bin/env bash
# =============================================================================
# 云效流水线实时监控 —— Linux 编译脚本
#
# 用法：
#   ./scripts/build-linux.sh                       # 默认产出 deb + rpm + AppImage
#   ./scripts/build-linux.sh --bundles deb         # 只产出 deb
#   ./scripts/build-linux.sh --bundles appimage    # 只产出 AppImage（无需 rpmbuild）
#   ./scripts/build-linux.sh --debug               # 调试构建（不优化，编译快）
#   ./scripts/build-linux.sh --features secure-store   # 令牌存系统钥匙串
#   ./scripts/build-linux.sh --skip-deps           # 跳过系统依赖安装
#
# 产物目录：src-tauri/target/release/bundle/
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BUNDLES=""
BUILD_PROFILE="release"
PROFILE_FLAG="--release"
EXTRA_FEATURES=""
SKIP_DEPS=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundles)   BUNDLES="$2"; shift 2 ;;
    --debug)     BUILD_PROFILE="debug"; PROFILE_FLAG="--debug"; shift ;;
    --features)  EXTRA_FEATURES="$2"; shift 2 ;;
    --skip-deps) SKIP_DEPS=1; shift ;;
    -h|--help)   sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "未知参数：$1（-h 查看用法）" >&2; exit 2 ;;
  esac
done

log()  { printf '\033[1;34m[build]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[warn ]\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; exit 1; }

# --- 识别发行版与包管理器 ---------------------------------------------------
PKG_MGR=""
if command -v apt-get >/dev/null 2>&1; then PKG_MGR="apt"
elif command -v dnf >/dev/null 2>&1; then PKG_MGR="dnf"
elif command -v zypper >/dev/null 2>&1; then PKG_MGR="zypper"
elif command -v pacman >/dev/null 2>&1; then PKG_MGR="pacman"
fi

SUDO=""
if [[ $EUID -ne 0 ]] && command -v sudo >/dev/null 2>&1; then SUDO="sudo"
elif [[ $EUID -ne 0 ]]; then warn "当前非 root 且无 sudo，系统依赖安装可能失败"; fi

# --- 1. 系统依赖 -------------------------------------------------------------
install_deps() {
  [[ $SKIP_DEPS -eq 1 ]] && { log "已跳过系统依赖安装（--skip-deps）"; return; }
  [[ -z "$PKG_MGR" ]] && { warn "未识别的包管理器，请手动安装 Tauri v2 Linux 依赖后重试"; return; }

  log "安装系统依赖（包管理器：$PKG_MGR）"
  case "$PKG_MGR" in
    apt)
      $SUDO apt-get update -y
      $SUDO apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev build-essential curl wget file \
        libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
        pkg-config ca-certificates
      # AppImage 打包需要
      command -v linuxdeploy >/dev/null 2>&1 || warn "未找到 linuxdeploy，AppImage 打包时 Tauri 会自动下载"
      ;;
    dnf)
      $SUDO dnf install -y webkit2gtk4.1-devel gcc gcc-c++ curl wget file \
        openssl-devel libappindicator-gtk3-devel librsvg2-devel \
        pkgconf-pkg-config ca-certificates
      ;;
    zypper)
      $SUDO zypper install -y webkit2gtk41-devel gcc gcc-c++ curl wget file \
        libopenssl-devel libappindicator3-1 librsvg-devel pkg-config ca-certificates
      ;;
    pacman)
      $SUDO pacman -Sy --needed --noconfirm \
        webkit2gtk-4.1 base-devel curl wget file \
        openssl libayatana-appindicator librsvg pkgconf ca-certificates
      ;;
  esac
}

# --- 2. 工具链检查 -----------------------------------------------------------
check_toolchain() {
  command -v node >/dev/null 2>&1 || die "未找到 node，请先安装 Node.js 20+（https://nodejs.org）"
  command -v npm  >/dev/null 2>&1 || die "未找到 npm"

  NODE_MAJOR="$(node -p 'process.versions.node.split(".")[0]')"
  [[ "$NODE_MAJOR" -ge 20 ]] || die "Node.js 版本过低（当前 $(node -v)），Tauri v2 需要 20+"

  if ! command -v cargo >/dev/null 2>&1; then
    die "未找到 cargo。请先安装 Rust：curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  fi
  log "工具链：node $(node -v) / $(cargo --version)"
  command -v pkg-config >/dev/null 2>&1 || die "未找到 pkg-config，Tauri 编译必需"
}

# --- 3. 前端依赖 -------------------------------------------------------------
install_node_deps() {
  if [[ -d node_modules ]]; then
    log "node_modules 已存在，跳过安装（如需重装请先删除该目录）"
  else
    log "安装前端依赖：npm install"
    npm install
  fi
}

# --- 4. 图标 -----------------------------------------------------------------
ensure_icons() {
  if [[ -f src-tauri/icons/icon.ico && -f src-tauri/icons/128x128.png ]]; then
    log "图标已存在，跳过生成"
    return
  fi
  log "生成应用图标"
  npm run icon:build
}

# --- 5. 编译 -----------------------------------------------------------------
build() {
  local args=(build)
  [[ -n "$PROFILE_FLAG" && "$BUILD_PROFILE" == "debug" ]] && args+=(--debug)
  [[ -n "$BUNDLES" ]] && args+=(--bundles "$BUNDLES")
  [[ -n "$EXTRA_FEATURES" ]] && args+=(--features "$EXTRA_FEATURES")

  log "开始编译（profile=$BUILD_PROFILE bundles=${BUNDLES:-all} features=${EXTRA_FEATURES:-默认}）"
  log "首次编译需下载并构建全部 Rust 依赖，通常 5–15 分钟，请耐心等待"
  npm run tauri -- "${args[@]}"

  log "编译完成，产物位于："
  find "src-tauri/target/$BUILD_PROFILE/bundle" -maxdepth 2 -type f \
    \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \) -print 2>/dev/null | sed 's/^/  /' || true
}

main() {
  log "项目根目录：$ROOT"
  install_deps
  check_toolchain
  install_node_deps
  ensure_icons
  build
  log "全部完成 ✅"
}

main
