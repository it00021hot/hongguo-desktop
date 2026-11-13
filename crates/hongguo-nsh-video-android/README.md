# Android 像素播放器

系统播放只使用一个 `MediaPlayer`。默认输出到原画 `TextureView`，不创建 WGPU device；开启增强时，独立 Rust 工作线程建立 GLES device，将同一个播放器的输出切到 `SurfaceFrames` 的外部 OES Surface，经普通 RGBA 纹理和共享 `hongguo-enhancement::Pipeline` 呈现到另一个 `TextureView`。Android 宿主 Java 仅承载系统调用与主线程调度，业务、界面和增强状态保留在 Rust。

关键边界：

- source epoch、用户 seek request ID、GPU 输出 token 独立。Android 匿名 seek callback 串行匹配，排队保留最新请求；未确认的旧 seek 超时后不会错误确认新请求。
- 暂停切换输出时，平台不会自动重绘。通过同播放器、同位置的内部 seek 获取真实当前帧；不启动播放，不改变用户暂停或速率，不发用户 `Seeked`。实际解码帧与 GPU 提交完成后才显示增强输出。
- 设置/viewport 改动可复用上一张真实输入帧；提交中的单槽纹理直到 GPU 完成才可覆盖。完成消息携带真实提交设置，旧设置的完成不会成为新设置的 Ready。
- seek 完成、首次准备完成和音频焦点恢复从 Rust mailbox 读取当前 source 代际的最新控制意图，不能用渲染工作线程较早的快照恢复播放；真正结束后 seek 到开头也按当前暂停意图恢复。
- 横竖屏改变时同步 ANativeWindow buffer 几何，退休旧 EGL back buffer 后重呈现真实缓存帧。暂停状态也更新实际画面比例，不重建播放器。
- 关闭增强、失败或失活时，先让 Java 主线程确认该输出 token 已从播放器分离，再释放 SurfaceTexture/EGL；确认超时则暂存输出直到真实分离或播放器释放。释放播放器后才停止 Rust worker，工作线程保持 JVM attachment 到其全部 Java 引用释放。
- 外部 OES 采样前显式解除上一轮 WGPU sampler。WGPU 的 mipmap minification sampler 会使无 mip chain 的外部纹理不完整，造成“首帧有图、随后黑屏”；见 [Khronos 扩展规范第 7 项](https://registry.khronos.org/OpenGL/extensions/OES/OES_EGL_image_external_essl3.txt)。
- OES 转换先将 GL framebuffer 的行原点归一到 WGPU 图像原点，再应用 SurfaceTexture 变换。合成图时间码和角落颜色在暂停、播放、旋转及失活恢复后均有方向断言，防止仅尺寸正确但画面倒置。
- 未知系统时长通过 `durationMs: null` 表示，不制造 0 时长或虚构全长。HTTP/缓存、业务续播、下载和账号处理不在本 crate。

## 独立原生验证

先启动明确属于测试的 Android arm64 模拟器，然后在仓库根执行：

```sh
node scripts/nsh-android-probe.mjs --serial emulator-5580
```

需要项目既有 Android/JDK/NDK 环境与 ffmpeg。`--skip-build` 只用于已构建的当前宿主。脚本仅安装 `com.hongguo.nsh.pixelprobe`，不启动业务 App。测试 APK 没有网络权限，素材为本地生成的 30 秒 H.264 色彩图，不提交媒体文件。

脚本复用生产 Java、JNI 和 Rust 实现，强断言暂停位置保持、暂停 seek、暂停锐化/Anime4K 的真实非黑像素、播放中的 GPU 像素、增强中 seek、相同 MediaPlayer 引用退回原画、新 source 最新 seek、设置 10→90 的真实完成、真实横竖屏恢复、Contain/Cover 与旋转后的像素边界、失活再激活、EOF 续播与暂停重播、GPU 活跃时销毁。失败、超时、黑图、比例偏差、JNI detached 引用或未确认分离日志都会导致非零退出码。

结果位于 `target/nsh-pixel-android-probe/{summary.json,native.log,paused.png,anime.png,playing.png}`。实际通过环境为 Android 37 arm64 模拟器 / GLES 3.1；这不替代所有物理机、GPU 驱动与 NSH 业务完整界面的验收。
