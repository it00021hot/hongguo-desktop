# NSH 四端壳实施方案

状态：方案已确认，正在实施。用户已明确选择纯 Rust 与独立移动端宿主。

本轮目标：新增第三个独立壳，覆盖 Windows、macOS、Android、iOS。
保持客户端直接调用红果官方 API 的工作方式；现有 Rust service 是本地业务内核，
不引入独立部署的服务端。Linux 与 Web 留待后续版本，本方案不展开。

## 1. 技术基线与已核实事实

- 本机 `nsh --version` 已确认为 `0.4.0 (e626a8d)`。
- 实现采用纯 Rust 模式，从 `nsh init --template rust` 生成的工程接入根 workspace。
  Android/iOS 使用本仓库的独立宿主，在挂载 NSH 根组件前提供系统私有目录，
  不加入 TSX 启动层或 QuickJS。
  NSH 依赖统一使用 0.4.0，与 CLI 对齐；依赖源以生成物和锁文件为准。
  本机另有 NSH 源码目录，但其工作区版本不同且有未提交修改，不将它作为应用依赖源。
- NSH 原生视频组件在 Apple 平台使用 AVPlayer，Android 使用 MediaPlayer，
  Windows 使用 Media Foundation。
  这些是播放后端，不代表自动支持所有红果流格式。
- 0.4.0 的 Linux 文档仍明确列出视频倍速不支持，因此 Linux 整个平台延后，
  本轮不开发 Linux 播放适配或承担其验收。
- `hongguo-service` 已无 UI 框架依赖，可以复用；现有 `hongguo-player`
  的真实实现仅覆盖 Windows/macOS，且画面挂接服务于既有壳，不能直接当作四端 NSH 播放器。
- 现有平台转码实现覆盖 Windows/macOS/Android；本轮需补齐 iOS 的媒体能力。

