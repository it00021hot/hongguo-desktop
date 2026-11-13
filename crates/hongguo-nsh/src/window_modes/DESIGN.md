# 播放窗口模式

规格来源：Tauri use-mini-window/use-portrait-window、window_cmd/incognito_cmd、AppShell、MiniScreenControls/MiniDragBar，以及 Android MainActivity.setLandscapeLocked。始终调整当前窗口，不创建第二个播放器，不换源/暂停/落库来切换尺寸。

- Mini：480×270，工作区右下 16 点；最小 360×180。摘掉顶栏/侧栏/底导航；顶部 24 点原生拖拽带；标题/集数/简介、同源 seek 与 28 点紧凑按钮。
- Portrait：手机比例取 min(真实竖视频比,9/19.5)，普通高至多工作区 70%，min360×400；保留 44 顶栏，隐藏分类插槽/侧栏/底导航。铺满使用当前工作区，再还原用户普通竖窗快照。换流比例变化超过 1% 才 refit。
- Frameless：保留原几何，隐藏壳 UI；mac 只藏交通灯，不改 styleMask。与系统全屏互斥。Mini/Portrait 互斥且恢复原始 Normal 快照，置顶意图独立于形态。
- Incognito：系统光标出窗先暂停、至少 80ms 后原窗隐藏，回到原矩形显示且不夺焦点。显式用户暂停不改写；最小化/老板键隐藏时不自动抢回。唯一 native 窗口桥聚合藏显。
- 手机横屏：系统确认旋转后更新模式；Android 仅藏导航条、保留状态栏。退出/移除宿主恢复原方向策略与系统栏。无确认或系统拒绝明确报错。

动效清单：系统窗口几何直接设置，系统全屏使用 OS 自带过渡；不对视频平面作入退场动画。壳 DisplayNone 切换保留子树 owner。紧凑控制条仅复用已有 chrome 渐隐/减少动效策略；按钮复用共享 hover/press/focus，seek 复用真实 Slider，不另建假的进度条。隐身的暂停送达窗口是行为时序，不用视觉透明度冒充原生隐藏。

预览 driver 只记录 typed 请求并由 fixture 显式 ack，不调整 LayoutPreview 实际窗口。系统行为只在自建实例验收；Headless/宿主编译不冒充跨平台运行通过。
