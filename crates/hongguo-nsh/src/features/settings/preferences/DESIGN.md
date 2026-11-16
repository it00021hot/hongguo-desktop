# 设置页交互清单 / Settings interactions

设置项修改即应用并持久化，不保留保存按钮、集中提交动作或成功保存提示。
账户、目录、命名与并发、代理、播放、快捷键、老板键、存储和更新保留原有顺序，
使用现有共享Input、Select、Slider和Switch。账户仍进入同一受控登录弹窗。

输入模型只承载控件当前值，不把整页内容写回设置。每次变化只提交改变的字段，
执行时从最新设置合并，保留同时发生的登录、外观及其他设置变化。
更新由应用owner持有并串行执行，页面退出不取消已经开始的应用与持久化；
页面销毁后不再写它的Signal。连续修改不因上一次请求而禁用输入或丢失后续值。

- Select、Switch、Slider：变化后立即应用，不额外添加成功提示或动效。
- 文本与数值：随输入更新。非法数值不持久化，保留输入并显示范围错误；不阻塞其他字段更新。
- 目录：桌面输入或系统面板选择后立即应用，手机继续使用私有目录；新缓存写入新目录，旧缓存不自动移动。
- 代理：模式和地址改变后立即持久化，测试代理仍是独立的显式动作。
- 播放：自动下一集、播放后删除、窗口不可见暂停即时更新；恢复可见不改写用户手动暂停。
- 老板键：录制或Backspace清空后立即调用NSH注册/注销；应用内冲突明确提示未应用。
  先注册新组合，成功后注销旧组合；占用失败保留原绑定。数据库失败回滚系统绑定，回滚失败有日志和可见提示。
- 播放快捷键、主题、语言、检测、缓存清理、更新检查和账户操作保留各自现有即时流程。

持久化失败不更新内存、下载调度器或缓存根。失败时只恢复尚未被再次编辑的受影响字段，
不覆盖用户随后输入；恢复控件不再次触发应用请求。失败反馈保留在原反馈区。

主题与语言仍归顶栏/手机菜单；表单不持有或写回外观。手机不挂载桌面键盘卡片。
保留表单滚动、焦点、键盘inset和原生目录面板；测试与目录面板的忙碌状态独立于即时更新。

动效沿共享令牌：Switch track/thumb、Select trigger、hover/pressed/focus使用FAST_MS与减少动效策略；
反馈文字即时替换，不添加保存中遮罩、不重建表单或移动输入焦点。
圆角、字体、间距、颜色、控件身份及可访问名称保持现有设计，移除底部保存按钮占位。

历史保存按钮与集中草稿的预览/测试记录不代表本次即时应用流程通过。
构建、静态检查和对应平台运行验收分别记录；不恢复已删除的测试、预览或探针入口。

2026-11-15 Windows独立实例（`target/nsh-settings-immediate-20261115`）确认：
并发3改7和自动下一集开关无需保存；重启仍为7，先前设置的缓存周期30天保留，
非法上限9999999未写入（重启为0）。非法输入时周期仍可独立改7天，范围错误提示保留；
上限改1024后提示清空。页尾截图确认无保存按钮，既有控件与布局保持。
NSH检查、Windows全工作区构建、严格Clippy及格式检查通过。
老板键系统占用/数据库故障回滚、明亮主题、减少动效、读屏和其他平台未运行验收。

提交前重启补验：并发7、缓存上限1024及周期7天正确保留。
录制Ctrl+Alt+F10后无需保存即可隐藏窗口，在另一独立数据目录的自建实例前台按同组合可恢复；
录制时Backspace清空后，原组合不再隐藏窗口。临时前台实例已关闭，未操作用户现有实例或账号。

Settings apply and persist on change. Only changed fields merge into the latest settings, and application-owned
updates run sequentially without disabling text input. Invalid numbers remain editable with visible errors.
Global shortcut registration preserves the previous binding on failure and rolls back if persistence fails.
Database writes complete before runtime settings change. Failed updates restore only fields the user has not
edited again. Existing focus, controls, motion and mobile restrictions remain; no Save button or bulk submission remains.
The isolated Windows run verified persistence across restart, independent valid edits alongside invalid numeric input,
validation feedback clearing after correction, and removal of the Save button. Other platforms and failure injection remain unverified.
Pre-commit runtime checks also confirmed immediate global shortcut registration, hide/restore from a separate
foreground instance, and unregistration with Backspace. Restart retained the valid concurrency, cache limit and retention values.
