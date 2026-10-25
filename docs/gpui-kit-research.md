# GPUI 自绘壳界面体验调研报告

> 目的：为 `hongguo-desktop` 的 GPUI 自绘壳（Rust + gpui-kit）改善界面体验提供外部参考，参照对象为 Tauri 2 壳（React 19 + WebView）。
>
> 调研范围：纯外部资料调研。**全程未读取、未修改本仓库任何代码**；所有结论均来自公开网络资源并附 URL。

---

## 0. 阅读指引与证据分级

| 标记 | 含义 | 取证方式 |
| --- | --- | --- |
| 【官方事实】 | 来自 gpui-kit / gpui-component / Zed / GPUI 官方仓库的**源码、官方文档、官方 issue 与 PR** | 直接抓取仓库文件与 GitHub API |
| 【社区经验】 | 来自第三方 GPUI 应用维护者的 README、issue 讨论、实测数据 | 抓取第三方仓库 README 与 issue |
| 【推断】 | 由上述事实推导出的结论，非任何来源的直接陈述 | 明确标注 |

**未能验证的部分（全篇如实标注，不编造）**

- Showcase 应用的 **preview 截图**为 PNG 二进制，本次调研以纯文本方式抓取，**无法读取图像内容**，因此各应用的「界面形态」除第 1.4 节九个重点标杆（有 README/文档级文字证据）外，其余均只能依据 manifest 描述推断。
- gpui-kit 官方站点 `/apps` 分页页面为 JS 渲染的 Astro SSR，权威清单以 showcase 仓库的 `manifest.json` 为准（见 1.1）。
- 旧版独立仓库 `longbridge/gpui-component` 已被单仓 `longbridge/gpui-kit` 取代，引用时注意区分（见 2.7）。

---

## 一、gpui-kit Showcase 应用全景

### 1.1 权威数据源与口径【官方事实】

| 来源 | URL | 说明 |
| --- | --- | --- |
| 应用总览页 | <https://gpui-kit.com/apps/> | Astro SSR，含 85 个应用、10 页分页；站点版本 v0.8.0（版本下拉含 v0.7.3 … v0.6.0） |
| Showcase 仓库 | <https://github.com/longbridge/gpui-kit-showcases> | **权威清单源**：每个应用目录含 `manifest.json` 与 `preview*.png` |
| 单应用 manifest | `https://raw.githubusercontent.com/longbridge/gpui-kit-showcases/main/apps/<id>/manifest.json` | 字段：`id` / `name` / `author` / `description` / `category` / `platforms` / `website` / `source` / `building` / `publishedAt` / `previews` / `stars` |
| Featured 列表 | <https://raw.githubusercontent.com/longbridge/gpui-kit-showcases/main/featured.json> | 22 个 featured ID（站点首页重点展示位） |
| 提交规范 | <https://raw.githubusercontent.com/longbridge/gpui-kit-showcases/main/CONTRIBUTING.md> | manifest 字段规范 |

`featured.json` 的 22 个 ID（原样）：
`longbridge-pro` · `openlogi` · `disktree` · `zedis` · `wake` · `tty7` · `ghostex` · `omarchist` · `dbflux` · `openprocmon` · `scope` · `reviu` · `cadence` · `longbridge-lite` · `pawse` · `omasend` · `zenclash` · `orrery` · `aloud-ink` · `tes` · `overlook` · `top`

> 注：`zenclash` 在 `featured.json` 与 `apps/` 目录中均为下划线形式。

### 1.2 分类统计（按 85 份 manifest 的 `category` 字段统计）【官方事实】

| 分类 | 数量 | 站内展示名 |
| --- | --- | --- |
| `dev` | 36 | Developer Tools |
| `work` | 23 | Productivity & Media |
| `system` | 17 | System & Desktop |
| `terminal` | 9 | Terminal & CLI |
| **合计** | **85** | — |

平台覆盖（据 manifest `platforms`）：仅 macOS 的约 25 个、三平台（macOS/Windows/Linux）约 30 个、含 Windows 独占约 8 个、含移动端（Android/iOS）1 个（Vibex）。

### 1.3 全量清单（85 个，按分类排序）

> 列含义：名称 / 作者 / 分类 / 支持平台 / GitHub 星数（`—` = manifest 未提供）/ 用途简介（manifest 原文）/ 主页或源码链接。

#### Developer Tools（`dev`，36 个）

| 名称 | 作者 | 平台 | ★ | 用途简介 | 链接 |
| --- | --- | --- | ---: | --- | --- |
| Arc | Muhammad-Owais-Warsi | Windows | 22 | A native API client with request editing, environments, collections, a response viewer, and stress testing. | https://github.com/Muhammad-Owais-Warsi/arc |
| Based | pavi2410 | macOS/Windows/Linux | 74 | A local-first, Git-friendly database client. Connections and saved queries live in a committed .based/ directory, with no backend service. | https://github.com/pavi2410/based |
| Blanco | zanmato | macOS/Windows/Linux | 2 | A cross platform SQL editor and database client for PostgreSQL, MySQL, SQLite, DuckDB, SQL Server, CockroachDB, ClickHouse and Redis. | https://github.com/zanmato/blanco |
| Broquest | zanmato | macOS/Windows/Linux | 14 | A local-first API client with JavaScript scripting and secrets management. | https://github.com/zanmato/broquest |
| Cellar | MRL-00 | macOS/Windows/Linux | 152 | An open-source desktop database client built with Rust and GPUI. Browse schemas, run SQL, inspect execution plans, edit result grids and review pending changes before they are committed. | https://github.com/MRL-00/cellar |
| Coco MCP | camiloazula | macOS/Linux | 10 | Inspect and debug MCP servers in depth… connect many stdio or HTTP servers, call tools from schema forms or raw JSON, read every wire message, replay history and diff each connect against the last snapshot. | https://github.com/camiloazula/coco-mcp |
| DbCat | forzen | macOS | — | A fast, native macOS database client written in Rust. Browse, query and edit MySQL, MariaDB, PostgreSQL, SQLite, ClickHouse, Redis and MongoDB in one app, with paginated grids, a smart SQL editor, structure editing and import/export. | https://dbcat.forzens.com |
| DBFlux | 0xErwin1 | macOS/Windows/Linux | 486 | A keyboard-first database client for relational and non-relational stores, with charts, dashboards, Lua scripting and MCP integration. | https://github.com/0xErwin1/dbflux |
| Dodo | MrGru | macOS/Windows/Linux | 7 | A native developer toolbox built with GPUI Kit, combining API exploration, container management, databases, diagrams, utilities, and multilingual input tools in one desktop app. | https://github.com/MrGru/dodo |
| DuckLocal | JetSquirrel | macOS | 21 | A local-first workspace for querying CSV, Parquet, JSON and Excel files with DuckDB SQL. | https://github.com/JetSquirrel/DuckLocal |
| Farcaster | behzade | macOS/Linux | 67 | A native desktop client for coding agents. Run Codex, Pi, OpenCode and Cursor sessions side by side in one keyboard-driven UI, review changes in an embedded terminal and Neovim. | https://github.com/behzade/farcaster |
| Fulgur | fulgur-app | macOS/Windows/Linux | 58 | A lightning-fast native text editor: syntax highlighting for 60+ languages, SSH remote editing, Markdown and CSV views, and end-to-end encrypted sync. | https://github.com/fulgur-app/Fulgur |
| GetCat | finch-xu | macOS/Windows/Linux | 46 | A Postman-style API debugging tool, no Electron or WebView. Handles huge responses smoothly, renders SSE and LLM streaming live, imports and exports cURL. | https://github.com/finch-xu/GetCat |
| Ghostex | maddada | macOS/Windows/Linux | 1402 | The native desktop app for Claude Code, Codex, OpenCode and every other coding agent. A Rust/GPUI shell with a chat view per agent, embedded Chromium browser, Kanban board, built-in IDE, session search and remote machines. | https://github.com/maddada/Ghostex |
| Gupi | suxiaoshao | macOS/Windows/Linux | 5 | A native desktop workspace for the Pi coding agent. Organize project conversations, follow model and tool activity, explore history branches. | https://github.com/suxiaoshao/gupi |
| Hadron | s0lda | macOS/Windows/Linux | 27 | A multi-agent execution environment: isolated Git worktrees, an automatic merge gate, interactive terminals and per-agent telemetry. | https://github.com/s0lda/hadron |
| IotMock | startewho | Windows | 5 | A Modbus TCP simulator with editable coils and registers, byte-order controls, request and response logs, and packet inspection tools. | https://github.com/startewho/IotMock |
| möbius | thinkingsand | macOS | — | A self-hosted home for coding agents. Chat with your Bots, review their diffs, files and terminals in side tabs. | https://mobius.thinkingsand.dev |
| Navop | feigeCode | macOS/Windows/Linux | 1901 | A native, all-in-one developer workspace that brings databases, SSH, SFTP, remote desktops, monitoring, notes, and AI agents into one GPU-accelerated desktop app built with Rust and GPUI. | https://github.com/feigeCode/navop |
| NetAssistant | SunJary | Windows/Linux/macOS | 70 | Cross-platform TCP/UDP network debugging tool with client and server modes, hex editing with checksums, message favorites and search, reply rules, scheduled sending and a built-in stress test engine. | https://github.com/SunJary/NetAssistant |
| OpenProcMon | progmboy | Windows | 356 | An open-source Windows Process Monitor: a kernel miniFilter driver, Procmon-compatible PML capture and replay, and an MCP interface. | https://github.com/progmboy/openprocmon |
| Orrery | Hankanman | Linux | 11 | A command center for every git repository in your dev directories: live status in one dense grid, host enrichment, on-device AI summaries, and one-click launch into an IDE or terminal agent. | https://github.com/Hankanman/Orrery |
| paperoll | Envl | macOS/Windows/Linux | 9 | A native scratchpad for code snippets, organized into persistent tabbed rolls and numbered pages, with language detection, syntax highlighting, and formatting. | https://github.com/Envl/paperoll |
| Probe | crizant | macOS/Windows/Linux | 52 | A fast, native, local-first API client. Stores collections as Git-friendly OpenCollection YAML, ships a matching CLI for scripts, CI and AI agents. | https://github.com/crizant/probe |
| Protide | dreygur | macOS/Linux | 6 | An API testing tool covering HTTP, GraphQL, WebSocket, gRPC, tRPC and Socket.IO, with mock servers and local-first P2P collaboration. | https://github.com/dreygur/protide |
| Reviu | reviu-dev | macOS/Windows/Linux | 96 | A native desktop workspace for running coding agents and reviewing what they write: sessions on the left, conversations, diffs and terminals in split panes, and a real Git client in the dock. | https://github.com/reviu-dev/reviu |
| RustGrid | yisier | macOS/Windows/Linux | 359 | A Navicat-style database GUI built in Rust with GPUI: browse and edit MySQL, PostgreSQL, SQL Server, Oracle and SQLite databases, run SQL, and back up. | https://github.com/yisier/rustgrid |
| Setu | bajrangCoder | macOS/Windows/Linux | 56 | A minimal REST client with layered Global, Workspace and Project variables, multiple workspaces, Postman collection import and keyboard shortcuts throughout. | https://github.com/bajrangCoder/setu |
| Shouting Robin | zanmato | macOS/Windows/Linux | 6 | An SEO crawler for e-commerce sites, crawling over plain HTTP or a real Chrome through spider-rs. | https://github.com/zanmato/shouting-robin |
| Spur | press-again | Windows/macOS | 7 | A native Git client for Windows and macOS that monitors multiple repositories. Browse commit graphs and diffs, stage changes, manage branches and stashes, and resolve conflicts. | https://github.com/press-again/SpurGit |
| Threadlane | wheregmis | macOS/Linux | 21 | A native workspace for AI-assisted software development. Organize project conversations, work with coding agents, and use integrated terminals, file tools and Git review panels. | https://github.com/wheregmis/threadlane |
| Tusk | Alpcan Aydın | macOS/Windows/Linux | 408 | A native, keyboard-driven database client built with Rust and GPUI Kit. Connect to SQL and NoSQL databases, edit data, manage schemas, and write queries with an AI assistant. | https://github.com/alpcanaydin/tusk |
| Vibex | peatboy | macOS/Windows/Linux/Android/iOS | 33 | A native, local-first AI coding workbench for Agent-powered software development. | https://github.com/vibex-ai/vibex |
| Wake | iAmCorey | macOS/Linux/Windows | 1485 | All your AI coding-agent sessions in one place. Browse by agent or project, full-text search with CJK and code support, read transcripts, and resume any session in your terminal. | https://github.com/iAmCorey/Wake |
| xvw | funap | macOS/Windows/Linux | 16 | A GPU-accelerated hex and binary editor for reverse engineering: memory-mapped multi-GB files, runtime Kaitai Struct parsing, a 2D visual map, synchronized diffs, a live data inspector. | https://github.com/funap/xvw |
| Zedis | vicanso | macOS/Windows/Linux | 2163 | A native Redis GUI that opens a million-key database without a spinner: virtual-scrolled SCAN, typed value viewers, a memory analyzer and live metrics. | https://github.com/vicanso/zedis |

#### Productivity & Media（`work`，23 个）

