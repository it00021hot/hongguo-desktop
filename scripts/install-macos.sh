#!/usr/bin/env bash
# 红果桌面版 macOS 安装脚本：从 GitHub Release 下载当前架构的 DMG，
# 校验 SHA256 后装入 /Applications。终端（curl）下载渠道不带系统隔离
# 标记，走本脚本安装不会遇到 Gatekeeper 的「已损坏，无法打开」弹窗
# ——那是浏览器下载给无证书应用打的隔离标记触发的，文件本身没有损坏。
#
# 用法：bash install-macos.sh [tag]    # 如 v0.5.12；缺省取最新 Release
# 依赖：curl / hdiutil / shasum（macOS 自带）
set -euo pipefail

REPO="it00021hot/hongguo-desktop"
APP_NAME="Hongguo.app"
INSTALL_DIR="/Applications"

log() { printf '%s\n' "$*"; }
die() { printf '错误：%s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || die "本脚本只能在 macOS 上运行"
command -v curl >/dev/null || die "需要 curl（macOS 自带）"

case "$(uname -m)" in
  arm64) ARCH="aarch64" ;;
  x86_64) ARCH="x64" ;;
  *) die "不支持的架构：$(uname -m)" ;;
esac

if pgrep -qx hongguo-desktop; then
  die "应用正在运行，请先退出再安装"
fi

if [ $# -ge 1 ]; then
  TAG="$1"
else
  log "查询最新 Release…"
  TAG=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" |
    grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')
  [ -n "$TAG" ] || die "拿不到最新版本号（GitHub API 限流？稍后再试，或手动指定：bash install-macos.sh v0.5.12）"
fi
VER="${TAG#v}"
DMG="Hongguo_${VER}_${ARCH}.dmg"
BASE="https://github.com/$REPO/releases/download/$TAG"

TMP="$(mktemp -d)"
MOUNT=""
cleanup() {
  if [ -n "$MOUNT" ]; then
    hdiutil detach "$MOUNT" -quiet >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

log "下载 $DMG 与 SHA256SUMS（$TAG）…"
curl -fsSL --retry 3 -o "$TMP/$DMG" "$BASE/$DMG"
curl -fsSL --retry 3 -o "$TMP/SHA256SUMS" "$BASE/SHA256SUMS"
EXPECT=$(awk -v f="$DMG" '$2 == f {print $1}' "$TMP/SHA256SUMS")
[ -n "$EXPECT" ] || die "SHA256SUMS 里找不到 $DMG"
ACTUAL=$(shasum -a 256 "$TMP/$DMG" | awk '{print $1}')
[ "$ACTUAL" = "$EXPECT" ] || die "SHA256 校验失败（期望 $EXPECT，实际 $ACTUAL）"
log "SHA256 校验通过"

log "安装到 $INSTALL_DIR/$APP_NAME…"
# 不能用 -quiet：它连结果行一起吞掉，挂载点就没法从输出里解析了
MOUNT="$(hdiutil attach -nobrowse -readonly "$TMP/$DMG" | awk -F'\t' 'END{print $NF}')"
[ -d "$MOUNT/$APP_NAME" ] || die "DMG 里没有 $APP_NAME"
rm -rf "$INSTALL_DIR/$APP_NAME"
if ! cp -R "$MOUNT/$APP_NAME" "$INSTALL_DIR/"; then
  die "写入 $INSTALL_DIR 失败——手动执行：sudo cp -R \"$MOUNT/$APP_NAME\" $INSTALL_DIR/ && sudo xattr -dr com.apple.quarantine \"$INSTALL_DIR/$APP_NAME\""
fi

# curl 渠道本身不带隔离标记；这里兜底清一次，兼容异常打标的场景
xattr -dr com.apple.quarantine "$INSTALL_DIR/$APP_NAME"

log "安装完成：$INSTALL_DIR/$APP_NAME（$TAG，$ARCH），直接打开即可。"
