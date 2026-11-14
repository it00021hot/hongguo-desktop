# 播放窗口模式

规格来源：Tauri use-mini-window/use-portrait-window、window_cmd/incognito_cmd、AppShell、MiniScreenControls/MiniDragBar，以及 Android MainActivity.setLandscapeLocked。始终调整当前窗口，不创建第二个播放器，不换源/暂停/落库来切换尺寸。

- Mini：480×270，工作区右下 16 点；最小 360×180。摘掉顶栏/侧栏/底导航；顶部 24 点原生拖拽带；标题/集数/简介、同源 seek 与 28 点紧凑按钮。
- Portrait：手机比例取 min(真实竖视频比,9/19.5)，普通高至多工作区 70%，min360×400；保留 44 顶栏，隐藏分类插槽/侧栏/底导航。铺满使用当前工作区，再还原用户普通竖窗快照。换流比例变化超过 1% 才 refit。
- Frameless：保留原几何，隐藏壳 UI；mac 只藏交通灯，不改 styleMask。与系统全屏互斥。Mini/Portrait 互斥且恢复原始 Normal 快照，置顶意图独立于形态。
- Incognito：系统光标出窗先暂停、至少 80ms 后原窗隐藏，回到原矩形显示且不夺焦点。显式用户暂停不改写；最小化/老板键隐藏时不自动抢回。唯一 native 窗口桥聚合藏显。
- 手机横屏：系统确认旋转后更新模式；Android 仅藏导航条、保留状态栏。退出/移除宿主恢复原方向策略与系统栏。无确认或系统拒绝明确报错。

动效清单：系统窗口几何直接设置，系统全屏使用 OS 自带过渡；不对视频平面作入退场动画。壳 DisplayNone 切换保留子树 owner。紧凑控制条仅复用已有 chrome 渐隐/减少动效策略；按钮复用共享 hover/press/focus，seek 复用真实 Slider，不另建假的进度条。隐身的暂停送达窗口是行为时序，不用视觉透明度冒充原生隐藏。

小窗置顶与普通顶栏一致：关闭为空心图钉，开启使用同一 PinFilled 实心资源；
两态均透明底、无选中边框，图标保持播放器浅色，可访问名称切换置顶/取消置顶。
图形即时切换，不新增动画；hover/pressed/focus 继续使用紧凑播放器控件反馈。

不可见暂停：设置开启时复用已有窗口组件计时器，每 250ms 至多查询一次真实可见性，
只在变化时发布 visibility 布尔事件；关闭设置后停止查询并解除临时门禁。
macOS 先检查当前窗口 isVisible/isMiniaturized/occlusionState，再用 Window Server 从前到后的
窗口边界扣除覆盖区域；窗口元数据查询不截屏、不读取标题、不要求屏幕录制权限。
仅使用系统遮挡标志会漏掉近乎不透明的逐像素合成窗口：独立实播夹具以 99% 黑色背景复现
系统仍报告 Visible、原生播放 rate 始终为 1 的问题，区域检测补齐这一场景。
同进程浮层不计作外部遮挡，窗口整体 alpha 小于 1 的覆盖层不扣除；逐像素透明内容无法仅由
公开窗口元数据精确还原，采用应用窗口矩形覆盖语义。屏幕之外的部分不计作可见区域。
可见性查询独立于窗口几何采集，窗口 screen 暂不可用时也必须发布不可见状态。
Windows 使用显示状态、DWM cloak、
屏幕区域与上方不透明窗口区域的差集。多个普通窗口共同盖满同样暂停，部分可见和单纯失焦继续播放。
Windows 跳过透明/色键及无法确认全不透明的分层窗口，避免把透过覆盖层仍可见的视频误判为不可见；
特殊合成透明效果和 Windows 实际运行仍需真机确认。实现依据见
[IsWindowVisible 的遮挡限制](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindowvisible) 与
[DWM 属性](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)。

原生暂停与用户暂停分离：恢复可见只重新允许播放，用户手动暂停仍保持；
当前路由、老板键/隐身和来源身份的既有门禁继续生效，不重建视频或重置时间轴。
macOS 独立外部进程的真实不透明窗口测试已通过：部分遮挡、完全遮挡与恢复、手动暂停保持、
最小化、隐藏和禁用开关；它验证真实窗口事件与生产播放准入函数，不代替真实流音画测试。
两项小窗明暗用例及两项设置明暗用例通过，NSH 全部 342 项 Rust 测试通过。
macOS 与真实 Windows MSVC 目标的 all-targets 严格 Clippy 通过；Windows 真机行为未验。

真实可见性用例放在 .native 目录，避免被无原生窗口的常规测试自动收集；从仓库根显式运行：

```sh
swiftc scripts/nsh-visibility-cover.swift -o target/nsh-visibility-cover
DYLD_LIBRARY_PATH="$PWD/target/native-media/aarch64-apple-darwin/libde265-1.1.3/lib" \
CARGO_TARGET_DIR="$PWD/crates/hongguo-nsh/preview/target" \
nsh test crates/hongguo-nsh/preview/layout/.native/Visibility.test.tsx --target macos
```

遮挡进程和窗口只由离线预览创建，3 秒后自行退出，预览销毁时也会回收所创建进程；
该辅助程序不打入客户包。保留同进程窗口测试的中间失败日志，最终以外部不透明窗口的通过结果为准。

真实播放回归：`node scripts/nsh-visibility-playback.mjs`，使用仓库合成 HEVC/AAC 样本、
正常桌面后端和生产 renderer，不启用测试宿主强制刷新。独立 AppKit 计时器读取本进程
AVPlayerLayer 的真实 player.rate，与原生进度回调共同验证暂停、时间轴停止和恢复；
覆盖普通窗口、透明合成窗口、部分遮挡、先前手动暂停和关闭设置五种场景。
每个实例使用 target 下独立目录，自动回收精确创建的遮挡进程，不操作用户在用实例。

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