| 名称 | 作者 | 平台 | ★ | 用途简介 | 链接 |
| --- | --- | --- | ---: | --- | --- |
| Aloud Ink | Aloud Ink | macOS | — | A native macOS dictation app: hold a global shortcut to speak, release to get clean, filler-free text at your cursor in any app. | https://aloud.ink/ |
| BuMoSm PDF Viewer | nabehiro | Windows | — | A simple PDF viewer for Windows built with Rust and GPUI. Smooth scrolling, tabs, text and regex search, bookmarks, a page editor, a print preview screen, invert colors, and custom themes. | https://github.com/BuMoSm-Series/BuMoSmPDFViewer |
| Cadence | infomiho | macOS | 74 | A minimal native Spotify player for macOS. | https://github.com/infomiho/cadence |
| Castle | BeratHundurel | Windows | 13 | A native notes and kanban app for Windows: Markdown notes with an outline view, boards with cards, labels, checklists and due dates, kept in a local SQLite database. | https://github.com/BeratHundurel/castle |
| Chelete | phoscoder | Linux | 1 | A keyboard-driven personal finance tracker for Linux. | https://github.com/phoscoder/chelete |
| CloudBridge | JetSquirrel | macOS/Windows/Linux | 62 | A local-first desktop ledger for cloud and AI model spend. | https://github.com/JetSquirrel/cloudbridge |
| Coop | reya | macOS/Windows/Linux | — | A Nostr direct-message app, built on a fork of the component library. | https://git.reya.su/reya/coop |
| FluxDown | zerx-lab | Windows/macOS/Linux | 4935 | A free, open-source download manager and IDM alternative powered by a Rust engine. HTTP/FTP multi-segment acceleration, BitTorrent and magnet links, HLS/DASH streams, queues, RSS auto-download and browser capture, with zero ads. | https://github.com/zerx-lab/FluxDown |
| Glim | NeoPanic | macOS | 24 | A free, open-source file viewer for macOS. Read Markdown, PDFs, images, JSON, CSV and source code in one workspace. Built with Rust and GPUI Kit. | https://github.com/NeoPanic/Glim |
| Longbridge Lite | longbridge | macOS/Windows/Linux | 67 | A market-reading Longbridge client made for Omarchy — the reference for running a JavaScript application natively through GPUI Shell. | https://github.com/longbridge/longbridge-lite |
| Longbridge Pro | Longbridge | macOS/Windows/Linux | — | The trading desktop GPUI Kit was built for. Real-time quotes, charts and dense market data, shipped on all three platforms. | https://longbridge.com/desktop |
| Meow Player | xarzhi | Windows | — | A Windows native music player with a simple and elegant interface. | https://xarzhi.github.io/meow-player-docs/ |
| NearWord | Sergei G. | macOS/Windows/Linux | — | Look up words and phrases anywhere on your screen without switching apps. | https://nearword.app/ |
| Notey | Kronz | macOS | 7 | A simple, fast plain-text editor for macOS with tabs, session restore, live Markdown preview, PDF/DOCX export, find and replace, and light/dark theming. | https://github.com/kronzhq/notey |
| Overlook | Last Refuge | Linux | 4 | A native Gmail and Google Calendar client for Linux, with an Outlook-inspired workspace, local-first offline mail, a durable Outbox, and Light and Dark themes. | https://github.com/last-refuge/overlook |
| Panda Reader | yuhangch | Windows/macOS/Linux | 22 | A desktop RSS reader built with GPUI. Keep feeds in a local library or sync with Miniflux or FreshRSS. Read offline, extract full articles, translate side by side, and tune typography and themes. | https://github.com/yuhangch/panda-reader |
| Pawse | popovpsk | macOS/Windows/Linux | 76 | A native player for a local music library: bit-perfect playback matching sample rate and bit depth, FLAC through DSD, time-synced lyrics and a built-in web remote. | https://github.com/popovpsk/pawse |
| Picoo Camera | Haoxincode | macOS/Windows | 6 | Turn an Android phone or iPhone into a wireless desktop webcam over your local network for video meetings and OBS. | https://github.com/Haoxincode/picoo-camera |
| Pomelo Desktop | HaiwenZhang | macOS/Windows/Linux | 5 | A PCB viewer based on GPUI and GPUI Kit, supporting Cadence Allegro, Altium Designer, ODB++, PADS, Ansys HFSS 3D Layout, and KiCad. | https://github.com/HaiwenZhang/Pomelo-Desktop |
| RailOxide | triamazikamno | macOS/Windows/Linux | 23 | A privacy-oriented desktop wallet for RAILGUN private transactions, written in Rust on GPUI. | https://github.com/triamazikamno/railoxide |
| Scope | scopeclient | macOS/Windows/Linux | 178 | A native Discord client built for power users. | https://github.com/scopeclient/scope |
| tbis | mahi160 | macOS | — | A native macOS client for Jellyfin, built with GPUI and playing video through an embedded mpv. | https://github.com/mahi160/tbis |
| TES | GOODDASH | macOS/Windows/Linux | — | A desktop workspace for CNC thermal experiments, combining temperature and displacement data collection, visualization, correlation analysis, and regression modeling. | https://gooddash.github.io/tes-website/ |

#### System & Desktop（`system`，17 个）

| 名称 | 作者 | 平台 | ★ | 用途简介 | 链接 |
| --- | --- | --- | ---: | --- | --- |
| BongoCat | ayangweb | macOS/Windows | 24105 | A desktop pet for macOS and Windows. A Live2D cat lives on your screen, tracks your mouse, and plays a motion for every key you press. Settings and model library built with gpui-kit. | https://github.com/ayangweb/BongoCat |
| CLV3000 Plus | Sopaco | macOS/Windows | 11 | A fast, Rust-powered cleaner for Windows & macOS that reclaims space from AI-agent leftovers. | https://github.com/sopaco/CLV3000-Plus |
| disktree | tobi | Linux/macOS/Windows | 3576 | A treemap for finding and removing what fills your disk. Directories drawn as a nested mosaic sized by real disk cost and colored by kind of data, with reclaimable space hatched and marks reviewed before anything is trashed. | https://github.com/tobi/disktree |
| FerrisPass | elias-tilegant | macOS | 7 | A KeePass-compatible password manager that reads and writes KDBX 4 vaults, with TOTP, Auto-Type, auto-lock and a headless CLI. | https://github.com/elias-tilegant/ferrispass |
| Nohrs | noh-rs | macOS | 157 | A Raycast-style launcher and a keyboard-driven file explorer in one Finder alternative, extensible with sandboxed WASM plugins and its own search index. | https://github.com/noh-rs/nohrs |
| Omafiles | aloisdeniel | Omarchy Linux | 35 | The file explorer Omarchy was missing: keyboard-first and retinted live by your theme, with vertical tabs and workspaces, fuzzy and content search, tree-sitter previews, Git markers and diffs. | https://github.com/aloisdeniel/omafiles |
| Omarchist | tahayvr | Omarchy Linux | 770 | The configuration and theme designer for Omarchy Linux, with visual theme editing, live previews and a built-in theme collection. | https://github.com/tahayvr/omarchist |
| Omasend | huacnlee | macOS/Windows/Linux | 42 | Copy, paste, and send to your other devices. A local-network file sharing app for Omarchy. | https://github.com/huacnlee/omasend |
| OpenLogi | AprilNEA | macOS/Windows/Linux | 24003 | A local-first alternative to Logitech Options+. Remap buttons, DPI and SmartShift over HID++ — with no account and no telemetry. | https://github.com/AprilNEA/OpenLogi |
| Piku | pikachuu184 | macOS/Windows/Linux | 22 | A fast, monochrome, keyboard-driven file manager with tabbed and split workspaces and rich previews for images, PDFs, code and archives. | https://github.com/pikachuu184/Piku |
| Scope | khurrambhutto | Linux | 24 | A Linux desktop app that puts apps from APT, Snap, Flatpak, AppImage, and desktop launchers into one searchable list, with preview-first uninstall and update. | https://github.com/khurrambhutto/scope |
| Shotori | mengh04 | Linux | 14 | A Wayland-native screenshot tool — select, annotate, pin, long-capture scrolling pages, and read text with on-device OCR. | https://github.com/shotori-screenshot/shotori |
| Steward | iFence | Windows | 14 | A small Windows launcher and plugin platform: a global hotkey, fuzzy search over a SQLite app index, and plugins written in JavaScript over JSON-RPC. | https://github.com/iFence/steward |
| System Pulse | eas4ai | macOS/Linux | 13 | A native dockable desktop system monitor with live CPU, memory, GPU, disk, network, energy and thermal charts, a searchable process table, and a tray graph. | https://github.com/eas4ai/system-pulse |
| Top | bshk-app | macOS | 9 | A native system monitor for Apple Silicon Macs, with real-time CPU, GPU, memory, disk, and network metrics, a dashboard, menu bar indicators, a floating widget, and a process list. | https://github.com/bshk-app/top |
| TopQ | dirkvdb | macOS/Windows/Linux | 2 | MQTT broker topic explorer. With live topic tree, payload pane with diff support and value monitoring with charts. | https://github.com/dirkvdb/topq |
| Widget-RS | tierlabx | Windows | 9 | Modular desktop widgets for Windows, including sticky notes, to-do lists, break reminders, and desktop fences, with snapping and persistent layouts. | https://github.com/tierlabx/widget-rs |

#### Terminal & CLI（`terminal`，9 个）

| 名称 | 作者 | 平台 | ★ | 用途简介 | 链接 |
| --- | --- | --- | ---: | --- | --- |
| Baudrun | packetThrower | macOS/Windows/Linux | 32 | A serial terminal for switch consoles and router CLIs, with saved device profiles, auto-reconnect, safe paste and XMODEM/YMODEM transfers. | https://github.com/packetThrower/Baudrun |
| KazeTerm | bikesheddev | macOS/Windows/Linux | — | A lightweight terminal inspired by Windows Terminal and built around Alacritty, with tabs, theme overlays and swappable terminal backends. | https://github.com/bikesheddev/kazeterm |
| Miaominal | cppakko | macOS/Windows/Linux | 59 | A desktop SSH workspace for remote development and server operations, combining terminal sessions, SFTP transfers, port forwarding, configuration sync, and secure credential storage. | https://github.com/cppakko/miaominal |
| Nyx | BX-Team | Windows/Linux | 25 | A lightweight desktop GUI for the Mihomo proxy core, with profiles, proxy groups, rules, TUN mode and a connection inspector. | https://github.com/BX-Team/Nyx |
| Oxidal | sh4den | macOS/Windows/Linux | 26 | A native SSH session manager for people who avoid Electron: organized connections, an integrated terminal and local-only configuration. | https://github.com/sh4den/Oxidal |
| ShellRS | since2006 | macOS/Windows/Linux | 416 | A native SSH client in Rust: Xshell-style host management and tabbed terminals, WinSCP-style dual-pane SFTP, port forwarding, sidebar tools for processes, services and Docker. | https://github.com/since2006/shell-rs |
| tty7 | l0ng-ai | macOS/Linux | 1321 | A terminal workbench in pure Rust: persistent sessions, SSH, remote work and coding agents, over Alacritty's VT core. | https://github.com/l0ng-ai/tty7 |
| WSL2 Hyper-V Firewall Manager | ssut | Windows 11 | 31 | A GUI for WSL2's Hyper-V firewall rules: automatic port discovery, a git-like draft-and-commit workflow and snapshot rollback. | https://github.com/ssut/WSL2-HyperV-Firewall-Manager |
| ZenClash | HaiwenZhang | macOS/Windows/Linux | 41 | A desktop client for the Mihomo proxy core. | https://github.com/HaiwenZhang/ZenClash |

### 1.4 与「短剧 / 视频流播放 + 列表浏览 + 设置 + 下载任务」最相关的九个标杆

以下九个均有 README 或官方文档级文字证据，界面形态描述基于其文字证据；未验证的截图细节已注明。

---

#### ① tbis —— 视频播放壳（最贴近「播放页 + 媒体库列表」）

**界面形态**【社区经验】：<https://github.com/mahi160/tbis> README

- 定位：macOS 原生 Jellyfin 客户端，**GPUI 壳 + 内嵌 mpv 播放视频**（GPL mpv/FFmpeg 随包分发）。
- 自述性能实测（MacBook Air M4 / 16GB / macOS 27.2，2026-10）：
  - 启动到首窗口 **94 ms**，对比 Jellyfin Media Player 851 ms、Moonfin 671 ms；
  - 首页空闲内存 **258 MB**，对比 461 / 416 / 775 MB；
  - 播放中 CPU 24%（对比 35% / 25% / 11%），空闲 CPU 2.3%（Moonfin 32%）；
  - 是唯一能直放 67 Mbps 4K HEVC Dolby Vision 原片的客户端。
- 交互：支持系统菜单栏「Check for Updates…」、应用内更新（下载→校验 checksum→替换→重启）。

**可借鉴点**

| 维度 | 借鉴 |
| --- | --- |
| 布局骨架 | 「媒体库列表 → 详情/播放页」两级；播放层与 GPUI 界面同窗口共存 |
| 导航形态 | 菜单栏集成 + 应用内更新入口，桌面应用惯例 |
| 播放器控制条 | 播放能力由 **mpv 承担**（外部播放器内核），GPUI 只做界面——这是「GPUI 壳 + 独立播放内核」的范式，与红果 `hongguo-player`（macOS AVPlayer / Windows MediaEngine）**架构同构** |
| 列表/卡片 | 未在 README 描述，**未验证** |
| 弹窗体系 | 未在 README 描述，**未验证** |
| 主题与密度 | 未在 README 描述，**未验证** |

> 对红果的直接价值：**播放内核与 GPUI 界面解耦**是被实测验证过的可行路线；播放时 CPU/内存开销应作为 GPUI 壳的验收指标之一。

---

#### ② FluxDown —— 下载管理器（最贴近「下载任务 + 队列 + 进度」）

**界面形态**【社区经验】：<https://github.com/zerx-lab/FluxDown> README（4.9k★，AGPL-3.0）

- 桌面端是 Rust + GPUI（`crates/app`），与 Agent/Daemon/Engine 三层分离：
  `fluxdown-desktop → fluxdown-agent → fluxdownd → fluxdown_engine`。
- 引擎层（`native/engine`）**不依赖 UI 与 FFI**，被桌面 / 移动 / 服务端 / CLI 共用。
- UI 能力：命名队列（named queues）、任务详情、分片可视化、light/dark + 自定义主题、**tray 常驻**（关闭 UI 后服务继续跑）、SQLite WAL 持久化 + 可恢复状态。

**可借鉴点**

| 维度 | 借鉴 |
| --- | --- |
| 布局骨架 | 「任务列表（主区）+ 任务详情（面板）」；列表与详情分离 |
| 导航形态 | 命名队列 = 分组导航；tray 常驻 = 后台任务不随窗口消失 |
| 播放器控制条 | 不适用（无播放） |
| 列表/卡片 | 任务行含 进度/速度/状态；分片可视化 |
| 弹窗体系 | 设置 → API Service 开关式分组；首启向导（access key） |
| 主题与密度 | light/dark + 用户自定义主题 |
| 架构 | **UI 只发命令、进度靠事件回灌**——与红果 `ServiceEvent` 事件泵同构；引擎与 UI 分离可直接映射到红果 `hongguo-service` ↔ `hongguo-gpui` 的现有分层 |

---

#### ③ Zedis —— 大数据量列表 + ⌘K + 左右分栏

**界面形态**【社区经验】：<https://github.com/vicanso/zedis> README（2.1k★）

- 关键屏（README 截图说明）：key browser（左 namespace 树 + 右 JSON/语法高亮值编辑器）、memory analyzer（Top-N 表 + TTL 直方图）、live metrics（实时图表）、geo map、vector set + KNN、**command palette（⌘K）**。
- 性能自述：SCAN 虚拟滚动，**百万 key 保持 60+ FPS**，内存占用小。
- GPUI Fast 渲染引擎对比实测（macOS）：滚动 Config/Metrics 页 **CPU 约低 20%**，表格与编辑器低几个百分点，key tree 相同，**内存约多 20%**。

**可借鉴点**

| 维度 | 借鉴 |
| --- | --- |
| 布局骨架 | 左侧树 + 右侧分栏（编辑器/图表/详情可切换）——与红果「分类树 + 剧集列表 + 详情」结构同构 |
| 导航形态 | ⌘K 命令面板 + 命名空间树 |
| 播放器控制条 | 不适用 |
| 列表/卡片 | 虚拟滚动撑百万级；Top-N 表 + 直方图 |
| 弹窗体系 | 生产连接默认写锁；破坏性命令**要求输入服务器名确认**——红果「删除下载任务/清空缓存」可直接套用这种「输入名称确认」范式 |
| 主题与密度 | 见 2.3 令牌体系；gpui-fast 是可选渲染引擎（0.8.0 起 feature） |

---

#### ④ disktree —— 网格/马赛克 + 可拖拽侧栏 + 键盘优先（**封面网格的最佳范式**）

**界面形态**【社区经验】：<https://github.com/tobi/disktree> README（3.6k★，MIT，featured）

