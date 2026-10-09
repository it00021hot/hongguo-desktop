# 应用名称与本地化

应用名在**三个层**上各有其位，别混淆；改名的入口都在 `scripts/` 里，产物
提交进仓库（构建不依赖本机 Python / 网络）。

| 层                                           | 值                                                 | 机制                                                        | 生成命令             |
| -------------------------------------------- | -------------------------------------------------- | ----------------------------------------------------------- | -------------------- |
| 包名 / 可执行文件 / `productName`            | `Hongguo`                                          | 固定不本地化                                                | 改 `tauri.conf.json` |
| macOS 系统显示名（Dock / Finder / 菜单栏）   | zh：红果播放器 · zh-Hant：紅果播放器 · en：Hongguo | `Contents/Resources/<lang>.lproj/InfoPlist.strings`         | `make assets`        |
| macOS 窗口标题（Mission Control / 窗口菜单） | 同上，跟随界面语言                                 | 运行时 `setTitle(t('app.name'))`（`src/routes/__root.tsx`） | —                    |
| Windows 快捷方式名 /「应用和功能」           | 同上                                               | 安装时按系统语言定（NSIS）                                  | `make assets`        |

**包名不本地化**：改名等于换了应用——LaunchServices 注册、单实例认亲、更新
包识别都会断。显示的国际化全部靠上面的覆盖机制。

## macOS

macOS 有**运行时**本地化：系统按当前语言去 bundle 里找对应的
`InfoPlist.strings`，覆盖 `CFBundleDisplayName` / `CFBundleName`。所以目录叫
`Hongguo.app` 而 Dock 显示「红果播放器」并不矛盾——微信就是这么做的
（目录 `WeChat.app`，Dock 显示「微信」）。

- 源文件：`src-tauri/i18n/<lang>.lproj/InfoPlist.strings`
- 生成：`python3 scripts/make-macos-lproj.py`（`make assets` 会带上）
- 进包：`tauri.conf.json > bundle.macOS.files`（键 = `Contents` 下的目标路径，
  值 = 仓库内源路径）

两个坑：

1. **编码必须是 UTF-16LE + BOM**（`file` 看微信那份就是 "UTF-16, little-endian"）。
   UTF-8 会被系统静默忽略——名字不变，还不报错。脚本已处理。
2. 装到 `/Applications` 后要 `lsregister -f <app>` 重新注册，否则 LaunchServices
   还拿旧缓存。验证看 `lsregister -dump` 里的 `localizedNames` 字段（CLI 里
   `FileManager.displayName` 不查本地化，对微信也返回英文，别拿它判断）。

## Windows

Windows 没有运行时机制：快捷方式显示名就是 `.lnk` 文件名，「应用和功能」里的
名字是安装时写进注册表的字符串——只能在安装那一刻定死。所以走 NSIS 安装器。

- 名字表与钩子：`src-tauri/nsis/installer-hooks.nsh`
  生成：`python3 scripts/make-nsis-hooks.py`
- 安装器模板：`src-tauri/nsis/installer.nsi`
  生成：`python3 scripts/sync-nsis-template.py`（从 GitHub 拉与
  `@tauri-apps/cli` 同版本的官方模板，套补丁；`--check` 判断是否过期）
- 接入：`tauri.conf.json > bundle.windows.nsis.template` / `installerHooks`

补丁只做一件事：把模板里写死的 `${PRODUCTNAME}.lnk` 换成 `$AppShortcutName`
（安装时按语言定）。名字的来源、`DisplayName`、旧快捷方式改名都在钩子里。

**编码必须是 UTF-8 + BOM**（与 macOS 侧相反）：NSIS 在 Windows 上默认按 ANSI
读脚本，含中文的 `.nsh` 丢了 BOM 就会变乱码。Tauri 自己落盘语言文件也是
`write_utf8_with_bom`。

繁体（`TradChinese`）已加进 `nsis.languages`：只定义 `LangString` 而不加进
语言列表会牵出一串「LangString ... is not set in language table」告警。

### 升级 Tauri 之后

`pnpm tauri build` 用的模板是仓库里这份，升级 CLI 后要重跑：

```bash
make nsis     # 重新拉取对应版本的官方模板并套补丁
```

忘了跑的后果：模板停留在旧版本，与 bundler 新写入的上下文不匹配（缺变量、
多占位符都会导致编译失败——不会静默装错）。

## 验证手法（不需要 Windows 机器）

macOS 上装了 `makensis` 就能把整条链编出来做静态校验：

```bash
make assets && make nsis        # 生成全部产物
python3 scripts/sync-nsis-template.py --check   # 模板是否与上游同步
```

再做一次真实编译：把模板按 bundler 的方式渲染（替换 Handlebars 占位符）后交给
makensis。这一步能验到「官方模板 + 补丁 + 钩子」零告警编译、语言表数量正确
（3 个）、钩子插入位置在快捷方式创建之前，以及条件编译真的生效（对比
`DISPLAYLANGUAGESELECTOR` 开/关两种构建的指令数——本项目实测 59 vs 44）。

**运行时行为**（快捷方式到底显示什么名字）仍需在 Windows 真机上装一次确认；
本机没有 Windows，也跑不了 .exe。
