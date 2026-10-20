#!/usr/bin/env bash
# 把 GPUI 壳打成 macOS `.app`。
#
# 为什么需要：macOS 的系统通知/通知权限、Dock 身份、菜单栏应用名都挂在
# **应用包**上——裸二进制跑起来 gpui 会明确报
# `system notifications disabled: not running from an app bundle`，
# 追更通知点了没反应。Tauri 壳一直有 `dmg/app` 产物，GPUI 壳缺这一层。
#
# 用法：
#   bash scripts/gpui-app.sh            # release 构建
#   bash scripts/gpui-app.sh debug      # 本地验证用（快）
# 产物：target/gpui-app/红果.app（ad-hoc 签名；正式分发需换 Developer ID）。
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
mode="${1:-release}"
case "$mode" in
  release) cargo_profile="release"; profile_dir="release" ;;
  debug)   cargo_profile="dev";     profile_dir="debug" ;;
  *) echo "用法: $0 [release|debug]" >&2; exit 2 ;;
esac

version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
app="$root/target/gpui-app/红果.app"

cargo build -p hongguo-gpui --profile "$cargo_profile"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/target/$profile_dir/hongguo-gpui" "$app/Contents/MacOS/hongguo-gpui"
if [[ -f "$root/crates/hongguo-tauri-app/icons/icon.icns" ]]; then
  cp "$root/crates/hongguo-tauri-app/icons/icon.icns" "$app/Contents/Resources/icon.icns"
fi

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>hongguo-gpui</string>
  <key>CFBundleIdentifier</key><string>com.hongguo.desktop.gpui</string>
  <key>CFBundleName</key><string>红果</string>
  <key>CFBundleDisplayName</key><string>红果</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.video</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# ad-hoc 签名：本地/开发分发够用；正式分发换成 Developer ID + notarize。
codesign --force --deep --sign - "$app" >/dev/null
echo "已生成：$app"