- 界面三段式（README「The screen」原文描述）：
  1. **Top**：面包屑路径 + 度量方式（Size / Files / Age / Hidden / Apparent size / 深度）；
  2. **Under it**：扫描总量、过滤器、图例；
  3. **Mosaic**：目录画成嵌套马赛克，**颜色表示数据类别**（code / agent scratch / toolchains / synced files / git / media / documents / caches），统一 muted 色阶、越深越亮；**斜纹 = 可回收空间**（与颜色正交）；顶层目录带色条 + 名称带，深层用细标签行；
  4. **Panel（左侧，可拖拽左边线调整宽度，双击边线复位）**：选中项大小/占比/文件数/最后写入、*Worth a look*、已标记列表、磁盘剩余空间。
- 颜色纪律：**只有一种颜色被单独保留（琥珀色）= 选中 / 主操作 / 可回收**；标记项用 danger 色。
- 键盘优先：方向键在同级 tile 间移动、`tab` 跳到下一个最大兄弟、Enter 进入、Backspace/Esc 上一级、`/` 按名过滤、`?` 打开全部快捷键说明面板、`?` 面板是自带的快捷键索引。
- 布局算法：**squarified treemap**（实现于 `crates/disktree-core`，与 UI 分离）。
- 尺寸纪律：所有尺寸走**单一 rem 标度**，界面缩放保持比例；明确遵循 gpui-kit design guides（引用 <https://gpui-kit.com/versions/main/docs/design-guides/>）：「primary 只留给 Enter 会做的事」「应用只问它无法收回的那一个问题」。
- 进度纪律：扫描中**不猜百分比**（总量在遍历结束前未知）；进度用「单一可替换快照」而非增长的事件队列；路径更新节流到每秒 5 次；计数用等宽字体固定列宽，数值变化不重排版面。

**可借鉴点（对红果封面网格最有价值的一条）**

| 维度 | 借鉴 |
| --- | --- |
| 布局骨架 | 顶部路径/度量条 → 主区马赛克 → 右侧信息面板（可拖拽、可复位） |
| 导航形态 | 键盘优先 + `?` 快捷键面板；面包屑 + 侧栏双通道 |
| 播放器控制条 | 不适用 |
| 列表/卡片 | **类别着色 + 斜纹标记（正交编码）** 是封面网格最省事的视觉语言：色相=题材/分类，纹理=状态（如「已下载 / 未下载 / 更新中」），比给每张封面加角标干净得多 |
| 弹窗体系 | 删除前统一进「复核列表页」，不是即时确认弹窗；永久删除才二次确认且**必须写明删什么、找回多少** |
| 主题与密度 | rem 单一标度 + 主色只给主操作 + 计数等宽定宽列 |

---

#### ⑤ Panda Reader —— 内容流 + 排版主题（阅读/详情页范式）

**界面形态**【社区事实，仅 manifest】：本地库 / Miniflux / FreshRSS 三种源；离线全文提取；**并列翻译**（左右对照）；可调排版与主题。
**可借鉴点**【推断】：并列翻译 = 左右分栏双滚动的实现范式；「调排版与主题」说明阅读型详情页把 **字号/行高/行宽/主题** 暴露为用户可调项——红果剧集详情页的简介/剧文本可沿用。
**未验证**：具体界面形态（README 未抓取）。

---

#### ⑥ Pawse / Cadence / Meow Player —— 播放器控制条与播放列表

**界面形态**

- Pawse【社区事实，仅 manifest】：bit-perfect 播放（采样率与位深对齐）、FLAC through DSD、**时间同步歌词**、内置 Web remote。
- Cadence【社区事实，仅 manifest】：极简 Spotify 播放器（macOS）。
- Meow Player【社区事实，仅 manifest】：Windows 原生音乐播放器，界面简洁优雅。

**可借鉴点**【推断】

- 三个播放器一致的骨架：**左侧播放列表 / 右侧当前曲目 + 封面 + 控制条**（典型的两栏媒体应用布局）。
- Pawse 的「时间同步歌词」= 需要一条**高频刷新（~10Hz）的时间轴**；红果弹幕/进度同理——这条与 GPUI 渲染帧预算直接相关（见第四节第 11 条）。
- 「Web remote」= 播放状态可被外部读取（红果对应能力：播放状态事件已在 `ServiceEvent` 中）。
**未验证**：具体控件排列、波形/进度条样式。

---

#### ⑦ Orrery —— 密集网格总览

**界面形态**【社区事实，仅 manifest】：所有 git 仓库在一个**密集网格（dense grid）**中展示实时状态 + 主机信息 + 本地 AI 摘要 + 一键启动 IDE/终端。
**可借鉴点**【推断】：「密集网格」= 小行高 + 高信息密度 + 状态色点，是**收藏/剧集库总览页**的合适范式（而非大卡片流）。
**未验证**：网格是等高网格还是瀑布流——**未查到**。

---

#### ⑧ Ghostex / Reviu —— 多标签与分栏工作区

**界面形态**

- Ghostex【社区事实，仅 manifest】：每个 agent 一个 chat view、内嵌 Chromium、Kanban 看板、内置 IDE、会话搜索、远程机器。
- Reviu【社区事实，仅 manifest】：**左会话列表 + 右分屏（对话 / diff / 终端）+ dock 内 Git 客户端**。

**可借鉴点**【推断】

- Reviu 的「左列表 + 右分屏」与 gpui-kit 的 **ResizablePanelGroup（`h_resizable` / `v_resizable`）** 是同一套原语：红果「剧集列表 + 详情」可用 `h_resizable`；「详情 + 弹幕/评论」用 `v_resizable`。
- dock 模块（gpui-component `dock` + gpui-base `dock/`）可直接承载「下载任务 / 收藏 / 历史」这类常驻侧栏。

---

#### ⑨ Longbridge Pro —— gpui-kit 的孵化场景（密集数据 + 三平台一致性）

**界面形态**【社区事实，仅 manifest】：实时行情、图表、**密集市场数据**，Windows / macOS / Linux 三平台同发。
**可借鉴点**【推断】：gpui-kit 的组件与令牌是**为「数据密集型桌面应用」设计的**，不是为「营销落地页」设计的——红果 GPUI 壳的信息密度可以按这个标准定，而不是按 WebView 壳的大卡片风格定。
**配套参考**：Longbridge Lite 是 **GPUI Shell（JS 扩展运行时）**跑原生 JS 应用的参考实现（<https://github.com/longbridge/longbridge-lite>）。

---

## 二、gpui-kit / gpui-component 仓库本体

### 2.1 仓库结构【官方事实】

仓库：<https://github.com/longbridge/gpui-kit>（18,567★ / 1,139 fork / 119 open issues，最近 push 2026-10-24）

三层结构（README / AGENTS.md）：

```
gpui-kit（umbrella，pins GPUI 快照 gpui-pre-*）
├── gpui-base        无样式的行为 / 状态 / 基础设施
├── gpui-component   带样式的组件库（shadcn 风格）
└── gpui-shell       JS 扩展运行时（可选，未发布到 crates.io）
```

`crates/` 实际目录【官方事实】——<https://api.github.com/repos/longbridge/gpui-kit/contents/crates>：
`assets` · `base` · `component` · `component-macros` · `fps` · `kit` · `shell` · `story` · `story-web` · `webview`

- README：<https://raw.githubusercontent.com/longbridge/gpui-kit/main/README.md>
- AGENTS.md：<https://raw.githubusercontent.com/longbridge/gpui-kit/main/AGENTS.md>
- 发布说明：<https://raw.githubusercontent.com/longbridge/gpui-kit/main/release-notes.md>

### 2.2 组件清单（完整，按功能归类）【官方事实】

来源：`crates/component/src/lib.rs` 的 `pub mod` 声明（共 **68 个公开模块**）
<https://raw.githubusercontent.com/longbridge/gpui-kit/main/crates/component/src/lib.rs>

| 类别 | 模块 |
| --- | --- |
| **布局 / 导航** | `sidebar` `tab` `dock` `resizable` `scroll` `nav_stack`(base) `tabs`(base) |
| **表单控件** | `button` `checkbox` `radio` `select` `combobox` `switch` `slider` `stepper` `rating` `input` `number_input`(base) `date_picker` `time_field` `calendar` `color_picker` `otp_input`(base) `questionnaire` `form` `clipboard` |
| **内容展示** | `text` `label` `link` `tag` `badge` `avatar` `attachment` `bubble` `diff` `highlighter` `chart` `plot` `marker` `separator` `description_list` `group_box` `kbd` `speech` `empty` `skeleton` `shimmer` `spinner` `progress` `carousel` `breadcrumb` |
| **列表 / 数据** | `list` `searchable_list` `virtual_list` `table` `tree` `history` `message_scroller` `pagination` |
| **浮层 / 反馈** | `dialog` `sheet` `popover` `hover_card` `menu` `native_menu` `tooltip` `alert` `message` `notification` `bubble` |
| **主题 / 其他** | `theme` `accordion` `collapsible` `setting` `toolbar` `status_bar` `command` `chart` |

顶层 re-export 的关键类型：`VirtualList` / `VirtualListScrollHandle` / `v_virtual_list` / `h_virtual_list` / `ResizablePanel` / `ResizablePanelGroup` / `h_resizable` / `v_resizable` / `Root` / `GlobalState` / `Rope` / `RopeLines` / `IndexPath` / `IconName` / `WindowBorder` / `window_paddings`。

**三个对红果直接有用的发现：**

1. **`command` 模块存在** —— gpui-component 自带命令面板组件（`pub mod command;` 且 `init(cx)` 中注册）。Zedis 的 ⌘K 即是这类实现。**红果不需要从零做 Command Palette。**
2. **i18n 已内置** —— `crates/component/src/lib.rs` 末尾：
   ```rust
   rust_i18n::i18n!("locales", fallback = "en");
   pub fn locale() -> impl Deref<Target = str>;
   pub fn set_locale(locale: &str);
   ```
   即组件库自带 `locales/` 资源与 locale 切换入口，直接对接红果的 zh-CN / en-US 双语要求。
3. **必须调 `gpui_component::init(cx)`** —— 入口装配时需初始化 theme / global_state / root / input / diff / date_picker / dock / sheet / list / command / carousel / notification / popover / questionnaire / menu / table / tooltip。

> 组件数量口径差异：README 称 "75+ Components and Primitives"，AGENTS.md 旧文案称 "60+"；按 `lib.rs` 统计为 68 个公开模块（base 层另有大量无样式原语）。三者均指同一套库，不冲突。

### 2.3 主题令牌体系（两层）【官方事实】

**第一层：语义令牌 `SemanticThemeTokens`** — <https://raw.githubusercontent.com/longbridge/gpui-kit/main/crates/base/src/theme_tokens.rs>

| 组 | 令牌名与取值 |
| --- | --- |
| `colors` | `background` `foreground` `surface` `surface_foreground` `primary`(+`primary_foreground`) `secondary`(+fg) `muted`(+fg) `accent`(+fg) `destructive`(+fg) `border` `input` `ring` `selection` |
| `radius` | `none`/`sm`/`md`/`lg`/`xl`/`full` = 0 / 3 / 6 / 8 / 12 / 9999 px |
| `spacing` | `xxs`/`xs`/`sm`/`md`/`lg`/`xl`/`xxl` = 2 / 4 / 8 / 12 / 16 / 24 / 32 px |
| `typography` | `sans`/`mono` + `xs`/`sm`/`md`/`lg`/`xl`/`mono_md` = 12 / 14 / 16 / 18 / 20 / 13 px，行高 16 / 20 / 24 / 28 |
| `shadow` | `sm` / `md` / `lg` |

**第二层：组件级 legacy 令牌 `ThemeColor` / `ThemeTokens`** — <https://raw.githubusercontent.com/longbridge/gpui-kit/main/crates/component/src/theme/theme_color.rs>

覆盖：`accent` `background` `border` `button`（主/次/危险/info/success/warning × active/hover/foreground）`chart_1..5` `chart_bullish` `chart_bearish` `chart_grid` `danger` `foreground` `info` `input` `link`(+active/hover) `list`(+active/active_border/even/head/hover) `muted` `popover` `primary` `progress_bar` `ring` `scrollbar`(+thumb/hover) `secondary` `selection` `sidebar`(+accent/border/foreground/primary) `skeleton` `slider_bar` `slider_thumb` `success` `switch`(+thumb) `tab`(+active/active_foreground/bar/bar_segmented/foreground) `table`(+active/border/even/head/foot/hover/row_border) `title_bar`(+border) `status_bar`(+border) `warning` `overlay` `window_border` 及 `red`/`green`/`blue`/`yellow`/`magenta`/`cyan`(+`_light`)。

**内置主题**：`themes/` 目录含 17 套 —— adventure、alduin、ayu、catppuccin、flexoki、gruvbox、harper、hybrid、jellybeans、macos-classic、mellifluous、molokai、solarized、spaceduck、tokyonight、twilight；另有 schema 文件 `.theme-schema.json`（32 KB）。

**设计规范原文（规范性文档）**：<https://raw.githubusercontent.com/longbridge/gpui-kit/main/website/docs/design-guides.md>（52 KB）
涵盖：设计原则、层级、语义色用法、radius/spacing/密度表、spatial grammar、对齐轴线、密度档位、rem/zoom、shell 模式、表单与设置、**浮层选择层级**、motion、**数据密集界面**、界面语言与 i18n、可访问性清单。
配套：<https://raw.githubusercontent.com/longbridge/gpui-kit/main/website/docs/coding-guides.md>（50 KB）。

### 2.4 布局原语盘点【官方事实】

**有：**

| 能力 | 来源 |
| --- | --- |
| Flex（`h_flex` 交叉轴居中 / `v_flex` 拉伸）、padding / margin | `crates/base/src/styled.rs` |
| 滚动容器、滚动条、弹性滚动、遮罩 | `scrollbar.rs` `scroll_bounce.rs` `scrollable_mask.rs` |
| **虚拟列表（变高 / 定高）** | `crates/base/src/virtual_list.rs` → `v_virtual_list` / `h_virtual_list` |
| 可调分栏 | `crates/base/src/resizable/` → `h_resizable` / `v_resizable` |
| Dock | `crates/base/src/dock/` + `crates/component/src/dock.rs` |
| Tabs | `crates/base/src/tabs.rs` + `crates/component/src/tab.rs` |
| 导航栈（进栈/出栈） | `crates/base/src/nav_stack.rs` |
| 焦点陷阱 | `crates/base/src/focus_trap.rs` |
| 浮层：popup / positioner / anchored、dialog、sheet、popover、hover_card、menu、tooltip | `crates/base/src/{popup,positioner,popover,dialog,sheet,hover_card,tooltip}.rs` |
| 动效 / 减弱动效 | `crates/base/src/animation.rs` `motion.rs` `motion/` `reduce_motion.rs` |
| 观察 / 自动滚动 / Toast / 撤销历史 | `observe.rs` `auto_scroll.rs` `toast.rs` `undo_history.rs` `history.rs` |
| 文本输入（TextView / Rope / LSP / Tree-sitter） | `crates/base/src/input/` `text/` `text_selection.rs` |
| 可访问性 | `crates/base/src/a11y_disabled.rs` `macos_accessibility.rs` |

**没有（重要结论）：**

