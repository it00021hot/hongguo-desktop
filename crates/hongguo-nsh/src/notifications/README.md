# 系统更新通知

通知由应用自己的原生组件 `hongguo.notifications` 承载，不修改 NSH SDK，也不使用 QuickJS。
`Notifications` 是 App owner 内创建并捕获的句柄；异步任务不得重新查找上下文。
权限请求只由用户开启「弹出提醒」触发。关闭开关不申请权限、不发送通知；已有本地提醒记录仍可查看。

| 平台        | 接入                                                                       | 权限与生命周期                                                                                                                  |
| ----------- | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| macOS / iOS | UserNotifications，前台显示 banner/list；点击经 delegate 回到 NSH 激活窗口 | 首次显式请求时初始化通知中心；裸 macOS binary 返回需使用 `.app` 的错误，启动不触碰通知中心；组件卸载时仅恢复自己接管的 delegate |
| Windows     | WinRT Toast，DOM 文本节点承载标题/正文；点击回到 NSH 激活窗口              | 当前用户下注册独立 `com.hongguo.nsh` AUMID；查询系统通知开关；保有 toast 至点击、消失或失败，卸载时释放 COM 与持有项            |
| Android     | NotificationManager、独立更新频道和应用内打开 PendingIntent                | Android 13+ 显式申请 POST_NOTIFICATIONS；拒绝及频道关闭均返回未授权；点击打开本应用 Activity                                    |

Apple 的 `objc2-user-notifications 0.3.2`、Windows 的 `windows 0.62` 均复用仓库
已锁定的 MIT 平台绑定，只补目标依赖声明与必要 features，未升级版本或引入通知框架。

请求具有递增序号和逐条回执，允许乱序完成。原生侧按序号去重，Rust 侧只移除已完成项；
最多同时保有 32 个未完成请求，超过时明确报错。追更事件逐条等待发送回执，不并发冲垮队列。
系统接受发送请求不等于用户看见通知；Windows 异步投递失败单独记录日志。

这是本地通知，消息来源仍为已有追更检查服务。手机进程被系统挂起或终止时，不承诺继续检查更新；
没有 APNs/FCM 服务，也没有应用自己的后台服务。

2026-11-13 验证：三项 Headless 行为测试通过，覆盖跨 owner 延迟请求、乱序/重复回执、
未响应原生宿主的容量上限；所有权回归先确认旧实现将请求送入错误的子 owner，再改为捕获句柄后通过。
macOS 库严格 clippy、Android 宿主 Java 编译通过。

Android API 37 自有模拟器 `emulator-5580` 的独立应用
`com.hongguo.nsh.notificationprobe` 直接链接生产 `NotificationComponent.java`，未复制修改组件逻辑。
实际确认未授权发送返回未授权且不弹框、显式请求后拒绝返回未授权、再次请求允许返回已授权；
权限流程 Maestro 1/1、17 秒。随后在系统通知栏看到正确标题和正文，点击后该验证应用从后台回到前台；
点击流程 Maestro 1/1、4 秒，已保存通知栏截图和系统前台 Activity 记录。
首轮验证夹具因状态文字被状态栏遮挡导致选择器失败，修正夹具布局后重跑，未计为产品失败或通过。

证据位于 `target/nsh-followup-20261113/notification-probe-*`。本次仅验证自有测试应用的原生通知桥，
未使用真实账号制造追更事件。macOS、iOS、Windows 的实际授权、展示和点击仍未验收。

Windows 生产通知适配器与传输类型另外通过真实 `x86_64-pc-windows-msvc` 严格 clippy。
独立轻量 crate 通过 `#[path]` 引用生产源码，使用同一 NSH revision 与锁定版本的平台绑定，
未复制适配器、修改 SDK 或伪造目标条件。首轮发现 `RoInitialize` 已返回 `Result`、
额外 `.ok()` 错转为 `Option` 的目标编译错误，修正后零警告通过。
证据为 `notification-windows-probe-{red,green}.log`。随后平行安装与既有 VVdec 兼容的
LLVM 19.1.7，保留默认 LLVM 23、原有 Werror 和源码不变，完成完整 NSH 的真实 MSVC
`--all-targets` 严格 clippy，退出码 0；记录为 `desktop/windows-strict-llvm19-final.log`。
完整目标检查通过仍不能等同于 Windows 安装运行或实际通知展示通过。
