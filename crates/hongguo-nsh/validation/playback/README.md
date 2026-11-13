# 正式播放验收

该目录运行正式 `App`，请求红果接口并解码真实视频。它与 `preview/layout` 的固定数据预览独立，
不加入默认离线预览命令。请使用专用数据目录，避免读写日常账号与观看记录。

在仓库根执行：

```sh
mkdir -p target/nsh-playback/profile
CARGO_TARGET_DIR="$PWD/target" HONGGUO_DATA_DIR="$PWD/target/nsh-playback/profile" \
  nsh test crates/hongguo-nsh/validation/playback --target macos \
  --filter 'production playback starts' \
  --junit target/nsh-playback/results.xml
```

测试从推荐起播，检查实际时间推进、暂停、实际倍速比例、下一集、历史续播、
手选画质与切回自动后继续走时，以及暂停后的进度条双向拖动。`Seek.test.tsx` 可单独复验拖动；
`Progressive.test.tsx` 经真实找剧入口重验曾失败的来源，`FutureSeek.test.tsx` 检查同集大幅跳转。
渐进冷启动用例须单独指定尚不存在的数据目录；它通过设置页关闭新配置默认开启的后台暂停，
断言首次真实进度在 2–4 秒，再检查持续推进。不能复用已配置或已缓存的 profile 代替冷启动。

```sh
test ! -e target/nsh-playback/cold-profile && \
  CARGO_TARGET_DIR="$PWD/target" HONGGUO_DATA_DIR="$PWD/target/nsh-playback/cold-profile" \
  nsh test crates/hongguo-nsh/validation/playback --target macos \
  --filter 'production progressive playback' \
  --junit target/nsh-playback/cold-results.xml
```

慢转码时暂停等待未来片段的行为另由可控生产者回归覆盖；真实转码在请求前完成时，
不能把大幅跳转通过算作等待新片段已验证。音量检查静音状态回流，不测量扬声器输出。
原生报错立即失败，保留失败截图；测试结束打印 `PLAYBACK_VERIFICATION_JSON` 供运行记录存档。
不要把跳过、按钮响应、静帧截图或合成媒体探针单独视为正式播放通过。

`Relay.test.tsx` 单独验收剧终接力：专用 profile 开启自动下一集，首页选择真实末集，
暂停稳定后拖到片尾前三秒，等待真实结束；必须看到新剧名、新集号，并在同一新源继续推进至少两秒。
下一季与首页下一部均合规；仅出现新标题或保留旧时间不算通过。它的日志与截图使用独立 relay 名称。

`NavigationLifetime.test.tsx` 用正式 App 验收推荐播放→找剧搜索→详情→返回保留搜索稿件→推荐恢复。
初始与返回均要求真实时钟推进，结束前检查视频表面暂停。排查生命周期时使用 `-v --trace`，
并检查整个日志没有 `owner was disposed`、`panicked` 或 `binding failed`；仅用例退出0不足以排除
框架记录的孤立绑定错误。红色原始 trace 与修复后的日志须分别保存。

原生视频层不包含在 NSH 的界面截图中，黑色视频区域不能用于判断解码结果；实际系统窗口
画面与原生时间推进分别留证。锁屏或窗口被系统遮蔽时，默认的最小化暂停设置会生效；本轮
后台验收仅在上述专用数据目录关闭了该设置，未修改日常配置，也不能据此报告生命周期验收通过。

此入口使用仓库根 `target`，不要复用 `preview/target`：不同 Rust root 的生成宿主名称相同，
共享输出目录可能误用另一个入口的缓存宿主。影视缓存与截图只写入忽略的 `target/`，不随源码分发。