> **gpui-kit / gpui-component 中不存在 grid、masonry、瀑布流、封面网格模块。**
> `crates/component/src/lib.rs` 的 68 个 `pub mod` 中无 `grid`；`crates/base/src/styled.rs` 只有 flex 系列（`h_flex` / `v_flex`）与 padding / margin，没有 CSS Grid 式二维布局原语。
> 【官方事实】——这是「结论不是没查到」，而是通过模块声明确认**不存在**。

**含义【推断】**：红果的「短剧封面网格 / 瀑布流」**不能靠 gpui-kit 现成组件**，必须自建（见第五节第 3 档）。可行路线有两条：
1. 用 flex 行 + 固定卡片宽度 + `flex_wrap` 模拟等高网格（最省事，但列数需按窗口宽度自行计算）；
2. 自建瀑布流布局（照 disktree 的 squarified treemap 思路做列分配，见 3.2）。

### 2.5 示例与脚手架【官方事实】

| 用途 | 路径 |
| --- | --- |
| 桌面组件画廊（跑起来看效果） | `crates/story`（`cargo run`） |
| WASM 版画廊 | `crates/story-web` |
| 官方示例目录 | `examples/`（<https://api.github.com/repos/longbridge/gpui-kit/contents/examples>） |

`examples/` 全量：`ai_recipes` `app_assets` `brush` `color_mix_oklab.rs` `dialog_overlay` `dock` `editor` `focus_trap` `fps_monitor` `hello_world` `html` `input` `js_dock` `js_story` `js_todolist` `js_webview` `large-text` `markdown` `markdown_table` `sidebar` `speech` `stream-markdown` `syntect` `system_monitor` `table_in_scrollable` `text_max_lines` `text_selection` `tig` `tooltip_top_edge` `touch_selection` `webview` `window_title`

> 与红果最相关的示例：`sidebar`（侧栏骨架）、`dock`（常驻面板）、`dialog_overlay`（弹窗与浮层关系）、`focus_trap`（焦点管理）、`fps_monitor`（**帧率监控——红果的 `make gpui-preview` 验收可对标**）、`table_in_scrollable`（表格嵌套滚动）、`system_monitor`（密集数据面板）、`markdown` / `stream-markdown`（流式文本——对应弹幕/简介流式渲染）。

**脚手架结论**：gpui-kit **没有**「新建项目」脚手架命令；提供的是**组件画廊 + 示例目录**作为参考实现。gpui 官方另有 GPUI 官方示例：<https://gpui.rs/>（文档入口 `crates/gpui/README.md`、`crates/gpui/docs/contexts.md`、`crates/gpui/docs/key_dispatch.md`）。

### 2.6 版本与活跃度【官方事实】

来源：crates.io API <https://crates.io/api/v1/crates/gpui-kit>

| 项 | 值 |
| --- | --- |
| 最新版本 | **0.8.0**，发布于 **2026-10-19** |
| 版本总数 | 12（0.1.0、0.6.0、0.6.1、0.6.2、0.6.4、**0.6.5（已 yank）**、0.6.6、0.7.0、0.7.1、0.7.2、0.7.3、0.8.0） |
| 累计下载 | **162,321** |
| 0.8.0 下载量 | 8,136 |
| 许可 | Apache-2.0；Rust edition 2024 |
| 发布者 | huacnlee（Jason Lee） |
| 仓库 | 18,567★ / 1,139 fork / 119 open issues / 最后 push 2026-10-24 |
| 默认 features | `component` + `assets` |
| 可选 features | `gpui-fast`（0.8.0 新增，配 gpui-fast 0.1.10）、`inspector` `profiler` `speech` `syntect` `decimal` `test-support` `tree-sitter` 及各语言 feature |

> **gpui-fast**：0.8.0 起可选的替代渲染引擎（`gpui-pre-*` 仍是默认）。Zedis 实测：滚动密集页 CPU 约省 20%，内存约多 20%【社区经验】。

### 2.7 `gpui-component` 与 `gpui-kit` 的关系（避免引错仓库）【官方事实 + 推断】

- 历史上存在独立仓库 `longbridge/gpui-component`，结构为 `crates/ui` / `crates/base` / `crates/macros` / `crates/story`；现已合并进单仓 `longbridge/gpui-kit`，结构变为 `crates/base` / `crates/component` / `crates/component-macros` / `crates/story`。
- 旧文档站或第三方文章若引用 `gpui-component::Button` / `gpui_component` 顶层路径，需注意对应现为 `gpui_component` crate（`crates/component`）与 `gpui_base`（`crates/base`）。
- **本报告全篇统一使用 `longbridge/gpui-kit` 单仓口径。**

---

## 三、Zed 上游范本 + 代表性 GPUI 应用

### 3.1 Zed 桌面交互通用做法与源码路径【官方事实】

> **重要提示**：Zed 已重构，网络上大量文章引用的旧路径已失效。
> **已失效（返回 404，勿引用）**：`crates/workspace/src/command_palette.rs`、`crates/workspace/src/panel.rs`、`crates/workspace/src/tabs.rs`、`crates/workspace/src/toast.rs`、`crates/workspace/src/settings_page.rs`、`crates/command-palette/src`、`crates/workspace/src/workspace/`、`crates/gpui/src/app/notification_center.rs`。
> **正确路径**：`crates/command_palette/`（**下划线**）、`crates/settings_ui/`、`crates/tab_switcher/`、`crates/ui/src/components/`。

| 交互范式 | 确切源码路径 | 说明 |
| --- | --- | --- |
| **命令面板** | `crates/command_palette/src/command_palette.rs`（64 KB）、`command_palette_settings.rs`、`persistence.rs`；另有 `crates/command_palette_hooks/` | 独立 crate，不是 workspace 里的文件 |
| **快捷键面板** | `crates/ui/src/components/keybinding.rs`（30 KB）、`keybinding_hint.rs` | |
| **Tab 切换器** | `crates/tab_switcher/src/tab_switcher.rs`（30 KB）+ `tab_switcher_tests.rs`（23 KB） | 有专门测试文件，可直接照抄交互契约 |
| **多标签页 / 分屏** | `crates/workspace/src/pane.rs`（370 KB）、`pane_group.rs`；`crates/ui/src/components/tab.rs`、`tab_bar.rs`、`stack.rs` | |
| **右键菜单** | `crates/ui/src/components/context_menu.rs`（**102 KB**）、`dropdown_menu.rs`、`right_click_menu.rs`、`popover_menu.rs` | ContextMenu 是独立大件，值得单独研究 |
| **Toast / 通知** | `crates/workspace/src/toast_layer.rs`、`crates/ui/src/components/notification/`、`crates/workspace/src/notifications.rs`（77 KB） | |
| **状态栏** | `crates/workspace/src/status_bar.rs`；gpui-component 也有 `status_bar` 模块 | |
| **焦点管理** | `crates/gpui/src/tab_stop.rs`、`crates/gpui/src/focus.rs`（未见，**未查到确切路径**）；gpui-kit 侧为 `crates/base/src/focus_trap.rs` | |
| **模态层** | `crates/workspace/src/modal_layer.rs`（12 KB）、`security_modal.rs`（28 KB）、`crates/ui/src/components/modal.rs` | |
| **空态 / 欢迎页** | `crates/workspace/src/welcome.rs`、`invalid_item_view.rs`、`crates/ui/src/components/project_empty_state.rs` | |
| **设置页** | `crates/settings_ui/src/settings_ui.rs`（**299 KB**）、`pages.rs`、`pages/`、`page_data.rs`（**528 KB**）；数据层 `crates/settings/src/settings_store.rs`（117 KB）、`settings_file.rs`、`keymap_file.rs`（100 KB）、`content_into_gpui.rs`、`editable_setting_control.rs`、`vscode_import.rs`（67 KB） | 设置项以声明式数据驱动 UI（`page_data.rs`），是设置页组织的范式 |
| **工具栏 / 搜索** | `crates/workspace/src/toolbar.rs`、`searchable.rs`、`crates/ui/src/components/navigable.rs` | `navigable` 是键盘导航列表的基础件 |
| **Dock** | `crates/workspace/src/dock.rs`（64 KB）；gpui-base `crates/base/src/dock/` | |
| **虚拟列表** | `crates/gpui/src/elements/list.rs`（变高 `ListState`）、`elements/uniform_list.rs`（定高 `UniformList`） | |
| **图片 / 图片缓存** | `crates/gpui/src/elements/img.rs`、`elements/image_cache.rs`、`elements/svg.rs` | 封面图缓存的关键原语 |
| **按键分发 / 动作** | `crates/gpui/src/key_dispatch.rs`、`keymap.rs`、`action.rs`；`crates/gpui/docs/key_dispatch.md` | |
| **上下文 / 实体** | `crates/gpui/src/app/context.rs`、`app/async_context.rs`、`app/entity_map.rs`、`app/headless_app_context.rs`、`app/test_context.rs`、`crates/gpui/docs/contexts.md` | |
| **可访问性** | `crates/gpui/src/window/a11y.rs` | |
| **几何 / 布局** | `crates/gpui/src/geometry.rs`（`crates/gpui/src/` 下有 `geometry.rs`）、`crates/gpui/src/taffy.rs`（Taffy 布局引擎接入） | Taffy = GPUI 可选 CSS Flex/Grid 布局引擎 |
| **测试基座** | `crates/gpui/src/app/test_app.rs`、`test_context.rs`、`visual_test_context.rs` | `visual_test_context` = 视觉回归测试基座 |

**整体信息架构（可借鉴）**：Zed = `workspace`（多窗口 / 多工作区）→ `dock`（左右侧栏，可停靠面板）→ `pane` / `pane_group`（分屏）→ `tab`（每个 pane 内多标签）→ `status_bar`（全局底栏）→ `modal_layer` / `toast_layer` / `notification`（全局浮层）。

> 红果 GPUI 壳可直接套用这条骨架：**左侧导航 Dock + 右侧主区 Pane + 主区内 Tab + 底部状态栏 + 全局浮层层**。

### 3.2 虚拟列表 / 网格 / 瀑布流范式【官方事实 + 社区经验】

| 场景 | 方案 | 出处 |
| --- | --- | --- |
| 变高列表（短剧条目高度不一致） | gpui-kit `v_virtual_list` / `VirtualListScrollHandle`；Zed `ListState` | `crates/base/src/virtual_list.rs`、Zed `crates/gpui/src/elements/list.rs` |
| 定高列表（下载任务行高一致） | Zed `UniformList`；gpui-kit `h_virtual_list`（横向定高） | Zed `crates/gpui/src/elements/uniform_list.rs` |
| **定高且行数极大** | gpui-kit 提供 `ListState::with_uniform_item_height` / `reset_with_uniform_height` 预置高度 | <https://github.com/longbridge/gpui-kit/issues/3513> |
| **封面等高网格** | ⚠️ **无现成组件**，需自建 | 见 2.4 |
| **非等高马赛克 / 瀑布流** | ⚠️ **无现成组件**；唯一公开实现范式是 disktree 的 **squarified treemap**（布局算法在 `crates/disktree-core`，与 UI 分离） | <https://github.com/tobi/disktree> |
| **密集等高网格（总览页）** | Orrery 的 dense grid | <https://github.com/Hankanman/Orrery> |
| **树形 + 内容分栏** | gpui-kit `tree` + `resizable`（`h_resizable`） | `crates/base/src/{tree.rs,resizable/}` |
| **表格 + 分页 + 虚拟滚动** | gpui-kit `table` + `pagination`；Zed `crates/ui/src/components/data_table.rs`（56 KB） | |

**性能基准【社区经验】**：gpui-kit PR #3085（huacnlee）把 `virtual_list` 改为「每帧成本与条目数无关」——用 `Rc<[Pixels]>` 承载 sizes/origins，可见区间用**二分查找**而非从前往后线性扫描；**1M 条目时单核 CPU 从 34.6% 降到 12.7%**。
<https://github.com/longbridge/gpui-kit/pull/3085>

> **结论【推断】**：红果「剧集列表 / 下载任务列表」应直接用 `v_virtual_list`；「短剧封面网格」必须自建，且**必须自建时就把「可见区间二分查找」写进设计**——不要用「从头遍历累加高度」的实现方式。

### 3.3 其他代表性 GPUI 应用 / 示例仓库【官方事实】

| 项目 | 定位 | 与红果的相关点 | URL |
| --- | --- | --- | --- |
| **gpui-kit 官方示例集** | 30+ 个可运行示例 | `sidebar` `dock` `dialog_overlay` `focus_trap` `fps_monitor` `table_in_scrollable` `system_monitor` `stream-markdown` | <https://api.github.com/repos/longbridge/gpui-kit/contents/examples> |
| **gpui 官方示例** | GPUI 上游 examples | 渲染管线、事件、窗口 | <https://gpui.rs/> |
| **Longbridge Lite** | JS 应用跑在 GPUI Shell 的参考实现 | 「GPUI 壳 + Web/JS 内容」混合架构——**与红果 Tauri 壳思路对照极有价值** | <https://github.com/longbridge/longbridge-lite> |
| **Longbridge Pro** | gpui-kit 的孵化场景，三平台 | 密集数据界面标准 | <https://longbridge.com/desktop> |
| **disktree** | treemap + 侧栏 + 键盘优先 | 网格/马赛克 + 密度 + 色彩编码 | <https://github.com/tobi/disktree> |
| **Zedis** | 百万级虚拟滚动 + ⌘K | 大列表性能 + 命令面板 | <https://github.com/vicanso/zedis> |
| **FluxDown** | 下载管理器 | 下载任务 / 队列 / 进度 | <https://github.com/zerx-lab/FluxDown> |
| **tbis** | GPUI + 内嵌 mpv 播放视频 | 播放壳 | <https://github.com/mahi160/tbis> |
| **Ghostex** | Rust/GPUI 壳 + 内嵌 Chromium | **GPUI 壳内嵌 WebView**——与红果双壳架构最直接可比 | <https://github.com/maddada/Ghostex> |
| **Navop** | 2k★ 全能开发者工作区 | GPUI 大型应用的组织方式 | <https://github.com/feigeCode/navop> |
| **ShellRS** | Xshell 式多标签终端 + WinSCP 双栏 SFTP | 多标签 + 双栏 + 侧栏 | <https://github.com/since2006/shell-rs> |
| **BuMoSm PDF Viewer** | 平滑滚动 + 标签 + 搜索 + 书签 + 页面编辑 | 阅读器交互 | <https://github.com/BuMoSm-Series/BuMoSmPDFViewer> |

---

## 四、GPUI 常见体验坑（14 条，每条附链接）

> 分级：【官方事实】= gpui-kit / Zed 官方 issue、PR、release notes；【社区经验】= 第三方项目实测与讨论。

