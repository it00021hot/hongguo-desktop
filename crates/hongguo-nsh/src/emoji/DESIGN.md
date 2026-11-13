# 表情与评论附件

原规格来自 `src/components/common/emoji/`、`utils/danmaku-emoji.ts` 与接口文档 9.6。

- 系统档默认打开，本地码表/原图与 Tauri 同源，8列、24点图、最大208点滚动区。
- 评论可选搜索/系统/我的/GIF；弹幕与回复只显示系统档，不猜回复附件协议。
- GIF 6列，100条分页；搜索非空才请求，15条且前缀匹配；我的49条，上传和收藏同列表。
- 选择后不关面板，插入记录的 UTF-16 选区并恢复输入焦点；无选区时追加末尾。
- 系统码发送仍是文本，正文/弹幕解析为真正 inline Image；未知码/空白/超长码原样保留。
- 远端评论附图保留完整 Emoticon 与 scene2/4；本地图只在发表时上传，移除/失败不丢草稿。
- 收藏用静态 web_uri，删除二次确认，重复收藏/空回显明确报错；所有写入需登录。
- 输入使用 NSH 0.4 公开 Rust RichEditor：系统表情是 inline image atom，提及是独立 atom；同一 draft owner 保留原始码与 UTF-16 选区，提交不改协议。
- schema 保留 hard_break，粘贴/外部草稿的连续换行不丢失；未知码保留文本。非 IME 的 Enter（含 Shift+Enter）与 Tauri 一样提交；组合输入期间由原生 editor controller 阻止提前发送。
- 字段销毁只通过独立 binding 失效，不在 cleanup 或晚到原生回调中读取已销毁的 draft。最大长度按发送文本 UTF-16 计算，不能用一张 atom 的 document size 冒充码长。
- 单行字段用公开的 max-content/whitespace-nowrap 与真实横向 Scroll；通过公开 scroll-to-selection 插件和 session caret 几何保持光标可见。Review 保持多行。两者都不以裁掉正文或覆盖层替代编辑。
- RichEditor 0.4 会拦截 Tab；应用插件显式把非 IME 的 Tab/Shift+Tab 转给调用处提供的真实相邻 ElementRef，评分、表情按钮等仍是原控件焦点，不使用 SDK 私有遍历。

状态/动效：按钮复用ui hover/press/focus；Popover从trigger上方短缩放淡入/退出，消费FAST_MS与PRESS_SCALE；
reduced-motion下即时。Tab选中持续可见；请求/上传期间真实loading、禁重复提交；结果与错误就地展示。
系统格不播放入场波浪，避免连续选取和滚动回收反复动画；附件加入/移除直接更新内容，无额外布局动画。
键盘Tab/Enter可选择，Escape关闭并恢复输入；收藏菜单按NSH公开组件在桌面右键、手机长按开启。
移动真机选图/输入法尚未验收，不把共享内核或宿主编译通过当成真机通过。

验收按组保存：目录/解析/选区回归，分页与附件payload回归，Headless交互；原生明暗/窄窗预览共用生产组件，
不以静态图替代真实发送，也不自动对真实账号执行上传/收藏/删除/发送。

成功收藏/上传保留完整对象回显；SessionHandle单一CollectionMemory按account隔离，评论图片与所有编辑器共享。
索引未更新时仍可立即选择回显，服务端分页确认后去重，删除覆盖不会被旧索引复活。
只有服务端原始分页能确认已收录，不能把UI自身叠加的回显视为索引确认；空回显用双语就地错误。

已验：14项解析/选区/分页/payload/Headless交互；image-only评论另1项先红后绿；
LayoutPreview同生产picker/sendbox原生深浅两项通过，4图已目检（system/attachment × dark/light）。
固定fake覆盖选码不关popup、搜索→收藏→我的→附件、失败保留、图文与纯图发送，不执行远端写入。

RichEditor 核心定向回归 12 项已通过，覆盖真实挂载后的 atom 删除/插入、原码发送、中文 composition gate、提及、最大长度、换行粘贴与 draft owner 销毁重建；40 组中文/表情的首尾 caret 同行，End 发正 X/零 Y 的真实 ScrollTo，Home 回零。调用处原 Review 键盘评分→发送、Body Tab 邻接回归已通过。

原生两主题用例 preview/layout/RichEditor.test.tsx 已 2/2 通过（58.7 秒），六图已亲自目检：行内图本体、单键删除/选择插入后原码提交，120 UTF-16 单位的中文/表情长稿 End/Home 真实 caret 位移与再次提交。未对真实账号执行写入；CompositionChange 测试不等同于各移动系统输入法真机验收。

macOS 横向 viewport 在操作后仍显示一条原生滚动条。NSH 0.4 的 macOS scroll::config 明确使用 `indicator || (horizontal && !replaced)`，即使局部 `scrollbar(false)`，仍为没有横滚轮的鼠标保留操作条。保留该公开后端行为，不修改 SDK 或全局滚动条配置来掩盖差异。
