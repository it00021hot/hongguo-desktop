# 菜单交互验收

2026-11-13 收尾：完整 NSH 库299/299通过。手机底部弹层移除重复的顶部安全区，
Android 状态栏文字随明暗主题变化；真实 Android/iOS 验证包的明暗菜单与关闭流程通过。
本次独立记录在 `target/nsh-followup-20261113/`，包含 `library-tests.log`、
`android-after.xml`、`ios-sheet-verified.xml` 和两端明暗截图；此前日期的结果仍按历史批次理解。

本轮以 Tauri 真实组件和 NSH 生产 presenter 为准；LayoutPreview 只注入固定数据，不读用户配置或调用账号写接口。原生截图、日志与 JUnit 均留在仓库根 target/nsh-preview，不提交影视帧。

| 菜单            | 核验动作                                                                      | 证据                                                                                              |
| --------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| 全局语言        | Escape、Enter 重新打开、英文/中文切换，关闭后入口仍可操作                     | MenuAudit；menu-audit-select-fixed-results.xml 两主题通过                                         |
| 全局主题        | 实际切换主题后侧栏文字颜色变化，弹层关闭                                      | 同上；不是只检测选项存在                                                                          |
| 设置下拉        | End 后 Escape 保持原值、焦点返回、Enter 重开、End/Enter 保存、重复按入口开关  | 同上；虚拟化后再由 menu-audit-finishing-results.xml 的 global dark 通过                           |
| 合并候选        | 注入真实自定义 trigger 皮肤，Home/End/Enter 分别改变 merge-output             | finishing 的 global dark，同生产 SelectControl                                                    |
| 排行榜题材/月榜 | 选项关闭、独立筛选保留、Escape/Enter 返回、换路由清旧弹层                     | menu-audit-results.xml 的 Rank 两主题通过                                                         |
| 排行榜长列表    | 18 组题材、108 个月份；真实滚动前后截图、末项实际点击、标题回显并关闭         | menu-audit-pending-results.xml 两主题通过                                                         |
| 找剧联想        | Escape 保留稿件、End/逐字 Backspace 真编辑、联想重新出现、方向键与 Enter 提交 | pending 两主题通过                                                                                |
| 新剧日期/频道   | 切空日期、切回有数据日期、切女频后的实际选中颜色                              | pending 两主题通过                                                                                |
| 历史/下载批量   | 管理选择、取消、Escape、确认后的固定内存数据变更                              | 已有 LibraryActions.test.tsx；本轮未重复运行这些已验场景                                          |
| 播放器各菜单    | 倍速、画质、选集/季、音量、弹幕、紧凑 More 的真实意图、关闭与焦点             | 已有 PlayerMenus / MobilePlayerMore / Danmaku 用例；增强/窗口为后续本轮装配，由最终受影响矩阵验证 |

本轮修复三处产品问题：

1. macOS 的 NSH 0.4 先把 Enter 交给有焦点的 Press 控件。原 BaseSelect 保留入口焦点，只有高亮变化，导致 Enter 关闭而不提交。应用层 SelectControl 使用公开 Popover/Sheet 和真实可聚焦 Option，保留 combobox/listbox/option 与受控值；候选保留虚拟列表。定向回归由未派 Focus 的真实失败转为 3/3 通过，包含 1000 个候选的 End/Home 回收身份。
2. 排行榜只限制弹层祖先高度，Scroll 仍按内容高度展开，造成溢出且没有实际原生滚动视口。高度约束现落在真正 Scroll owner，减去共享面板的 padding/border，短内容保持自然高度。红图另存 menu-audit-long-total-red-{dark,light}.png；绿图可见滚动条到末端，末项在弹层边界内。Tauri 默认 collisionPadding 为 0，已移除 NSH 自加的 8 点边距，右边界与源一致。
3. 正式 App 提交找剧搜索时，旧海报 Grid 的函数作用域先销毁，虚拟单元仍读取其中的 Memo。`navigation-lifetime-native.log` 的真实堆栈定位到 `ui/browse.rs:184`，19 次失败均发生在提交搜索之后。Browse 与按频道重建的新剧 Grid 现各有具名组件作用域，内部状态随真实列表卸载；保留原虚拟化、数据与几何。相同正式流程修后实际退出0、1/1通过（用例27.083秒），完整 trace 的 disposed/panic/binding error 均为0；见 `target/nsh-playback/navigation-lifetime-fixed-native.log` 与 `navigation-lifetime-fixed-results.xml`。流程含初始真实播放、搜索、详情、返回保留稿件、推荐恢复实际推进和视频画面暂停。Headless 的旧实现未复现此问题，不能把其基线绿作为红证据；新增两 Grid 清空/重填、页面销毁，以及原生 prop/opacity 源替换回归均显式收集各自引擎线程的错误日志。

