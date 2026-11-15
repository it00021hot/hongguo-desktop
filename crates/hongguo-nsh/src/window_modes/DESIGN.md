# 播放窗口模式 / Playback window modes

始终调整当前窗口，不创建第二个播放器，不为切换尺寸换源、暂停或落库。
桌面窗口几何、显示器工作区、最小尺寸、最大化/全屏、置顶及藏显激活统一使用
NSH `WindowRef`、`WindowAttributes` 和窗口状态。命令与状态通过共享 Rust 控制器传递，
不再为桌面基础操作维护 JSON 原生桥或自定义窗口命中测试。

Windows 使用 NSH frame、阴影、缩放边缘与系统圆角；小窗/无边框设为 `WindowFrameStyle::None`，
普通窗口恢复默认 frame。拖动继续使用 `window_drag`。
平台补充仅保留完整遮挡检测、窗口外的系统光标位置，以及 macOS 原生红绿灯显隐。
手机方向控制和文件导出仍由独立宿主完成，框架尚未提供等价接口。

- Mini：480×270，工作区右下16点，最小360×180。隐藏应用顶栏/侧栏/底导航，顶部44点拖动带独立于播放控件显隐。macOS原生红绿灯、Windows等价窗口按钮跟随播放器Chrome显隐；标题、集数、简介、seek和紧凑播放按钮保持原操作。
- Portrait：比例取 min(真实竖视频比,9/19.5)，普通高度至多工作区70%，最小360×400；保留44点顶栏，隐藏分类插槽/侧栏/底导航。铺满使用当前工作区，还原用户普通竖窗快照；换流比例变化超过1%才refit。
- Frameless：保留几何并隐藏壳UI；macOS只隐藏红绿灯。与系统全屏互斥。Mini/Portrait互斥，退出恢复原始Normal快照；置顶意图独立于形态。
- Incognito：光标出窗先暂停，至少80ms后隐藏，回到原矩形显示且不夺焦点。保留用户手动暂停；最小化或老板键隐藏时不自动抢回。窗口藏显统一经NSH。
- 全屏：使用系统过渡；从全屏切换其他形态，等待NSH窗口状态确认退出，再应用尺寸。确认超时明确报错。
- 手机横屏：系统确认旋转后更新模式；Android只隐藏导航条。退出/移除宿主恢复方向策略和系统栏；系统拒绝或未确认时明确报错。

框架窗口命令异步提交，实际最大化/全屏状态以NSH窗口事件为准。
命令effect只订阅请求，窗口事件只驱动状态采样，避免窗口状态变化重放切换命令。
窗口控制器释放内部借用后再发布事件；原生观察器只发布具名 `Observation`，
手机旋转请求与回执保留具名serde契约。

不可见暂停开启时，每250ms至多查询一次真实可见性；关闭时停止查询并解除临时门禁。
macOS先检查显示、最小化和系统遮挡状态，再扣除Window Server上方不透明窗口覆盖区域；
只读取窗口元数据，不截屏、不读取标题、不要求屏幕录制权限。
Windows检查显示状态、DWM cloak和屏幕区域，再扣除上方不透明窗口。
多个窗口共同盖满同样暂停，部分可见和单纯失焦继续播放；
透明、色键及无法确认全不透明的分层窗口不计作完全遮挡。
逐像素透明内容无法仅由公开元数据精确还原，仍采用应用窗口矩形覆盖语义。

原生暂停与用户暂停分离；恢复可见只重新允许播放，不重建视频或重置时间轴。
保留路由、老板键/隐身和来源身份的播放门禁。尺寸切换使用系统几何操作，
不对视频平面加入动画；紧凑控制条沿用Chrome渐隐、减少动效及共享按钮反馈。
置顶两态沿用空心/实心Pin资源和透明背景。

2026-11-15 独立Windows实例使用 `target/nsh-framework-native-20261115-1701` 数据目录：
真实流持续播放，小窗480×270，拖动从(2064,1106)到(1944,1146)，
最大化至2560×1400后还原480×270，退出小窗恢复1282×872普通窗口。
按钮与播放控件共同显隐，系统圆角截图已目检；全屏2560×1440后Esc恢复普通窗口，未重复切换。
实例已关闭。DPI切换、隐身恢复焦点、完整遮挡、macOS和移动端尚未运行验收。
此前自定义桥的实测记录不用于证明本次框架迁移通过。

Desktop geometry, sizing, window modes, levels and visibility now use NSH window APIs.
Native supplements only observe complete occlusion and the global cursor, or control macOS traffic lights.
Existing mini, portrait, frameless, privacy and playback behavior is retained by the shared controller.
Mobile orientation remains host-specific. Builds and static checks do not replace platform runtime acceptance.
