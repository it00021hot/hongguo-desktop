# 用户互动状态

点赞以vid为键，收藏/追更以series id为键，全部再加账号键；匿名不能提交写操作。
SessionHandle独占不透明SocialMemory，成功后保存乐观计数/状态并更新revision。
详情和播放器消费同一成功记忆，避免服务端索引延迟使返回详情后恢复成旧状态。
刷新应用记忆时保留本视图正在提交的字段；错误只回滚当前操作，账号切换不跨用记忆。
本轮记忆仅会话内有效，不额外创建持久化状态源。

评论/剧评见[comments/DESIGN.md](comments/DESIGN.md)，表情/图片见[../../emoji/DESIGN.md](../../emoji/DESIGN.md)。
这类写操作用fake/纯状态测试，不自动提交真实账号数据。
