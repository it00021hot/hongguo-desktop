# 评论与剧评

单集评论与剧评共享 Feed、完整发送回显、回复行、表情编辑器和正文解析；领域入口必须区分。

剧评来源为 `src/pages/series/components/review-{list,composer}.tsx`：

- 剧评首屏评分取 `credibility_score` / `credibility_score_count`，并显示 `filter_tag` 统计。
- 五星选择为1–5，未选不可发表；请求评分为星级×2的十分制，正文非空，最大500。
- 剧评发送/回复只支持文本及系统表情，公开API没有附图参数，不借用单集附件协议。
- 剧评本条赞使用 `digg_review`，回复赞仍为 `digg_comment`；删除本条service2，回复service4。
- 展开回复按需请求 `fetch_review_replies`；新增回复保留完整reply_id及reply_to身份。
- 每条评分按十分制÷2展示，保留半星、评分后缀与日期；不能拿书本评分字段代替摘要。
- 剧评Composer是Card60%/border/p12，摘要border/p16x12；列表头像36、行py16、gap12。

动效复用ui按钮的hover/press/focus、评分悬浮预览和持久选中；发送只在pending禁重复。
失败保留正文与所选星级，并允许重试。切换账号后不能延续旧账号的乐观互动状态。
离线预览使用相同展示组件与固定数据；不把fixture模拟发送算作真实账号写入验收。

五星的五个Radio在具名ReviewOptions子组件挂载后构造，保证原生组件注册边界内仍继承RadioGroup上下文。
不能在调用方提前构造Radio：Headless可能可用，但原生会缺失focus stop与箭头Keymap。
原生深色回归验证点击2星→Right→3星→完整6分回显，非仅验证已选样式；明暗鼠标全流程均已验。
当前单列表滚动包含详情header、评分表单/摘要与稳定key行。评论、回复及回复分页状态扁平为root/reply/footer三类entry；回复key同时包含parent ID与reply ID，不再使用固定360px的内嵌列表，也不累积Each渲染整页回复。

## 桌面评论对齐与交互修复（2026-11-13）

播放器评论对照`src/features/player/components/comment-panel.tsx`与`src/components/common/reply-thread.tsx`：

- 桌面为舞台右0/bottom56、宽380/max85%、高舞台68%的浮层；header48，左右16，底部输入区p12。使用公开overlay锚到PlayerGeometry.comment_anchor，不扩大互动rail布局、不创建全屏遮罩；关闭按钮和Esc返回真实Comments触发器。
- 评论作者12px正常字重、正文14/19.25、meta11；头像32/gap10，右侧心形16与10px计数。回复作者11/16.5、正文12/16.5、meta10、心形12/count9，沿2px左边线缩进10；剧评回复缩进12。
- 剧评本条仍为头像36、name14medium、正文14/22.75、meta12；行py16，展开回复组后的divider与正文同宽。展开/收起按钮按内容宽度靠左。
- 评论主输入为同排36px编辑器/图片/表情/发送，附件64px预览在上；剧评为64px多行编辑器与居中表情/发送。保留真正RichEditor原子表情、图片服务及完整API回显，不增加假输入覆盖层。
- 评论分页对齐Tauri的outline/sm：按内容宽度居中、32px高、左右12px、上下外距12px；标签复用`player.comments.loadMore`显示剩余条数。请求时按钮禁用并显示14px加载图标，结束后由真实has_more决定是否保留。预览复用同一按钮，末页点击仅为离线交互。
- 手机保留系统Sheet；实际动作边界至少48×48，正文16/24，浅色Sheet使用主题前景，桌面播放器面板使用固定暗色皮肤。

真实故障与修复：

- 原Like匿名直接disabled，无法反馈；现在仍可操作并显示登录提示，发送端同样允许匿名编辑、在提交时反馈登录要求。
- 原row事件使用Stored.with包住回调，更新展开状态时同步重建又借用同一Stored，实际点击触发RefCell already borrowed。事件保存为Rc，get克隆后释放Stored借用再执行，避免重入借用。
- “回复”编辑与“展开/收起”分开；零回复不建展开入口。加载资源按真实parent选择Replies/ReviewReplies端点，回复发送保留reply_to_reply_id；成功增加父count，失败保留编辑内容并显示错误。
- Reply resource、展开状态及按(account,id)保存的Like pending/error属于整个thread owner。Like异步任务不随虚拟行回收取消，旧账号结果不回写新账号；只回滚点赞字段，不覆盖期间新增的回复count。

验证：`target/nsh-commit-20261113/comments-click-red.log`保留匿名disabled/零回复两个行为红；`comments-thread-runtime-red.log`保留点击回复的借用重入红栈。`comments-complete-tests.log`17/17绿，含正确parent展开/收起、点赞乐观与失败回滚、回收期间异步返回、评分焦点/完整回显及原有列表回收。

`LayoutPreview`的mod+shift+c为独立数据评论入口；详情剧评沿原tab。两者共用生产Threads、Actions、虚拟行和输入呈现，只有读取/写入回调用固定离线数据。mod+shift+0用于空态。原生首轮保留选择器/截图目录失败，修正后`comments-native-recheck-results.xml`4/4通过，覆盖明暗Like/展开/发送回复/count/附件/关闭返回及剧评评分与回收；该证据属于原生UI和离线控制器验收，不是账号云端写入实测。

Tauri四张基线由生产React组件导入，位于`target/tauri-comment-reference/`。最终leading spine/输入居中/空态补图为`comments-visual-final-results.xml`3/4，唯一失败为深色首例未进入预览快捷键；通过真实倍速按钮打开、Esc返回建立本实例焦点后，仅深色评论补验`comments-visual-final-dark-recheck-results.xml`1/1通过（13.7秒）。四个受影响case以最新结果均通过，已目检明暗评论initial/expanded/attachment/empty及剧评展开/发送/空态图，无生命周期或借用错误。保留中间失败日志，不将它们改写为成功。正文与内联文本明确使用400，深色原生与WebView仍有字形平滑观感差异，不用降低字重补偿。

交付定向复查另保留`mobile-comments-delivery-targeted-tests.log`的16/17红：旧测试把`Screen.texts()`树顺序当成视口首行。SDK对此类scroll使用开放的内容clip，已滚到后段时仍可能报告停放cell的旧文本；等待2秒也不消失，不是滚位被恢复。测试现通过仅`cfg(test)`接入的公开`on_viewable_items_change`断言真实first>10，对该可见ID点赞12→13，再滚回first0并验证原首条仍为13。生产布局与行为未改，诊断打印已移除。最终`comments-delivery-corrected-tests.log`17/17绿（19.15秒），替代旧文本顺序断言作为回收证据。