| # | 坑 | 表现 | 解法 / 依据 | 链接 |
| ---: | --- | --- | --- | --- |
| 1 | **虚拟列表每帧成本随条目数线性增长** | 1M 条目时单核 CPU 34.6%，滚动卡顿 | 用 `Rc<[Pixels]>` 承载 sizes/origins，可见区间**二分查找**；改后降到 12.7%【官方事实】 | <https://github.com/longbridge/gpui-kit/pull/3085> |
| 2 | **滚动条拖拽范围漏算 list padding** | 永远拖不到底部；jump-to-latest 按钮不消失；长历史未 seed 行高时 thumb 起始过大 | 用 `ListState::with_uniform_item_height` / `reset_with_uniform_height` 预置高度；注意 `remeasure` 保留过期 size_hint 会造成「幽灵滚动范围」【官方事实】 | <https://github.com/longbridge/gpui-kit/issues/3513> |
| 3 | **嵌套滚动：父子同时滚** | 内层列表滚到头后外层跟着滚 | 只有指针下**最内层**滚动容器响应；新增 `scroll_wheel_taken` / `take_scroll_wheel` / `div.scroll_max`【官方事实】 | <https://github.com/zed-industries/zed/pull/64057> · <https://github.com/zed-industries/zed/pull/64594> |
| 4 | **带 padding 的可滚动 TextView 拖拽自动滚动不动** | 合成 wheel 事件落在 padding 命中区之外 | 自滚参与者必须走自己的 list API，合成 wheel 只留给祖先滚动【官方事实】 | <https://github.com/longbridge/gpui-kit/pull/2948> |
| 5 | **`set_searchable` 每次都 notify → 每帧重建整页** | 在 gpui-fast 下每帧全量重建 | 仅在值**变化时** notify（与 `set_line_number` 同策略）【官方事实】 | <https://github.com/longbridge/gpui-kit/pull/3576> |
| 6 | **虚拟列表内 `TextViewState::reveal_range` 触发 a11y 节点 id 重复而崩溃** | `Duplicate a11y node id` | prepaint 重试时 a11y builder 未随 `Window::transact` 回滚而回滚【官方事实】 | <https://github.com/longbridge/gpui-kit/issues/3295> |
| 7 | **Table 行事件不带 `Modifiers`** | Cmd/Shift 多选做不到，需 fork 表格 + 虚拟列表 | 社区方案：维护本地 fork（Ferail 项目）【社区经验】 | <https://github.com/longbridge/gpui-kit/issues/2795> |
| 8 | **DataTable 对屏幕阅读器只暴露无名 DataItem** | 无 Table/Grid 容器、无 ColumnHeader / Cell、键盘焦点缺失 | 补齐 a11y 结构（容器角色 + 表头 + 单元格 + 键盘焦点）【官方事实】 | <https://github.com/longbridge/gpui-kit/issues/3647> · <https://github.com/longbridge/gpui-kit/pull/3667> |
| 9 | **键盘选中高亮与鼠标 hover 高亮同时出现** | picker / command palette 里两条高亮叠在一起 | 用 `keyboard_navigated` 标志 + `block_mouse_except_scroll` 覆盖层，鼠标移动后恢复 hover【官方事实】 | <https://github.com/zed-industries/zed/pull/50827> |
| 10 | **圆角与 overflow 裁剪冲突** | 内层输入框 / 选中行被父级圆角裁掉 | 改为独立 padding-edge clip scope；command palette 只外层圆角、内层保持直角【官方事实】 | <https://github.com/zed-industries/zed/pull/65753> |
| 11 | **原生子视图 / WebView 与 GPUI overlay 的层级关系未定义** | 播放层被菜单/对话框盖住，或浮层画不到原生表面上 | 三平面 composition 模型（**base / native surface / overlay**，`CompositionTree`），示例 `cargo run -p gpui --example native_webview`；**明确说明同样适用于 native video / audio / PDF surface**【官方事实】 | <https://github.com/zed-industries/zed/pull/61945> · <https://github.com/zed-industries/zed/pull/62379> |
| 12 | **滚轮步进与系统不匹配（滚太快）** | Windows `Lines to scroll`、X11/Wayland `SCROLL_LINES`=3 导致滚太快 | 新增 `ScrollWheelEvent.wheel_notches` 区分 notch 与行数【官方事实】 | <https://github.com/zed-industries/zed/pull/65256> |
| 13 | **GPUI overdraw 5–6 倍** | GPU 占用与功耗显著高于预期（RenderDoc 实测） | 长期 open issue；对策是减少半透明叠层、避免大面积 blur / shadow【官方事实（issue 长期未闭）】 | <https://github.com/zed-industries/zed/issues/8043> |
| 14 | **Zoom 未真正替换底层内容** | 视图放大后 GPU 使用率显著上升 | 长期 open issue【官方事实】 | <https://github.com/zed-industries/zed/issues/52075> |

**补充：滚动手感基线（gpui-kit v0.8.0 release notes，官方事实）** — <https://raw.githubusercontent.com/longbridge/gpui-kit/main/release-notes.md>

- ScrollBounce 默认在 macOS 启用（NSScrollView 手感：tracking 0.075、response ≈503 ms、stretch ≤36 px）；
- 普通行滚轮**不弹跳**，嵌套区域**不再一起滚**；
- `ScrollableMask::vertical_wheel` 横向条带；cubic-bezier 保留 overshoot；
- Table Home/End 行选择；TabBar overflow 菜单调用 tab 自身 on_click、滑动指示器延后一帧；
- focus trap 保持焦点与 a11y；Popover 内容内键盘激活；Tree **点击而非按下**才选中/切换；TextView 复制在重绘前取当前手势。

**未解决/仍为长期请求【官方事实】**：平滑滚动（smooth scrolling）长期 feature 请求，1018 reactions —— <https://github.com/zed-industries/zed/issues/4355>
Wayland 帧迟到导致滚动卡顿/跳帧 —— <https://github.com/zed-industries/zed/pull/65013>

---

## 五、对红果 GPUI 壳的可落地借鉴点（三档分类）

> 三档定义：
> **A 直接可用** = gpui-kit 已有组件/原语，接上即用，不需改上游；
> **B 需要改造** = 有现成实现但需改造（换数据源、加业务字段、换视觉、改交互契约）；
> **C 需要新建** = gpui-kit 与 Zed 均无现成实现，须自建。

### A. 直接可用（gpui-kit 现成件，装上即用）

| # | 借鉴点 | 对应 gpui-kit 组件 / 原语 | 来源 |
| --- | --- | --- | --- |
| A1 | **主界面骨架**：左导航 + 右主区 + 主区内 Tab + 底部状态栏 | `sidebar` `tab` `dock` `status_bar` `resizable`（`h_resizable`/`v_resizable`） | 组件清单（2.2）<br>Zed 架构（3.1） |
| A2 | **命令面板（⌘K）** | `gpui_component::command`（`pub mod command;` 且 `init(cx)` 注册） | `crates/component/src/lib.rs` |
| A3 | **设置页** | `setting` `switch` `select` `checkbox` `radio` `slider` `stepper` `input` `combobox` `rating` `form` `group_box` `description_list` | 组件清单（2.2）<br>Zed `crates/settings_ui/`（3.1） |
| A4 | **剧集 / 下载任务长列表** | `v_virtual_list` / `VirtualListScrollHandle` / `searchable_list`（带搜索过滤） | `crates/base/src/virtual_list.rs`<br>Zed `elements/list.rs` |
| A5 | **等高数据表（下载明细 / 存储统计）** | `table` + `pagination` | 组件清单（2.2） |
| A6 | **弹窗体系**：确认框 / 侧滑面板 / 气泡 / 右键菜单 / 提示 | `dialog` `sheet` `popover` `hover_card` `menu` `native_menu` `tooltip` `alert` `message` `notification` | 组件清单（2.2） |
| A7 | **焦点管理（弹窗打开时焦点锁定、Esc 关闭）** | `gpui_base::focus_trap`（`FocusTrapElement`） | `crates/base/src/focus_trap.rs`<br>示例 `examples/focus_trap` |
| A8 | **空态 / 加载态 / 骨架屏** | `empty` `skeleton` `shimmer` `spinner` | 组件清单（2.2） |
| A9 | **进度与状态**：下载进度、标签、徽章 | `progress` `badge` `tag` `status_bar` | 组件清单（2.2） |
| A10 | **主题与令牌**：统一语义色 / radius / spacing / 字号，含 17 套内置主题 | `theme` 模块 + `SemanticThemeTokens` + `ThemeColor` | 2.3（`theme_tokens.rs`、`theme_color.rs`、`themes/`） |
| A11 | **i18n 落地** | 组件库内置 `rust_i18n!("locales")` + `locale()` / `set_locale()` | `crates/component/src/lib.rs` 末尾 |
| A12 | **滚动手感与键盘导航基线** | ScrollBounce（macOS）、Tab Home/End、focus trap、Tree 点击才切换 | v0.8.0 release notes |
| A13 | **帧率监控工具** | `examples/fps_monitor`；gpui-kit 自带 `crates/fps` | `crates/` 目录 |
| A14 | **图标** | `gpui_component::IconName` + `crates/assets`（`assets` feature 默认开启） | `crates/component/src/lib.rs` |

### B. 需要改造（现成实现需按红果业务改造）

| # | 借鉴点 | 改造点 | 来源 |
| --- | --- | --- | --- |
| B1 | **短剧封面网格**（等高网格） | gpui-kit **无 grid 原语**：需用 flex 行 + 固定卡片宽 + 自算列数实现；若列数需随窗口宽度变化，要自己写测量与重排 | 2.4（`lib.rs` 无 `grid` 模块）<br>Zed `crates/gpui/src/taffy.rs`（若要用 Grid 布局引擎需自行接 Taffy） |
| B2 | **封面/海报图片加载与缓存** | gpui-kit 侧需接 Zed 的图片缓存思路（`elements/img.rs` + `elements/image_cache.rs`）；红果需接自己的 `hongguo-protocol` 封面 URI（`hongguo-local/cover`） | Zed 3.1 |
| B3 | **播放器层与 GPUI 浮层共存** | 按 Zed 三平面 composition 模型（base / native surface / overlay）重新组织红果播放层与菜单/对话框的层级；红果用系统播放器（AVPlayer / MediaEngine）而非 WebView，**该模型明确覆盖 native video surface** | Zed PR #61945 / #62379（第四节第 11 条） |
| B4 | **弹幕 / 进度的高频刷新** | 需按 gpui 帧预算控制更新频率（示例 `examples/stream-markdown` 可参照），避免每条弹幕触发一次重绘 | `examples/stream-markdown` |
| B5 | **命令面板条目** | 组件现成，但条目要接红果的业务动作（打开剧集、下载、跳转设置），并与现有 IPC/service 边界对齐 | A2 + 项目内 `hongguo-service` 分层 |
| B6 | **下载任务列表** | 表格/虚拟列表现成，但需接 `ServiceEvent` 进度事件；**事件必须只发「值变化」**（对应坑 #5：每帧 notify 会重建整页） | 坑 #5 <https://github.com/longbridge/gpui-kit/pull/3576> |
| B7 | **主题令牌对齐 Tauri 壳** | gpui-kit 有两套令牌（语义层 + legacy 组件层），需选一层为主并映射红果现有 i18n/主题资源；禁硬编码色值 | 2.3 |
| B8 | **快捷键体系** | gpui-kit 提供 kbd / keybinding 展示件，但**按键绑定表与动作注册**需按红果 GPUI 壳的动作表实现（gpui 侧 `action.rs` / `keymap.rs`） | Zed `crates/gpui/src/{action.rs,keymap.rs}` |
| B9 | **密度档位** | 按 design-guides 的密度档位与 rem 单一标度定三档（舒适/标准/紧凑），并支持界面缩放 | 2.3 design-guides.md<br>disktree README（rem 标度） |
| B10 | **设置项声明化** | 参照 Zed `crates/settings_ui/src/page_data.rs`（声明式数据驱动 UI），把红果设置项也写成声明式，再由 `setting` 组件渲染 | Zed 3.1 |

### C. 需要新建（gpui-kit 与 Zed 均无现成实现）

| # | 借鉴点 | 为什么必须自建 | 参考范式 | 来源 |
| --- | --- | --- | --- | --- |
| C1 | **瀑布流 / 不等高封面网格** | gpui-kit 无 grid / masonry / 瀑布流模块（**确认不存在**，非「没找到」） | disktree 的 **squarified treemap**（布局算法与 UI 分离于 `crates/disktree-core`） | 2.4 + <https://github.com/tobi/disktree> |
| C2 | **短剧播放器控制条** | gpui-kit 无媒体控制条组件；且红果播放内核是系统播放器（AVPlayer / MediaEngine），控制条需自绘并对接 `PlayerEvent` | Pawse（时间同步歌词）、Cadence、Meow Player（两栏媒体布局） | 1.4⑥ |
| C3 | **进度条 / 弹幕轨 / 选集的时间轴组件** | 无现成时间轴组件（`progress` 是通用进度条，不含刻度与轨道） | Pawse 时间同步歌词 | 1.4⑥ |
| C4 | **剧集横向选集条（可滑动封面小卡）** | `h_virtual_list` 只解决「定高横向虚拟滚动」，**卡片渲染、选中态、圆角裁剪、滚动吸附**需自建 | Zeidis 的 key tree 横向区 + disktree 的「放大到指针所在目录」交互 | `crates/base/src/virtual_list.rs` + disktree README |
| C5 | **命令面板的搜索/模糊匹配与分组** | `command` 组件提供外壳，**模糊匹配算法与结果分组**需自建（可参考 Nohrs「fuzzy search over a SQLite app index」、Setu「layered variables + workspaces」） | <https://github.com/noh-rs/nohrs> · <https://github.com/bajrangCoder/setu> | 1.3 |
| C6 | **红果专用主题（播放器红色皮肤）** | gpui-kit 内置 17 套主题中无红果皮肤；需按 `.theme-schema.json` 规范新增一套 | `themes/` + `.theme-schema.json` | 2.3 |
| C7 | **「删除前复核列表页」范式** | gpui-kit 只有 `dialog` 确认框，没有「多选复核 → 汇总 → 提交」的多步流程页 | disktree 的 review 屏幕（标记 → 复核 → 提交/导出） | disktree README |
| C8 | **封面状态编码（色相=分类，纹理=状态）** | 属自定义视觉语言，需在红果设计规范中定义并落到 `ThemeColor` 扩展 | disktree 色彩纪律（muted 色阶 + 斜纹 + 单一琥珀色保留给主操作） | disktree README |
| C9 | **全局快捷键（应用级 hotkey）** | gpui-kit 组件层无应用级全局热键设施（gpui-kit 依赖的 gpui 侧有 `platform`/全局动作机制，但需按红果窗口模型接入） | Aloud Ink（按住全局快捷键说话）、Steward（全局热键 + 模糊搜索） | 1.3 |

---

## 六、组件归位决策表（第二轮落地的实测结论）

> 口径：gpui-kit / gpui-component **0.7.3**（crates.io，未 fork），gpui-pre **0.3.9**，
> gpui-base **0.7.3**，accesskit **0.24.1**。以下每条都对着本地 registry 源码核过，
> 不是文档推测。**「归位」= 把壳里手搓的 div/自绘件换成组件库现成件。**

### 6.1 结论总表

