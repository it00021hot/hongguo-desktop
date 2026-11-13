# 播放窗口模式

规格来源：Tauri use-mini-window/use-portrait-window、window_cmd/incognito_cmd、AppShell、MiniScreenControls/MiniDragBar，以及 Android MainActivity.setLandscapeLocked。始终调整当前窗口，不创建第二个播放器，不换源/暂停/落库来切换尺寸。

- Mini：480×270，工作区右下 16 点；最小 360×180。摘掉顶栏/侧栏/底导航；顶部 24 点原生拖拽带；标题/集数/简介、同源 seek 与 28 点紧凑按钮。
- Portrait：手机比例取 min(真实竖视频比,9/19.5)，普通高至多工作区 70%，min360×400；保留 44 顶栏，隐藏分类插槽/侧栏/底导航。铺满使用当前工作区，再还原用户普通竖窗快照。换流比例变化超过 1% 才 refit。
- Frameless：保留原几何，隐藏壳 UI；mac 只藏交通灯，不改 styleMask。与系统全屏互斥。Mini/Portrait 互斥且恢复原始 Normal 快照，置顶意图独立于形态。
- Incognito：系统光标出窗先暂停、至少 80ms 后原窗隐藏，回到原矩形显示且不夺焦点。显式用户暂停不改写；最小化/老板键隐藏时不自动抢回。唯一 native 窗口桥聚合藏显。
- 手机横屏：系统确认旋转后更新模式；Android 仅藏导航条、保留状态栏。退出/移除宿主恢复原方向策略与系统栏。无确认或系统拒绝明确报错。

动效清单：系统窗口几何直接设置，系统全屏使用 OS 自带过渡；不对视频平面作入退场动画。壳 DisplayNone 切换保留子树 owner。紧凑控制条仅复用已有 chrome 渐隐/减少动效策略；按钮复用共享 hover/press/focus，seek 复用真实 Slider，不另建假的进度条。隐身的暂停送达窗口是行为时序，不用视觉透明度冒充原生隐藏。

预览 driver 只记录 typed 请求并由 fixture 显式 ack，不调整 LayoutPreview 实际窗口。系统行为只在自建实例验收；Headless/宿主编译不冒充跨平台运行通过。

2026-11-13 桌面全屏底栏修正：Tauri `player-controls.tsx` 的倍速至全屏为
`ml-auto flex` 右组；NSH 原先缺少自动左边距，全屏隐藏224点侧栏后右缘停在1083点，
而 seek 与舞台早已扩至1280点，并非原生视频宽度未更新。共享控制条补同一右组后，
真实布局回归从1083/1264失败转为通过；1280及1600视口分别保持16点右内边距，
全屏退出后同一舞台仍在。日志 `target/nsh-commit-20261113/fullscreen-controls-{red,green}.log`。

独立LayoutPreview原生2/2通过（`fullscreen-controls-native.xml`）：全屏dark与无边框light
切换、退出后顶栏/侧栏返回、舞台仅一份。两图
`target/nsh-preview/window-{fullscreen-trailing-dark,frameless-trailing-light}.png`
已逐张目检，底栏全屏按钮与进度条右缘均保留16点边距，右侧不再空出旧宽度差。
这验证共享舞台真实AppKit布局与交互；窗口模式由预览driver确认，不代表重新测试macOS系统全屏动画。
未操作用户实例，也未运行移动端。
