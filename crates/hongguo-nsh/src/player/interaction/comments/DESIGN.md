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
当前单列表滚动包含详情header、评分表单/摘要与稳定key行；内联展开回复按页加入父行，尚未扁平化为独立虚拟行。