| 组件 | 是否归位 | 关键依据（源码坐标） |
| --- | --- | --- |
| `component::dialog::AlertDialog` | ✅ 已在用 | 壳内 5 个列表页 + tasks/storage/merge/settings 全部走 `WindowExt::open_alert_dialog`；**不需要再改** |
| `component::skeleton::Skeleton` | ✅ 新增使用 | `bg(theme.skeleton)` + 2s `opacity 1→0.5` 脉冲，与 Tauri `bg-accent animate-pulse` 同构；12 页首拉加载已接 |
| `component::separator::Separator` | ✅ 新增使用 | `horizontal()/vertical()/dashed()`，默认取 `theme.border` |
| `component::tag::Tag` | ✅ 新增使用 | `small()` + `rounded_full()` = 药丸徽标，与 Tauri `ml-auto rounded-full px-1.5 py-0.5 text-[10px]` 同参 |
| `component::badge::Badge` | ✅ 新增使用（带限制） | **未实现 `Styled`**（`component/badge.rs` 只有 `ParentElement`/`Sizable`/`RenderOnce`），不能链式加样式，得用外层 div 承担布局 |
| `base::Button`（基础层） | ✅ 新增使用 | 唯一能同时满足「键盘可达」与「视觉逐像素不变」的件：自带焦点句柄 / tab_stop / Enter·Space 激活 / role·aria，**样式完全交给调用方** |
| `component::button::Button`（组件层） | ⚠️ 仅特定场景 | 组件层强制内容居中且字号随 size 变，做不了「左对齐小字胶囊」；`pill_tab.rs` 已用它做顶栏胶囊 |
| `component::table::Table` | ✅ 新增使用 | 语义表：`role=table/row/cell/columnheader` + `aria_row_index/column_index`（gpui-base `table.rs`） |
| `component::tab::TabBar` / `base::Tabs` | ❌ **不采用** | `gpui-base/src/tabs.rs:15-21` 源码注释明写 *「Tabs do not participate in keyboard focus by themselves」*，`Tabs` 根只给 `role=TabList` 收集语义，**没有箭头键漫游焦点**。用它做筛选行＝键盘用户仍然够不到 |
| `component::setting::SettingPage` | ❌ **不采用** | `SettingItem::render_item` 是 `pub(super)` → `SettingItem` **只能**经 `SettingPage`/`SettingGroup` 渲染；而 `SettingPage` 自带页头（标题 + 搜索 + reset-all）+ 全局 `SettingsState`，与本壳「草稿 + 保存按钮」的语义冲突（见 `pages/settings/`） |
| `component::toggle::Toggle` | ❌ 不适合本形态 | 渲染成 `min_w_8/h_8` 的方形控件（`button/toggle.rs`），做不了 Tauri `ToggleGroupItem` 那种小字药丸 |
| `component::rating` | ❌ 不采用 | 无半星，替换 `score_stars` 会丢规格 |

### 6.2 换载体时必须回钉的库强制样式（踩坑记录）

`base::Button` 渲染时强制 `.flex().items_center().justify_center().line_height(relative(1.))`
（`gpui-base/src/button.rs:214-228`）。直接换载体会让左对齐变居中、行盒矮一截：

- 需要左对齐 → 追加 `.justify_start()`；
- 需要原行高 → 追加 `.line_height(gpui_kit::phi())`（`phi()` = `relative(1.618034)`，
  与 `TextStyle::default().line_height` 同值，钉回后与原 div 逐像素一致）。

### 6.3 焦点环：gpui 没有默认实现

gpui 0.3.9 **不提供默认焦点环**，只有可选的 `focus_visible` 样式细化；gpui-kit 的正统
`ThemeStyled::focus_ring_style(window, cx)` 需要 `&Window` 才能算圆角，而页面级渲染链
拿不到 `Window`。本壳按 Tauri/shadcn 同款规格自建：边框染 `theme.ring` + 3px 外圈阴影
（`ring-ring/50`），静止态零像素变化。实现收口在 `pages/list_common.rs::focus_ring`，
避免每处各写一份漂移。

### 6.4 accesskit 的 Role 变体名

`Role::Radio` **不存在**（accesskit 0.24 的枚举是 `RadioButton` / `RadioGroup` /
`Tab` / `Link` / `Button`）。单选组（筛选行、子导航）的正确写法是：
容器 `role(Role::RadioGroup)` + 子项 `role(Role::RadioButton)` + `aria_toggled(Toggled::True/False)`。

---

## 七、键盘可达改造的实测结论（第三轮）

> 口径同第六节。**「归位」在本轮 = 把鼠标可点、键盘够不到的裸 `div` 换成
> `gpui_kit::base::Button`**，使 Tab 能走到、Enter/Space 能激活、并带上 role 与
> 无障碍名称。Tauri 壳是语义基准（其列表卡本来就是 `role="button" tabIndex={0}`）。

### 7.1 像素还原的前提：`div()` 默认是 block，`base::Button` 是 flex row

gpui 的 `Style::default().display` 是 `Display::Block`（`gpui-pre/src/style.rs`），
所以壳里裸 `div()` 的子元素是**竖排堆叠**；而 `base::Button` 渲染时强制
`.flex()`，即 `display: flex` + `flex-direction: row`。换载体时若不显式回钉，
竖排卡会变成横排、标题不再按卡宽折行。**完整还原配方**：

| 原 div 的形态 | `base::Button` 上必须追加 | 原因 |
| --- | --- | --- |
| 竖排堆叠的卡（`v_flex` / block 容器） | `.flex_col()` | 库固定 row，卡内封面/标题/统计要继续竖排 |
| 子件需要撑满卡宽（标题按卡宽折行） | `.items_stretch()` | 库固定 `items_center`，子件会收缩到内容宽、标题不折行 |
| 内容从顶上排（多卡等高对齐） | `.justify_start()` | 库固定 `justify_center`；Tauri 侧 `related-card.tsx:33-37` 的注释点名过「button 会把内容垂直居中，整排一高一低」 |
| 有文字 | `.line_height(gpui_kit::phi())` | 库固定 `relative(1.)`，行盒比默认矮一截；`phi()` = `relative(1.618034)`，与 `TextStyle::default().line_height` 同值 |
| 固定尺寸盒（封面 92×123） | 保留原 `.w()/.h()/.flex_shrink_0()` | flex 容器会改写子件主轴尺寸 |

`base::Button` **不注入任何背景色 / 边框 / 内边距**（源码 `gpui-base-0.7.3/src/button.rs:214-255`
只注入 flex / items_center / justify_center / line_height 与 role、aria、焦点句柄），
所以视觉完全由调用方的 styled 链决定——这是「换载体但像素不变」的根本原因。

### 7.2 语义角色与 aria 对照（逐条对着 Tauri 侧真实实现）

| 元素 | Tauri 侧写法 | GPUI 侧归位 |
| --- | --- | --- |
| 列表卡 / 相关卡 | `role="button" tabIndex={0}` + `onKeyDown` 收 Enter/Space（`related-card.tsx:38-48`） | `base::Button`（默认 `Role::Button`），**不要**改成 `Role::Link`——那是外链语义 |
| 应用内导航（侧栏行、出品方芯片） | — | `.role(Role::Link)`，与壳内既有侧栏口径一致 |
| 单选组（评分星、筛选行、日期格） | `role="radiogroup"` + 子项 `aria-label`（`review-composer.tsx:45-67`） | 容器 `role(Role::RadioGroup)` + 子项 `role(Role::RadioButton)` + `aria_toggled(Toggled::True/False)` |
| 折叠展开 | `aria-expanded` | `.aria_expanded(bool)`（gpui-pre `elements/div.rs` 提供 `aria_expanded`） |
| 点赞类开关 | `aria-pressed` 语义 | `.aria_toggled(Toggled::True/False)` |
| 图标键 | `aria-label` | `.accessibility_label(...)` |

**「视觉高亮 ≠ 语义状态」**：续播集描边、当天日期高亮是「上次看到的位置」，
不是「当前选中」，加 `aria_toggled`/`aria_selected` 属于谎报，不加。

### 7.3 焦点环归位到令牌层

本轮把 `focus_ring` 从 `pages/list_common.rs` 移到 `tokens.rs::focus_ring`：
焦点环是**全壳主题派生值**（不是某个页面的私有件），归在 `pages/` 下会让
`detail/` 反向依赖 `pages::`，目录归属不成立。`tokens.rs` 已是「产品令牌层」，
放这里与 `hot()`/`star()`/`scrim()` 同层。

### 7.4 键盘可达源码闸门

`crates/hongguo-gpui/tests/keyboard_reachability.rs`（集成测试，只读源码文本）：
扫**整个 `src/`**，把「`div()`/`h_flex()`/`v_flex()` 起头、链上带 `.on_click(`」
判为违规。**判据按缩进分层**：链头比方法续行浅一层、嵌在参数里的子链比方法
续行深一层，所以链里 `.child(...)` 与 `.on_click(...)` 的先后顺序不影响判定。

第三轮的第一版检测器有两个**漏报**（实测发现，已修）：

| 漏报形态 | 原因 | 修法 |
| --- | --- | --- |
| `.bg(if active { … })` 这类多行参数的**收尾行**（`})`）截断了向上回溯 | 收尾行与方法续行同缩进，被当成「链结束」 | 收尾行跳过而非截断；只有真空行才截断链 |
| **注释行**截断了向上回溯 | 注释被剥成空行，与真空行同判 | 纯注释行跳过；真空行才截断链 |

修完后全仓违规数从 17 涨到 **36**——也就是说旧检测器漏了 19 处真问题（含
选集浮层的格子与分段 tab、互动栏单键、历史行、下载面板的集号格与浮层根、
评论行的 ♡ 点赞、列表页的 6 处卡片）。

第四轮把豁免范围收到**零**：播放器红色皮肤区不再整体豁免。全仓 36 处
最终分成三类——32 处换成 `base::Button`、3 处是「只吞点击」的屏蔽层（删掉多余
的 `on_click(stop)`，见 7.7）、1 处是显式标注的指针便利层（见 7.6）。
检测器另配 14 个单元测试钉住行为。

### 7.5 「让位」：舞台快捷键与子控件的空格双触发

gpui 的按钮把 Enter/Space 转成 click 的时机在 **KeyUp** 阶段，且条件是
`phase.bubble() && !window.default_prevented()`（`gpui-pre/src/elements/div.rs`
的 `pending_keyboard_down` 机制）。于是：

- 焦点落在控制栏按钮上时，按空格 → **按钮激活**（KeyUp 合成 click）**+ 舞台
  快捷键翻暂停**（KeyDown 处理）= 两次翻转 = 什么都没发生；
- 阶段三之前控制栏已经是 `component::button::Button`（内含 `base::Button`），
  点过「播放/暂停」钮之后焦点就落在它身上——这个「按空格没反应」是**既有缺陷**，
  不是新增的。

修法（`home/stage/render.rs`）：舞台的 `on_key_down` 在处理播放/暂停键前先看
焦点在谁身上——`Window::focused(&App) -> Option<FocusHandle>` 与舞台自己的
`FocusHandle` 比对（`FocusHandle` 实现了 `PartialEq`，按 id 比）。焦点在**子控件**
上就让位给那个控件，焦点在舞台自身（点画面时会把焦点带回舞台）才由舞台翻暂停。
方向键 / F / Esc 不让位（子控件不消费它们）。

### 7.6 指针便利层：唯一的一处显式豁免

「点画面暂停」那一层（`home/stage/render.rs` 的 `stage-video`）**不是**独立的
键盘入口：同一动作的键盘入口是控制栏的播放/暂停钮与舞台的空格快捷键。把它做成
按钮会在无障碍树里多出一个**覆盖整块画面的巨按钮**（Tauri 侧也没有这个按钮）。

所以它是**指针便利层**，在源码里显式标注：

```rust
// keyboard-skip: 点画面暂停是**鼠标便利层**，不是独立的键盘入口——…
div()
```

闸门认这个标记，但**理由不能为空**（`keyboard-skip:` 后面必须写理由）。豁免写在
人眼前的代码里，不是藏在测试里的名单。

### 7.7 点击屏蔽层：靠 `on_mouse_down(stop)` 就够，`on_click(stop)` 是多余的

gpui 的 click 是「同一元素上按下 + 抬起」合成的：元素在自己的 mouse-down
**冒泡**阶段记 `pending_mouse_down`，mouse-up 时才合成 click。所以**只要在
mouse-down 阶段拦住冒泡，点击就根本合成不出来**——仓库既有纪律已经这么用：

- 舞台的选集捕获层、小屏拖拽条：只有 `on_mouse_down(stop)`；
- 控制条根节点：`on_mouse_down(stop)` + `on_mouse_up(stop)`。

据此删掉了三处多余的 `on_click(stop)`：评论面板根、简介块行、下载浮层根。
**注意区分**：列表页选择圆钮 `pick_dot` 的 `stop_propagation` **必须保留**——圆钮是
卡片按钮的**子节点**，卡片在管理态的点击也是「切换选中」，不拦就是同一次点击
切换两次（点圆钮没反应）。

### 7.8 组件库 Button 的光标策略：调用方样式最后落盘，构造期补一条就够

gpui-component 0.7.3 的 `Button` 在根样式上固定 `cursor_default()`，只给 link/text
变体补 `cursor_pointer`（`component/button/button.rs` 的 root 构造链）——于是
ghost/primary/outline/custom/danger 这些最常点的变体，鼠标停在上面**看不出能不能点**。

组件库把**调用方样式**克隆成 `instance_style`，在组件链之后用 `refine_style` 最后
落盘；`refine_style` 对每个字段是「有值就覆盖」的替换语义，所以**调用方设的光标
盖得住组件库内部固定的 `cursor_default`**。据此在壳里建收口模块
`crates/hongguo-gpui/src/ui_button.rs`（提供 `button::new(id)`，返回组件库 Button
并带上 `cursor_pointer`），调用点**零改动**，只换 import：

```rust
// 之前
use gpui_kit::component::button::{Button, ButtonVariants};
// 之后
use crate::ui_button::button as Button;
use gpui_kit::component::button::ButtonVariants;
```

`Button::new(...)` 与组件库同名同义（变体、链式样式、`ButtonVariants` 全部照旧），
多的只是那条光标策略。这与 Tauri 侧的一次性全局规则
（`src/styles/index.css`：`button:not(:disabled) { cursor: pointer; }`）同构：
**一处收口，不逐个调用点加样式**。

禁用态：组件库把光标写死在根样式上，构造期无从得知禁用与否，所以由调用方在
`.disabled(cond)` 之后补 `.when(cond, |b| b.cursor_default())`——组件库的 disabled
按钮不响应点击，光标回箭头才对（与 Tauri 的 `:not(:disabled)` 同款）。

基础层 `base::Button` 全库无 `cursor` 相关代码（光标由调用点显式给），上一轮键盘
可达改造已逐个补齐 `.cursor_pointer()`（57 处），本次不动。

### 7.9 Windows 切集闪一帧：视频位的帧是**窗口级共享**资源

Windows 的画面通路是「引擎 → `TransferVideoFrame` 写进渲染器设备上的纹理 → 视频
quad 与 GPUI 内容画进同一张渲染目标」（vendor 补丁 `video_quad`）。视频 quad 按
HWND 注册，**进程级共享**：切集换的是播放器（每集一个新引擎），不是 quad。

闪一帧的两个成因（都在 `crates/hongguo-player-windows/src/player.rs`）：

