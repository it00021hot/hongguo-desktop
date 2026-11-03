#!/usr/bin/env bash
# 红果桌面版 macOS 安装脚本：从 GitHub Release 下载当前架构的 DMG，
# 校验 SHA256 后装入 /Applications。终端（curl）下载渠道不带系统隔离
# 标记，走本脚本安装不会遇到 Gatekeeper 的「已损坏，无法打开」弹窗
# ——那是浏览器下载给无证书应用打的隔离标记触发的，文件本身没有损坏。
#
# 用法：bash install-macos.sh [tag]    # 如 v0.6.1；缺省取最新 Release
# 依赖：curl / hdiutil / shasum（macOS 自带）
#
# 注意：macOS /bin/bash 仍是 3.2——`set -u` 下 `$VAR）`（全角右括号紧贴）
# 会被当成变量名导致「unbound variable」静默退出；变量展开一律写 `${VAR}`。
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

if pgrep -qx hongguo-desktop 2>/dev/null; then
  die "应用正在运行，请先退出再安装"
fi

latest_tag() {
  # 不用 curl -f 进管道：失败时 -s 吞掉正文，pipefail 会在 grep 处静默退出，
  # 走不到后面的 die。先落盘再解析，HTTP 非 200 / 无 tag 都给出可读错误。
  local body http tag
  body="$(mktemp)"
  http="$(curl -sS -o "${body}" -w '%{http_code}' \
    "https://api.github.com/repos/${REPO}/releases/latest" || true)"
  if [ "${http}" != "200" ]; then
    rm -f "${body}"
    die "查询最新版本失败（HTTP ${http:-?}）。可手动指定：bash install-macos.sh v0.6.1"
  fi
  tag="$(sed -nE 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' "${body}" | head -1)"
  rm -f "${body}"
  [ -n "${tag}" ] || die "解析最新版本号失败。可手动指定：bash install-macos.sh v0.6.1"
  printf '%s\n' "${tag}"
}

if [ $# -ge 1 ]; then
  TAG="$1"
else
  log "查询最新 Release…"
  TAG="$(latest_tag)"
fi
VER="${TAG#v}"
DMG="Hongguo_${VER}_${ARCH}.dmg"
BASE="https://github.com/${REPO}/releases/download/${TAG}"

TMP="$(mktemp -d)"
MOUNT=""
cleanup() {
  if [ -n "${MOUNT}" ]; then
    hdiutil detach "${MOUNT}" -quiet >/dev/null 2>&1 || true
  fi
  rm -rf "${TMP}"
}
trap cleanup EXIT

log "下载 ${DMG} 与 SHA256SUMS（${TAG}）…"
curl -fsSL --retry 3 -o "${TMP}/${DMG}" "${BASE}/${DMG}" \
  || die "下载 ${DMG} 失败（检查 ${BASE}/${DMG} 是否存在）"
curl -fsSL --retry 3 -o "${TMP}/SHA256SUMS" "${BASE}/SHA256SUMS" \
  || die "下载 SHA256SUMS 失败"
EXPECT="$(awk -v f="${DMG}" '$2 == f { print $1 }' "${TMP}/SHA256SUMS")"
[ -n "${EXPECT}" ] || die "SHA256SUMS 里找不到 ${DMG}"
ACTUAL="$(shasum -a 256 "${TMP}/${DMG}" | awk '{ print $1 }')"
[ "${ACTUAL}" = "${EXPECT}" ] || die "SHA256 校验失败（期望 ${EXPECT}，实际 ${ACTUAL}）"
log "SHA256 校验通过"

# 挂载前清 DMG 隔离标记（若用户误把浏览器下的 DMG 拷进脚本目录再装，
# 不先清会把 quarantine 传染进 .app）
xattr -dr com.apple.quarantine "${TMP}/${DMG}" 2>/dev/null || true

log "安装到 ${INSTALL_DIR}/${APP_NAME}…"
# 不能用 -quiet：它连结果行一起吞掉，挂载点就没法从输出里解析了
MOUNT="$(hdiutil attach -nobrowse -readonly "${TMP}/${DMG}" | awk -F'\t' 'END { print $NF }')"
[ -d "${MOUNT}/${APP_NAME}" ] || die "DMG 里没有 ${APP_NAME}"
rm -rf "${INSTALL_DIR}/${APP_NAME}"
if ! cp -R "${MOUNT}/${APP_NAME}" "${INSTALL_DIR}/"; then
  die "写入 ${INSTALL_DIR} 失败——手动执行：sudo cp -R \"${MOUNT}/${APP_NAME}\" ${INSTALL_DIR}/ && sudo xattr -dr com.apple.quarantine \"${INSTALL_DIR}/${APP_NAME}\""
fi

# 必须清掉并核验：残留 quarantine 时 Gatekeeper 会报「已损坏」；
# 以前 `|| true` 吞失败，装完仍打不开。
xattr -dr com.apple.quarantine "${INSTALL_DIR}/${APP_NAME}" 2>/dev/null || true
if xattr -lr "${INSTALL_DIR}/${APP_NAME}" 2>/dev/null | grep -q 'com.apple.quarantine'; then
  die "隔离标记未能清除。请手动执行：xattr -dr com.apple.quarantine ${INSTALL_DIR}/${APP_NAME}"
fi

log "安装完成：${INSTALL_DIR}/${APP_NAME}（${TAG}，${ARCH}），直接打开即可。"
log "不要用浏览器下载的 DMG 双击安装——那样会带隔离标记，系统会报「已损坏」。"
