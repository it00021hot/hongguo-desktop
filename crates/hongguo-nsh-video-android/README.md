# Android 像素播放器

视频由共享 Rust 解码会话提供不可变 BGRA8 帧，本 crate 上传到 GLES 的 RGBA 纹理，再经共享 `hongguo-enhancement::Pipeline` 呈现。原画与增强使用同一条 GPU 输出；关闭增强仍呈现同一份内置解码帧。Java 的 `MediaPlayer` 仅播放音轨、提供时钟，不绑定视频 Surface。

关键边界：

- 有 source 时必须提供 `frameSource` 租约地址。关闭消费者只关闭自身租约；代际变化撤销旧消费者后再应用新 source。
- 首帧实际 GPU 提交完成且音轨已 prepared 才放行声音。BGRA 转 RGBA 显式交换红蓝通道并跳过 stride padding。
- Android 匿名音频 seek callback 串行匹配，保留最新排队请求。音频完成后按实际位置重置 Rust 解码代际，只有新代 GPU 帧呈现后才发 `Seeked`，并恢复 mailbox 中最新暂停、音量与速率意图。
- 设置及 viewport 改动复用上一张不可变帧；GPU 完成前不覆盖上传纹理。旧设置完成不会成为新设置的 Ready。
- 增强失败后继续呈现原画；原画 GPU 路径失败时暂停音频并发出错误。失活后停止帧轮询，销毁前先关闭帧消费者和音频播放器，再停工作线程。
- 未知系统时长通过 `durationMs: null` 表示，不制造 0 时长或虚构全长。HTTP/缓存、业务续播、下载和账号处理不在本 crate。

## 定向验证

仓库根执行纯数据和架构回归：

```sh
cargo test -p hongguo-nsh-video-android --test pixel_bytes --test softdec_only
```

`tests/PixelDirectGateTest.java` 验证首帧、缓冲、seek 代际及关闭门控。Android 交叉 strict、Java 编译和真实业务 APK 的像素验收分别记录，不能把静态测试或编译通过称为设备运行通过。