1. **空纹理被当成画面发出去**：`present_frame` 每帧都 `set_frame`，但
   `TransferVideoFrame` 只在 `OnVideoStreamTick` 报新帧时才写纹理。换集后新引擎的
   首帧还没解出来，纹理是空的——发出去就是闪一帧黑。现在纹理带 `has_frame` 标记，
   没有画面就不发（落点仍每帧更新，暂停时拖窗口画面照样跟）。
2. **旧播放器 Drop 时清了共享帧**：帧属于窗口、不属于某一集。旧播放器先死、新集
   首帧未到时清帧 = 黑窗。现在帧的清理由「画面不再可见」触发（`detach()` / 舞台
   `visible=false`，切剧时 `system_video_ready` 置假走的就是这条路），换集保持上一集
   末帧顶住画面，等新集首帧接棒——这正是 Tauri `use-frame-hold` 的语义（壳里
   `system_video_ready` 的注释本来就写着这件事）。

macOS 侧不闪：AVQueuePlayer 队列接力不换播放器（`advance_preloaded` 真接力），原生层
一直显示上一集末帧；Windows 的 MediaEngine 没有播放列表，接力恒为重建
（`preload_next` / `advance_preloaded` 都返回 false）。

### 7.10 GPUI 的文本折行：只有拿到**确定宽度**才折，多元素富文本必横向溢出

gpui-pre 0.3.9 的折行判定在 `src/elements/text.rs`（`TextLayout::layout`）：

```rust
let wrap_width = if text_style.white_space == WhiteSpace::Normal {
    known_dimensions.width.or(match available_space.width {
        crate::AvailableSpace::Definite(x) => Some(x),
        _ => None,
    })
} else { None };
```

两个要点：

1. `white_space` 默认就是 `Normal`（`src/style.rs`），**不是 white_space 没设**；
2. `wrap_width` 只能来自「父给的确定宽度」——`known_dimensions.width`（元素被
   stretch / 被指定宽度）或 `AvailableSpace::Definite`。

而 flex 布局里每个 item 的 flex base size 都按 **max-content** 量（taffy 0.13
`src/compute/flexbox.rs` 的 `determine_flex_base_size`：主轴测量一律
`AvailableSpace::MaxContent`）。于是：

- 文本放在**行**容器里 → 宽度按内容算 → 不折行；
- 文本被拆成**多个 flex item**（提及、表情各自成件）→ 每个 item 都按 max-content
  测量 → 整段中文不折行、横向撑出容器；
- item 的自动最小尺寸（`min-width: auto`）在 GPUI 上等于 max-content
  （MinContent 约束下 `wrap_width` 仍是 `None`），所以 item 也不会被压缩。

**结论**：要折行，文本节点必须拿到确定宽度——最稳的做法是让文本待在**列**容器
里（列会 stretch 子件 → `known_dimensions.width` 有值），并且整段文本作为**一个**
文本节点渲染，不要拆成多个 flex item。

红果 GPUI 壳的落地（`crates/hongguo-gpui/src/comment_text.rs`）：

- 连续文本合成一段，用 `StyledText` 的高亮区间（`with_highlights`）区分颜色——
  `with_highlights` 在布局期才用 `window.text_style()` 派生 run，字号/颜色跟容器
  继承一致，不用自己拿 font；
- 提及前缀、正文提及段、表情码都进同一段文本（表情码切分文本段，GPUI 没有行内图）；
- 外层链路（评论面板）逐级改成列容器：滚动区 → 评论卡 → 正文容器 → 文本段，
  每一级给下一级 stretch 出确定宽度；
- 昵称这类「一行放不下也不该换行」的文本另走 `truncate()`（列容器 + 省略号），
  否则长昵称会顶到右侧按钮上。

断言用进程内几何（`crates/hongguo-gpui/tests/comment_text.rs`）：固定宽度列里
渲染长正文，断言文本节点高度 > 单行高度（即已折行）且宽度 ≤ 容器宽。Windows 上
gpui 的无头渲染器不可用（`current_headless_renderer` 返回 `None`），出不了 PNG，
几何断言是这轮唯一可自动化的验收手段。

### 7.11 图片圆角：父容器**不裁子件**，圆角必须落在 `img` 自己身上

**症状**：壳里的封面全是直角（Tauri 侧同一批图都是圆角），且「哪些页圆、哪些页
不圆」各写各的，同一张封面在不同页是两种圆角。

**根因（源码钉死）**：gpui-pre 0.3.9 的 `ContentMask` 是个**只有 bounds 的结构
体**（`window.rs:1283`），子件的裁切区域是**矩形**；`style.rs` 的 `overflow_mask()`
返回的也只是矩形掩码。而父 div 的 `rounded` 只影响**它自己**的背景与边框，
不裁子件。

真正能把图角裁圆的是图片元素自己的 `corner_radii`：`elements/img.rs` 的 `paint()`
把 `style.corner_radii.to_pixels(...)` 传给 `window.paint_image(...)`，渲染器据此
画圆角图片（gpui-pre 自带测试 `image_object_fit_cover_clamps_corner_radii_to_
visible_bounds` 断言的就是这个）。

**结论**：

- 圆角要写在 `img` 元素上（`img(...).rounded(r)` / `.rounded_t(r)` / `.rounded_full()`）；
  容器的 `rounded` 只管底色，两处取同一档才不会露方角；
- `rounded_t`（顶两角）用于「封面贴在卡片顶部、底角接文字区」的网格卡——Tauri
  侧是卡片 `overflow-hidden rounded-xl` 裁出来的同款效果，底角是方的；
- `CornerStylePrefix`（`gpui-macros/src/styles.rs`）生成的是 `rounded` /
  `rounded_t` / `rounded_tl` … 一族，签名 `impl Into<AbsoluteLength>`。

**取值对齐**：本项目 Tauri 侧 `src/styles/index.css` 的 `--radius: 0.625rem`
（10px），`@theme inline` 派生 `--radius-md`（8px）/ `--radius-lg`（10px）/
`--radius-xl`（14px）——**不是 Tailwind 默认的 6/8/12**。壳里按这三个值取档：
卡片 14px、行内封面 10px、小图 8px，落在 `tokens::radius`。

红果 GPUI 壳的落地：`tokens.rs` 放 `radius::{CARD, COVER, THUMB}` 与
`CoverShape`（档位 → 圆角的映射，`CoverShape::apply` 把圆角写到元素上），
`cover_img::card(url, shape)` 强制调用方选档（编译期兜住「忘了给圆角」）；
各页容器用同一批令牌取底色圆角。

断言（`crates/hongguo-gpui/tests/cover_corner.rs`，样式层，不开窗口）：逐档断言
元素样式里的四角半径——卡片档只有顶两角且为 14px、其余档四角齐、头像档取满
（整圆），外加「圆角确实落在图片元素上」与「令牌值 = Tauri 尺度」两条。

### 7.12 渲染器没有 mipmap：大图缩到小尺寸必须**进 GPU 之前**缩

**症状**：账户头像比 Tauri 侧明显糊（Tauri 走浏览器的高质量缩放）。

**根因（源码钉死）**：gpui-pre-wgpu 0.3.9 建纹理时 `mip_level_count: 1`、过滤
走 `FilterMode::Linear`（`wgpu_renderer.rs`），**没有 mipmap 链**。几百像素的
原图直接缩到 20px 画出来，每个输出像素只采到原图千分之一的信息——是采样噪声，
不是「稍微软一点」。

**结论**：缩放必须在交给 GPU 之前做完（与封面链路 `EDGE_*` 档位同一思路）：

- 按**短边**缩到显示尺寸（头像源图多为非正方形，`object_fit: Cover` 会裁切，
  短边到位才保证裁完不糊）；
- 大比例缩小用 `Lanczos3`（`image::imageops::FilterType`）——双线性在 20 倍缩小时
  只剩噪点；
- gpui 的 `rem` **不随 DPI 缩放**（`rem_size` 固定 `px(16.)`，只在 taffy 换算时
  用 `scale_factor` 做设备像素取整），所以显示边长就是物理像素：侧栏头像
  `size_5()` = 20px、设置页 `size_8()` = 32px，直接按这个数缩。

红果 GPUI 壳的落地：`cover_img::decode(bytes, display_px)` 按显示尺寸预缩
（Lanczos3）再造 `RenderImage`；侧栏与设置页各自传自己的显示边长。

### 7.13 「RefCell already borrowed」：gpui 内部拒掉的窗口更新，日志没有出处

**症状**：日志里出现三条**一模一样、没有 target、没有前缀**的
`[2026-10-24T11:08:12.117Z ERROR] RefCell already borrowed`，同一毫秒。

**正文来源（源码钉死）**：Rust ≥ 1.89 的 `BorrowMutError` 的 `Display` 就是
`RefCell already borrowed`（本机 1.99.1 的 `core/src/cell.rs:905`：
`Display::fmt("RefCell already borrowed", f)`）。gpui-pre 的 `AsyncApp::update_window`
用的是 `app.try_borrow_mut()?`（`app/async_context.rs:93`）——双借用时**返回
`Err` 而不是 panic**，上层统一用 `.log_err()` 降级成一条 ERROR 日志。

**为什么没有 target（这是最迷惑的一层）**：gpui-pre-util 的
`log_error_with_caller`（`gpui-pre-util-0.3.9/src/lib.rs:306-338`）是**从调用方
源码路径里拼 target** 的——路径里有 `crates/` 段就切成
`hongguo_gpui::home::stage::render`，没有就拼成**空串**。依赖 crate 装在
`%USERPROFILE%\.cargo\registry\src\...`，**没有 `crates/` 段** → target 为空
→ env_logger 的输出就是 `[时间 级别] 正文`（target 位置空着）。于是依赖 crate
内部打的 ERROR 看起来像来路不明的系统消息。

**为什么恰好三条**：gpui-pre 一帧的请求闭包里连着三个
`handle.update(&mut cx, …)`——`thermal_state`（`window.rs:1809`）、`draw`
（`window.rs:1887`）、`schedule_frame`（`window.rs:1910`）。App 已被借用时这三个
全被拒 → **一条帧请求 = 三条一模一样的日志**。

**根因（我们这侧）**：`App` 是 `RefCell<App>`（`gpui-pre/src/app.rs:84` 的
`AppCell`）。任何在**持有 App 借用时**触发的消息重入都会撞上——Windows 上
`ShowWindow` 会同步派发 WM_SHOWWINDOW/WM_SIZE 到我们自己的窗口过程，gpui 的
窗口过程这时要更新窗口状态。壳里藏显/前置窗口原先就写在 `handle.update(...)`
**借用内**（`shell/mod.rs` 的老板键与单实例前置、`home/incognito.rs` 的隐身
藏显），与 `platform/windows.rs` 里 `SetWindowPos` 的老教训同一类。

**结论与落地**：

- 窗口操作一律**延后到执行器下一拍**（Windows 复用既有的 `defer_window_op`；
  macOS 用 `NSWindow` 的 `Retained` 引用带进本地执行器任务）；
- 空 target 的记录**补上 `file:line`**（`crates/hongguo-gpui/src/logging.rs`），
  下次再犯能直接定位到 gpui 的哪个调用点，不用再从头猜。

### 7.14 流缓存：当前集**不是**只缓存开头

**结论**（`crates/hongguo-service/src/play_service/online/fill.rs` +
`crates/hongguo-stream/src/cache.rs`）：

缓存规则只有一条：**正在播的那一集缓存整集，其余一律只缓存开头 20%**
（`NON_CURRENT_HEAD_PERCENT`，2026-10-24 定档）。

| 条目 | 填多少 | 依据 |
| --- | --- | --- |
| 正在播的那一集 | **整集**（无界续填） | `fill_budget()`：当前集（`set_current` 记）或有读者在等这一档 → `FillBudget::Unbounded`；转正后另有 `resume_fill` 兜底续满 |
| 其余（下一集、连播预取的后续集、换剧预取的后续部） | **只填开头 20%** | `fill_budget()` 的 `FillBudget::Bounded(head_limit)`，上限按**顺序填充前沿**判（不是已下载总量——头部探测与 moov 也算已下载，短集上会把预算提前吃光） |

系统播放器接力（`advance_preloaded`）不重建播放器、也就不走 `prepare`，所以
接力后要显式 `set_current`：否则真正在播的那一集会被当成「其余的」按头部
预算封顶，而上一集反倒享受整集缓存（`crates/hongguo-gpui/src/home/playback/switch.rs`）。

整个缓存另有**字节预算** `RETAIN_BUDGET_BYTES = 48MB`（`cache.rs`）：超了从
最旧的**非当前**条目开始逐出；条目按 `(vid, definition)` 分条，当前集 + 显式
预取 + 最近 3 条（`KEEP_RECENT`）受保护。渐进流本身按 Range 需求供给
（4MB 窗口），不是「一次性缓存开头」。

---

### 7.15 网格列宽：别手算，也别用 `flex_1`

**结论**（`crates/hongguo-gpui/src/pages/grid.rs`）：剧集卡片网格（找剧页 /
新剧页）的列宽**交给布局引擎**，规则收在一处：每行固定 N 个等宽槽。

| 做法 | 后果 |
| --- | --- |
| 手算卡宽（`视口宽 − 侧栏 56 − 内边距 48`） | 侧栏一展开就算错——GPUI 侧栏展开 160px、Tauri 是 224px，手算里写死 56，卡片直接溢出内容区 |
| 用 `flex_1`（基值 0）分槽 | 卡有 1px 边框，flex 基值（内容宽）与空槽不等，**末行每张卡宽出 1px**，列与列对不齐 |
| **百分比基值 + 收缩**（本壳采用） | 每槽基值 = 容器宽 / 列数，由 flex 收缩挤掉间隙 → 每槽 = （容器宽 − 间隙）/ 列数，与 CSS grid 的 `1fr` 列同解；满行/末行都严格等宽 |

配套两条：

- **末行补空槽**：CSS grid 里末行的卡片仍占自己的列宽（右侧留空），纯 flex 行
  会把剩下的卡拉成巨卡——所以缺卡的槽补一个等宽空槽。
- **封面比例交给 `aspect_ratio`**（`aspect_ratio(3/4)`，同 Tauri `aspect-[3/4]`）：
  手算高度用的是**卡宽**，而封面实际比卡内容窄 2px（卡有边框），按卡宽算出来
  的封面就不是 3:4 了。

行高随列宽变（封面高 = 槽宽 ÷ 3:4 × 4/3），虚拟列表会在**列宽变化时作废
高度缓存重测**（`gpui-pre/src/elements/list.rs` 的 "If the width of the list has
changed, invalidate all cached item heights"），窗口缩放不会留下错位的行高。

守卫：`crates/hongguo-gpui/tests/browse_grid_geometry.rs`（断点、列宽、封面
3:4、同行等高、末行留空）。

### 7.16 a11y 节点 id：**只有「有 id 的祖先链」才参与**

**结论**：gpui 的 accesskit `NodeId` = 「**有 id 的祖先链** + 自身 id」的哈希
（`gpui-pre/src/element.rs` 的 `GlobalElementId::accesskit_node_id`）。**没有 id
的元素不进链**——所以同一个 `ElementId` 出现在两条**未加 id** 的分支下时，
两条路径完全相同 → 同一个 `NodeId` → a11y 树里出现重复节点。