版本依据：[0.4.0 发布记录](https://github.com/longbridge/nsh/releases/tag/v0.4.0)、
[0.4.0 Rust 模式](https://github.com/longbridge/nsh/blob/v0.4.0/website/docs/rust.md)、
[0.4.0 视频后端](https://github.com/longbridge/nsh/blob/v0.4.0/website/docs/web-view-and-video.md)、
[0.4.0 Linux 限制](https://github.com/longbridge/nsh/blob/v0.4.0/website/docs/linux.md)。

## 2. 分层与目录归属

新增 `crates/hongguo-nsh/`，保持入口只装配，业务功能按域组织：

```text
hongguo-nsh/
  Cargo.toml / nsh.json
  DESIGN.md / theme.json
  src/
    lib.rs / main.rs
    bootstrap/       启动、目录、数据库、设置和设备身份装载
    bridge/          service 异步结果与业务事件接入 NSH 生命周期
    ui/              基于 nsh-base 的应用样式组件与动效
    pages/           推荐、搜索、详情、榜单、历史、设置等
    player/          NSH 播放组件、控制与供数适配
    platform/        窗口、移动端生命周期、目录与导出适配
    i18n/            加载现有中英文资源
  tests/
```

依赖方向：NSH 页面 → 本壳交互与状态 → 既有 service/api → store/signer/stream/media。
平台分支集中在装配与平台适配处，业务层不引用 NSH，页面不分散条件编译。
只在真实复用点抽共享模块，不提前搬迁 GPUI 或 Tauri 的业务装配。

新增平台媒体实现沿用现有独立 crate 规则：iOS 媒体实现归
`hongguo-media-platform-ios`，
由既有媒体门面分派。具体依赖在平台验证阶段确定，不凭名称引入未核实的库。
macOS/iOS 共用的 VideoToolbox 管线归 `hongguo-media-videotoolbox`，不复制实现。
按本机 SDK 头文件核实，像素转移 API 从 iOS 16.0 可用；本壳 iOS 下限定为 16.0。
硬件选项和探测键的部分符号直到 iOS 17/17.4 才导出，iOS 使用可选符号查询，
缺少探测信息时不宣称硬件编码已验证。

## 3. 本地业务接入

1. 独立构造 `AppState`，复用现有数据库、设备注册、登录、查询、下载和播放服务。
   不调用 Tauri IPC，也不复制签名、请求参数或数据库实体逻辑。
2. 服务任务统一投到 `hongguo_service::runtime`，通过可等待结果接回 NSH。
   UI 状态只在 NSH 所属执行环境更新，数据库同步门面不阻塞界面线程。
3. 订阅既有 `ServiceEvent`，将任务进度、登录和播放准备结果交给本壳状态。
   处理订阅溢出后的状态重读、页面退出、请求取消及旧结果覆盖新页面的问题。
4. 持久化状态由 service/store 所有，页面临时状态由页面所有；进度事件不驱动整页重建。
5. 桌面数据目录用独立的 `hongguo-nsh` 根，与另两个壳平级隔离。
   Android/iOS 在任何目录解析、注册缓存或开库之前设置 app 私有绝对数据根。
6. 沿用 `src/locales/zh-CN.json` 与 `en-US.json`，新增文案同步两份。
   不复制出第三套翻译；先验证 Rust 侧资源装载和占位符格式，再接入页面。

## 4. 播放与媒体：先验证整条链路

统一走现有选档、拉流、解密与兼容处理逻辑，NSH 负责呈现、控制与平台播放事件。
播放视图默认采用 NSH 的 `video()`，控制栏使用本壳 `ui` 组件。

NSH 的标准视频源是 URL，不能假定它理解 `hongguo-stream` 或 Tauri 自定义协议。
推荐由应用进程提供仅绑定 loopback 的播放供数入口，复用现有 Range 供数逻辑，
将已准备好的明文媒体交给 NSH 视频组件。这个入口随客户端启动退出，
只承载本机媒体读取，不是远端部署服务，也不承载官方 API 转发。

供数入口必须验证：随机端口、会话隔离、请求取消、Range/HEAD/206/416、跳播、
并发读取、缓存淘汰、播放器销毁与重新起播。限制可访问的媒体对象，
不得提供任意文件路径或任意 URL 代理；不能依赖视频组件能附加自定义认证头。
移动端需验证系统对本机 HTTP 的限制和前后台切换，权限配置只放行实际所需范围。

| 平台 | 播放与媒体落实事项 |
| --- | --- |
| Windows | 验证 NSH Media Foundation 播放明文渐进流、倍速、音画同步和控制栏覆盖；复用既有 Windows 转码 |
| macOS | 验证 NSH AVPlayer、供数与跳播；复用既有 VideoToolbox 转码，不复用 GPUI 的窗口挂层 |
| Android | MediaPlayer + 既有 MediaCodec 兼容处理；验证音频焦点、来电、横竖屏及后台恢复 |
| iOS | AVPlayer；补齐 iOS 媒体能力探测与必要的 VideoToolbox 兼容处理，不能把 macOS crate 取消门控就视为支持 |

样本覆盖 H.264、HEVC、ByteVC2、CENC、长视频跳播与失效流地址。
ByteVC2 不假定系统可解；沿用已存在的兼容路径并核实每端执行条件。
缺少解码器或兼容处理失败必须明确显示原因，不用永久加载掩盖。
移动端兼容流若未通过，记录为未完成项，不能宣布四端播放完成。

## 5. 移动端语义

私有目录是数据库和工作缓存的唯一默认归属；文件导出通过系统提供的目标授权完成，
不把桌面绝对路径规则直接搬到 Android/iOS。选择目录、打开文件所在位置、
托盘、全局快捷键和小窗等能力按平台实际支持提供。

后台切换时保存播放位置、释放或暂停平台要求释放的资源；恢复时重新检查播放源有效性。
下载与转码的状态可恢复，但不承诺 iOS/Android 后台无限运行。
验证系统挂起与进程终止后的队列恢复；若要求后台持续下载，须另行落实系统任务机制。

## 6. 实施顺序与交付条件

以下按技术依赖推进，最终交付目标仍为完整四端壳，不以脚手架或静态页面代替功能。

| 阶段 | 工作 | 通过条件 |
| --- | --- | --- |
| A：工程与平台验证 | 生成 Rust 工程，接入 workspace；四端最小启动、Turso 开库、真实媒体供数与播放验证 | 明确四端工具链、系统下限、解码依赖与播放缺口；在铺业务页面前解决架构阻断 |
| B：基础接入 | 独立目录、状态装配、运行时桥、事件桥、登录与设备身份 | 请求和数据均复用既有服务；不写入其他壳的数据目录；退出能清理本壳任务与订阅 |
| C：界面基础 | 在 DESIGN.md 定义主题、字体、尺寸、交互与动效；建立 ui 组件层 | 明暗主题、键盘焦点、无障碍、移动触控和减少动态效果逐项验收 |
| D：核心链路 | 推荐/搜索 → 详情 → 播放 → 切集/续播/历史；互动与账号 | 四端完成真实完整用户流程，包含加载、失败、空态和取消 |
| E：功能对齐 | 按现有 Tauri 功能建立清单，完成榜单、新剧、追剧、预约、收藏、下载、合并、存储与设置等 | 每项明确通过、平台不适用或未完成；平台限制可见且有明确交互 |
| F：交付 | 四端构建打包、签名配置、安装与回归说明 | 每端产物可安装；原两个壳的既有检查通过；未验证项目逐项列明 |

纯结构调整与行为改动分开。CI 修改和发布另行取得明确授权；本方案不触发发布。
实施时新增本地 NSH 运行、检查、测试与打包入口，并更新 AGENTS.md、项目 skill 和使用文档。

## 7. 验证范围与当前环境

- Rust 命令统一从仓库根按包运行；完成针对性构建、格式、Clippy、行为测试。
  NSH 检查与测试入口先确认 workspace 行为，避免生成额外工作区或锁文件。
- 运行时与事件桥验证取消、析构、旧响应和订阅恢复；供数验证 Range 与真实跳播。
- NSH 无界面测试验证交互行为；真机/模拟器验证播放器、文件授权和生命周期。
  界面需检查明暗主题和桌面/移动布局，截图不能代替媒体验证。
- Windows、macOS 分别在对应宿主完成桌面打包与运行验证。
  Android 需要 SDK/NDK/JDK；iOS 模拟器与设备需要完整 Xcode，设备安装另需签名。
- 本机已安装 Android、iOS 和桌面相关 Rust 目标；已按用户授权补齐 Android SDK/NDK/JDK
  与模拟器、Xcode 27.2 与 iOS SDK/模拟器运行环境。Android 原生 release 编译、iOS 模拟器
  release 应用包签名已取得实际通过证据，最终运行与完整媒体链路仍需验收。

实施与验证进度见 [NSH 开发说明](../.agents/skills/hongguo-dev/references/nsh.md)。
宿主平台上的条件编译空壳检查不算 Android/iOS 编译通过；四端功能与发布验收仍需分别完成。
