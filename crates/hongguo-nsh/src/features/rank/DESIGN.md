# 排行榜响应式子榜导航

以 Tauri `rank-page.tsx` 为准：768px 以下为横向不换行的子榜导航，底部留4px，
与结果区间距8px；宽屏为144px独立纵向滚动栏，间距16px。
按钮使用共享表面、语义主题和可见焦点，桌面高度36px，手机触控目标至少48px。
滚动区域必须限制在实际视口；切换布局保留查询选择，仅重新建立对应方向的滚动区域。

| 状态变化              | 动效与工具                                                                                                      | 验证                                                                                      |
| --------------------- | --------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| hover、pressed、focus | 共享表面令牌的过渡、按压缩放和焦点描边；遵守 reduce_motion                                                      | 明暗原生hover/pressed通过，02实际焦点ring已目检                                           |
| 子榜选择              | 共享 pressed 主题与表面过渡，立即更新结果标题                                                                   | Headless与原生首/末点击及真实标题回显通过                                                 |
| 窄屏横滚、宽屏纵滚    | 原生 Scroll；由实际内容决定边界，不模拟滚动偏移                                                                 | Headless三平台及macOS 393/1280实际首末可达通过                                            |
| 跨断点布局            | computed 断点仅在横/纵模式变化时重建，外置查询身份保持                                                          | Headless及原生双向通过；按轴key防原生复用残留偏移                                         |
| 分组键盘              | 公开 ToggleGroup/Toggle，整组一个Tab停点；方向键/Home/End只移焦点，Enter/Space才确认，首末循环                  | 真实 Radix 1.2.1 依赖已核；Headless 键盘红→绿，含Tab离组/ShiftTab返回                     |
| 可见键盘焦点          | 普通子项 wrapper 的 ElementRef.on_focus_in 读取实际 on_layout 边界；必要时 scroll_to 最小可视范围，不重发 focus | 原生离屏红例已修；Headless两轴End/Home ScrollTo通过，明暗实际02可见、有焦点ring           |
| 主榜返回滚位          | 真实 virtual_list 复用窗口 remember_scroll；作用域为tab/sub/panel/background/identity，排除分页和结果数据       | Headless回页400恢复/换子榜归0红→绿；原生Dark实际回页后点初始视口外第7项并进入该条详情通过 |

headless 行为测试不代替实际文字、触控尺寸及明暗布局验收。

2026-11-13 定向原生验收使用 LayoutPreview 中的 `mod+shift+u`，
仅替换预览 RankTab 的24个 RankSubList，保留同生产导航、查询选择与结果标题。
默认固定数据与原 `mod+shift+r` 的长筛选面板预览不变；不访问 API 或个人配置。
RankSubLists.test.tsx 的两主题用例覆盖1280纵滚首末、393横滚首末及返回1280，
用真实 header 回显和选中文本颜色证明选择保留，并保留首/末截图供目检。
窄 macOS 窗口只证明共享响应式分支，不代表 Android/iOS 真机验收。
选择状态使用 Toggle 的实际 `aria-pressed`，不是普通 Button 的 selected；
对应状态断言检查 pressed=True，并保留查询标题、草稿、Back/Forward与跨断点断言。
原先普通 Button 列表的 Tab→Right 没有移动到第2项，行为红在
`target/nsh-followup-20261113/rank-sublists-keyboard-red.log`；
键盘、移动方向/首末、跨断点及生产 Back/Forward 共4项绿在 `rank-sublists-keyboard-green.log`。
原生键盘先打开真实月榜、Escape关闭后ShiftTab进入子榜组，不能以鼠标点击假设控件已取得键盘焦点。
视口变化后按原生布局完成同步，再实际滚动/点击末项；不会用语义树存在代替可见、可点击。
可见范围滚动保持 Toggle 内部 Roving 与 ElementRef；外层 wrapper 只观察子树焦点，
不设置额外 Tab 停点，也不更改用户手动滚动时的偏移。

原生 resize 红例的根因为 Scroll 在两轴间复用了同一原生实例，保留的纵向偏移使整个横栏移到视口外，
不是等候布局时间不够：fresh393用例实际通过，横Scrollbar也能移动但文本仍空。
`rank-sublists-bounds-native.log` 的4117/6481行记录同 id623 从 vertical 切为 horizontal；
NSH macOS scroll::config 改方向后沿用 x/y，因此应用按轴设置不同 key，让方向切换真正重建 Scroll。
外层 Query 的选择不重建；该机制不改 SDK，不定时重试清偏移。
加入轴key后的定向5/5见 `rank-sublists-axis-key-green.log`。

最终同一LayoutPreview host的 `view memory finishing` 五例5/5（2m44s）通过，
包含Browse明暗两例、fresh393 Dark和Rank1280→393→1280明暗两例；
日志/JUnit为 `target/nsh-followup-20261113/view-memory-finishing-native{.log,-results.xml}`。
Rank全部首/末/滚前后、明暗Focus02及主榜before/return图已目检；
原生主榜恢复后真实点击第7条并校验详情身份，不用静态标题代替可点击验证。
无 binding、owner-disposed 或 panic；进入固定数据即将上线详情时的离线无分集错误是预览场景，非生产API失败。
这一轮缺失旧 player-frame 文件导致封面灰占位，因此只对导航/滚动/焦点及主体几何给出通过结论；
完整封面外观在恢复ignored帧后另做同用例截图复验，不改生产UI。

恢复ignored测试帧后，同五例再次5/5（1m39s）通过，无源码重编，日志/JUnit为
`view-memory-visual-native.log` / `view-memory-visual-native-results.xml`。
有图Rank窄首/末明暗、桌面首/末、02可见焦点ring、主榜400 before/return均已目检；
封面已实际加载，前后主榜内容在同一滚位；没有 binding、owner-disposed、panic 或 image-failed。
这一轮完善同尺寸布局外观证据，不代表真实账号写操作或Windows/移动平台运行通过。