后果分两档（`gpui-pre/src/window/a11y.rs`）：

| 构建 | 表现 |
| --- | --- |
| debug | `debug_assert!(false, "Duplicate a11y node id: ...")` → **直接 panic**（abort） |
| release | 节点被**静默丢弃**（无障碍树少一块，界面看着正常） |

**触发面只有「带 a11y role 的元素」**：`Div::a11y_role()` 默认返回 `None`
（`gpui-pre/src/elements/div.rs`：`override_role` 为空就是没有 role），普通
`div().id(...)` 根本不进 a11y 树 → 不会触发重复断言。只有 `base::Button`
（role=Button）、显式 `.role(...)` 的容器、`Input` 这类才在雷区里。

**本壳的实战事故**（2026-10-25，详情页一打开就崩）：「相关作品」与「猜你喜
欢」两段都用 `detail-related-{series_id}` 当卡片 id，而两段容器都没带 id；
服务端 `parse_related_series` 对两个 cell **不去重**，猜你喜欢完全可能命中相
关作品里的同 IP 剧 → 同一部剧两段都有 → 路径相同 → 同 NodeId → panic。

修法是**作用域隔离**，不是「让 id 变随机」：

| 层级 | 做法 |
| --- | --- |
| 可能重复渲染同 id 的容器 | 容器自己带 id（`detail-related-works` / `detail-related-guess`）→ 两段隔成两棵子树 |
| 容器内的重复项 | id 用**段内序号**而不是业务 id（`related-card-{ix}`）——同一部剧在两段/两行里都出现也不会撞 |
| 网格行 | 行容器带 id（`browse-grid-row-{ix}`）→ 卡片 id 用 series_id 也安全（推荐流里同剧会重复出现） |

嵌套在卡里的子控件（如预约钮 `related-reserve-{series_id}`）不用管：它的路径
含卡片的 id，天然跟着卡片走。

守卫：`crates/hongguo-gpui/tests/a11y_id_guard.rs`（两段容器 id 不同且都挂上、
卡片 id 按序号、两页网格行带 id）。**注意**：gpui 测试上下文的 a11y 没激活
（`A11y::is_active()` 由平台的 UI Automation 客户端置位，测试里没有），
这条重复**在测试里复现不了**——只能靠源码守卫 + 代码审查保证。

### 7.17 成品图按「URL 哈希 + 档位」寻址：预热档位必须等于渲染档位

**结论**：GPUI 壳的成品图落盘路径是 `cover-cache/<sha256(url)>-<edge>.jpg`
（`hongguo_protocol::cover::sized_cache_path`）——**档位参与寻址**，所以
「预热 320、渲染找 64」必然取不到文件，头像就是**整块空白**（不是占位图）。

**实测事故**（2026-10-25）：详情页创作者筹码、创作者主页档案、详情页剧评与
回复头像三处全白。铁证是本地缓存目录（`%APPDATA%\hongguo-gpui\cover-cache`）
——同一张头像的原件（`<sha>.jpg`）与封面档产物（`<sha>-320.jpg`）**都在**，
唯独渲染侧要的头像档（`<sha>-64.jpg`）不存在：下载与转码链路没问题，纯粹是
档位错配。

**修法不是「记得传对档位」，而是让调用方没有档位可选**：

```rust
// 之前：档位由调用方传 —— prewarm(&urls, EDGE_CARD, cx)
pub(crate) fn prewarm_covers(urls: &[String], cx: &mut App) { prewarm(urls, EDGE_CARD, cx); }
pub(crate) fn prewarm_avatars(urls: &[String], cx: &mut App) { prewarm(urls, EDGE_AVATAR, cx); }
```

档位在 `cover_img` 模块内固定，渲染侧 `card()` / `avatar()` 用的是同一批常量
→ 预热与渲染**天然同档**。守卫：`crates/hongguo-gpui/tests/avatar_edge_guard.rs`
（入口档位不串、带档位的 `prewarm` 不外泄、调 `cover_img::avatar` 的文件清单
与预热清单一一对应——新增渲染点会当场失败）。

附带一条协议层事实（`hongguo-protocol` 的 `sized_cache_path_is_edge_scoped`）：
档位换档就是换文件，这是上面那条不变量成立的前提。

### 7.18 浮层「点外部关闭」「按住拖动」「层级」：gpui 的三条硬事实

**结论**：三条都是 GPUI 的事件模型决定的，不是实现疏忽：

1. **控制条根节点的 `stop_propagation` 会掐断舞台捕获层**。gpui 的元素级
   监听器是**窗口级**的（paint 时注册），bubble 阶段按「子元素先、父元素后」
   派发；控制条根节点 `on_mouse_down → stop_propagation`（`controls/bar.rs`）
   一跑，**排在它后面注册的舞台捕获层就永远收不到这一击**。所以「点外部关闭」
   不能指望舞台层那张透明捕获层——它只管画面，控制条上的控件它一概收不到。
2. **拖拽捕获期间 hover 位会掉**。gpui 进入拖拽后不再按命中盒判 hover，
   hover 驱动的弹层当场收起 → 滑杆从元素树里消失 → `on_drag_move` 再也送不到
   它，音量停在按下处（用户实测「按住拖不动」）。
3. **gpui 没有 z-index，层级就是绘制顺序**。后画的兄弟元素直接盖住先画的；
   而且**鼠标事件按命中盒几何派发**——重叠的兄弟元素**都会**收到
   `mouse_down`（所以「盖住」不等于「吃掉点击」，反过来也不成立）。
   控制栏向上弹的浮层必须用 `gpui_kit::deferred(...)` 挪到最后画，
   否则会被后画的评论面板 / 连播横条 / 登录面板压住。

**修法**：

```rust
// ① 浮层自己挂窗口级监听（capture 阶段、按命中盒判内外），不吞底下控件的点击
.on_mouse_down_out(cx.listener(|this, ev: &gpui_kit::MouseDownEvent, _, cx| {
    // 入口按钮本身排除在外：那一下交给按钮的 on_click 收起，
    // 否则「捕获层先关 → 按钮再开」会来回横跳
    if let Some(b) = this.picker_entry_bounds.get() && b.contains(&ev.position) { return; }
    this.picker_open = false;
    cx.notify();
}))

// ② 拖动期间把弹层钉住（hover 位在拖拽捕获期间不可靠）
let open = self.volume_hovered || self.volume_popup_hovered || self.volume_dragging;

// ③ 向上弹的浮层挪到最后画（本壳表达「层级」的既定手段）
gpui_kit::deferred(popup).with_priority(1)
```

三条都靠 gpui-pre 0.3.9 的既有 API：`Div::on_mouse_down_out`（capture 阶段、
`!hitbox.contains(&window.mouse_position())` 才触发、**不** stop propagation）
与 `Div::on_drag` / `on_drag_move`（后者在 mouse move 的 **capture** 阶段查
`cx.active_drag` 的载荷类型）。

Tauri 侧同一问题用 CSS z-index 解决（控制栏根 `z-30`、浮层统一 `z-30`、
评论面板 `z-40`）；GPUI 侧没有 z-index，只能靠绘制顺序，所以**每个向上弹的
浮层都要 deferred**——这条在本壳是不变量，由源码守卫钉住。

**顺带一条 gpui 拖拽语义**（写测试时踩到）：`on_drag` 是在 mouse move 的
bubble 阶段**起拖**的，而 `on_drag_move` 在 capture 阶段派发——所以**第一段
移动只起拖、第二段才开始收到 drag_move**。断言要连发两段移动。

守卫：`crates/hongguo-gpui/tests/picker_dismiss.rs`（3 条行为 + 1 条接线）、
`crates/hongguo-gpui/tests/volume_drag.rs`（2 条行为 + 1 条接线；其中一条是
**负对照**——纯 hover 裁决的弹层必须拖不动，否则说明断言没咬到点上）、
`crates/hongguo-gpui/tests/overlay_layering.rs`（3 条：deferred 后 hover 与命中
照常、重叠兄弟都收到 mouse_down、真壳三个浮层都 deferred 的接线守卫）。
接线守卫读源码文本，保证真壳用的是同一套机制（复刻通过 ≠ 真壳接了）。

---

## 附录 A：未查到 / 待验证项（如实列出）

| 项 | 状态 |
| --- | --- |
| Showcase 应用的 preview 截图视觉细节 | **未验证**（PNG 二进制，纯文本抓取无法读取图像）。1.4 节九个标杆的界面形态基于 README / 官方文档文字证据；其余 76 个应用仅依据 manifest 描述 |
| Zed 的 `crates/gpui/src/focus.rs` | **未查到**确切路径；焦点相关确认为 `crates/gpui/src/tab_stop.rs` + `key_dispatch.rs` |
| gpui-kit 是否内置 grid / masonry 组件 | **确认不存在**（依据 `crates/component/src/lib.rs` 的 68 个 `pub mod` 全量清单 + `crates/base/src/styled.rs` 仅 flex 系列），此为「确认无」，非「未查到」 |
| Orrery 的网格是等高还是瀑布流 | **未查到** |
| gpui-kit 是否有「新建项目」脚手架命令 | **未查到**；提供的是组件画廊（`crates/story`、`crates/story-web`）与 `examples/` 参考实现 |
| gpui-kit 各组件的完整 API 签名 | 本报告只列**模块名**与职责；具体 API 见 `docs.rs/gpui-kit`（<https://docs.rs/gpui-kit>） |
| gpui-base / gpui-pre 是否暴露 `text_center()` 之类的链式文本对齐 setter | **确认不存在**（`Style` 有 `text_align` 字段，但没对外暴露链式 setter），故音量读数暂按左对齐 |

## 附录 B：主要引用 URL 索引

**gpui-kit 官方**
- 站点首页 <https://gpui-kit.com/> ｜应用页 <https://gpui-kit.com/apps/> ｜设计规范 <https://gpui-kit.com/versions/main/docs/design-guides/>
- 仓库 <https://github.com/longbridge/gpui-kit> ｜README <https://raw.githubusercontent.com/longbridge/gpui-kit/main/README.md> ｜AGENTS.md <https://raw.githubusercontent.com/longbridge/gpui-kit/main/AGENTS.md> ｜release notes <https://raw.githubusercontent.com/longbridge/gpui-kit/main/release-notes.md>
- 组件模块清单 <https://raw.githubusercontent.com/longbridge/gpui-kit/main/crates/component/src/lib.rs>
- 语义令牌 <https://raw.githubusercontent.com/longbridge/gpui-kit/main/crates/base/src/theme_tokens.rs> ｜组件令牌 <https://raw.githubusercontent.com/longbridge/gpui-kit/main/crates/component/src/theme/theme_color.rs>
- 设计规范 <https://raw.githubusercontent.com/longbridge/gpui-kit/main/website/docs/design-guides.md> ｜编码规范 <https://raw.githubusercontent.com/longbridge/gpui-kit/main/website/docs/coding-guides.md>
- crates 目录 <https://api.github.com/repos/longbridge/gpui-kit/contents/crates> ｜base 模块 <https://api.github.com/repos/longbridge/gpui-kit/contents/crates/base/src> ｜examples <https://api.github.com/repos/longbridge/gpui-kit/contents/examples>
- crates.io <https://crates.io/api/v1/crates/gpui-kit> ｜docs.rs <https://docs.rs/gpui-kit>

**Showcase**
- 仓库 <https://github.com/longbridge/gpui-kit-showcases> ｜featured <https://raw.githubusercontent.com/longbridge/gpui-kit-showcases/main/featured.json> ｜CONTRIBUTING <https://raw.githubusercontent.com/longbridge/gpui-kit-showcases/main/CONTRIBUTING.md>

**Zed / GPUI**
- gpui.rs <https://gpui.rs/> ｜gpui 源码 <https://api.github.com/repos/zed-industries/zed/contents/crates/gpui/src> ｜elements <https://api.github.com/repos/zed-industries/zed/contents/crates/gpui/src/elements>
- workspace <https://api.github.com/repos/zed-industries/zed/contents/crates/workspace/src> ｜ui 组件 <https://api.github.com/repos/zed-industries/zed/contents/crates/ui/src/components> ｜command_palette <https://api.github.com/repos/zed-industries/zed/contents/crates/command_palette/src> ｜settings_ui <https://api.github.com/repos/zed-industries/zed/contents/crates/settings_ui/src> ｜settings <https://api.github.com/repos/zed-industries/zed/contents/crates/settings/src> ｜tab_switcher <https://api.github.com/repos/zed-industries/zed/contents/crates/tab_switcher/src>
- PR #64057 <https://github.com/zed-industries/zed/pull/64057> ｜#64594 <https://github.com/zed-industries/zed/pull/64594> ｜#65013 <https://github.com/zed-industries/zed/pull/65013> ｜#65256 <https://github.com/zed-industries/zed/pull/65256> ｜#65753 <https://github.com/zed-industries/zed/pull/65753> ｜#50827 <https://github.com/zed-industries/zed/pull/50827> ｜#61945 <https://github.com/zed-industries/zed/pull/61945> ｜#62379 <https://github.com/zed-industries/zed/pull/62379>
- issue #8043 <https://github.com/zed-industries/zed/issues/8043> ｜#4355 <https://github.com/zed-industries/zed/issues/4355> ｜#52075 <https://github.com/zed-industries/zed/issues/52075>

**gpui-kit issue / PR**
- PR #3085 <https://github.com/longbridge/gpui-kit/pull/3085> ｜issue #3513 <https://github.com/longbridge/gpui-kit/issues/3513> ｜issue #3295 <https://github.com/longbridge/gpui-kit/issues/3295> ｜PR #2948 <https://github.com/longbridge/gpui-kit/pull/2948> ｜PR #3576 <https://github.com/longbridge/gpui-kit/pull/3576> ｜issue #2795 <https://github.com/longbridge/gpui-kit/issues/2795> ｜issue #3647 <https://github.com/longbridge/gpui-kit/issues/3647> ｜PR #3667 <https://github.com/longbridge/gpui-kit/pull/3667>

**标杆应用**
- tbis <https://github.com/mahi160/tbis> ｜FluxDown <https://github.com/zerx-lab/FluxDown> ｜Zedis <https://github.com/vicanso/zedis> ｜disktree <https://github.com/tobi/disktree> ｜Panda Reader <https://github.com/yuhangch/panda-reader> ｜Pawse <https://github.com/popovpsk/pawse> ｜Cadence <https://github.com/infomiho/cadence> ｜Meow Player <https://xarzhi.github.io/meow-player-docs/> ｜Orrery <https://github.com/Hankanman/Orrery> ｜Ghostex <https://github.com/maddada/Ghostex> ｜Reviu <https://github.com/reviu-dev/reviu> ｜Longbridge Pro <https://longbridge.com/desktop> ｜Longbridge Lite <https://github.com/longbridge/longbridge-lite> ｜Navop <https://github.com/feigeCode/navop> ｜ShellRS <https://github.com/since2006/shell-rs> ｜Nohrs <https://github.com/noh-rs/nohrs> ｜Setu <https://github.com/bajrangCoder/setu> ｜BuMoSm PDF Viewer <https://github.com/BuMoSm-Series/BuMoSmPDFViewer>