测试输入边界也已纠正：App.type 公开契约是追加；当前原生 clear 的 SelectAll 未删除完整中文稿件。回归采用用户可执行的逐字编辑，并保留空串及重输入结果断言。没有改产品查询、数值或等待时间来掩盖失败；原始失败日志/JUnit 保留。

当前最近 4 项为 4/4 原生通过（1 分 46 秒）；全局/设置两主题和虚拟化后合并自定义触发器有独立通过记录。不存在的测试方法 App.selected 已移除，使用公开 spanStyle 读取实际选择后的前景变化。Windows 导航/窗口真实交叉 all-targets 检查由同组执行通过，不能推成 Windows 实机菜单验收；macOS 系统滚轮仍需解锁后的独立真实事件验证。

2026-11-12 收口：完整 NSH 库回归为 296/296（本轮早期 261），共享严格检查通过；见 target/nsh-preview/final-footer-loading-library-tests.log。手动切季的真实异步资源回归已验证换源后立即开始新请求、旧完成不清新 busy；自动接力另由正式 App Relay 1/1 验证实际末集结束后新剧时间2→4秒，证据在 target/nsh-playback/relay-live.log 与 relay-results.xml。这些记录不把未运行的物理滚轮、Windows设备或账号写操作记为已通过。

移动系统底栏的空白图标另已定位：应用只向 TabBarItem 传标题，五个原生 item 的 icon 全为 None；Android More 中的描边 SVG 实际正常，不能归因为通用 SVG 绘制故障。现保留系统 TabBar，iOS 配置 SF Symbol，Android 配置同源 Lucide VectorDrawable 资源并保留原生选中着色。真实 native items 属性回归先红后绿，两个平台均覆盖（mobile-tab-icons-{red,green}.log）；Android 宿主资源经 aapt2 编译通过。图像实际可见性由带此批资源的最终移动验证包截图另验，不能仅由属性非空推断。

手机页面可达性另有明确补齐：Tauri 自身的侧栏与折叠按钮在窄屏均隐藏，没有现成手机侧栏 Sheet。NSH 使用公开系统 header_right 的“主菜单”按钮和 Shell 单一 Sheet，复用已有十三个页面入口，保留五个系统 Tab。主菜单真 Scroll 能进入新剧、收藏、点赞、追更、预约、任务、合并与存储；非 Tab 页不误高亮首页。两平台 Headless 从缺少入口的真实红例变为通过，覆盖菜单 Back/Escape 优先关闭、实际 opener Focus 命令、八页逐个进入/返回及十三项完整性（mobile-main-menu-final-green.log）。手机 Headless 不模拟非输入控件的硬件 Tab/Enter 激活，不据此虚报手机键盘实测。

主菜单固定页脚复用共享 Select 的移动底部选择 Sheet，提供既有主题与语言控制。选项列表、语言失败提示与桌面同源，主题调用壳层原保存回调，未增加持久化。mobile-menu-preferences-{red,green}.log 记录两平台从缺少入口到实际 Light 配色状态变化、忙时禁用、重开值保持、中文/English 文案双向更新、子面板返回不关闭父菜单，以及关闭后 Header Focus 命令的回归。实体手机图像、原生按钮与实际保存结果由最终移动包另验。
