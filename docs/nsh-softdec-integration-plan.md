# NSH 接入 PR #23 的内置解码路线

## 目标

NSH 与 PR #23 统一使用内置视频解码，不保留兼容播放路径。
共享业务内核、解码会话与帧契约；Windows/macOS/Android/iOS 分别呈现同一 BGRA 输出，
系统媒体负责音频和时钟。Linux/Web 后续迭代，应用内更新安装本轮不实施。

## 实现边界

- 复用 `hongguo-softdec::IndexedVideo`：HEVC/ByteVC1 用 rusty_h265，ByteVC2 用 VVdeC，直接产生 I420 帧。
- 不编码成 H.264、不启动转码生产者、不生成 `.play.mp4`、不通过 HLS/grow 等待转码产物。
- 继续使用原始明文 MP4 的本地/在线 Range 来源，索引和样本按需读取，不等待整集下载。
- 保留现有 NSH UI、四端像素呈现和增强管线，不改 NSH SDK，不引入 JS 启动层。
- 系统媒体对象按 PR 的音轨视图机制承担音频、播放时钟、音量、倍速和 seek；视频由内置解码结果上屏。
- 用户已确认严格按 PR 当前支持范围：HEVC/ByteVC1/ByteVC2 内置解码；本地 H.264 沿用 PR 的系统播放处理，不新增 H.264 解核。当前索引要求 AAC，不得把现有能力写成已支持其它音频编码。
- 应用内更新安装按用户要求延期。

## 共享逻辑与归属

### hongguo-softdec

新增无 UI 的 `DirectSession`，四端共用：

- 原始数据读取接口、异步索引准备、首帧尺寸；索引遮罩视频轨，保留 AAC 音轨和原绝对偏移。
- 一个受控解码工作线程，seek 更新代次并取消旧读取，合并连续跳转，不无限创建线程。
- 解码产生 I420，在后台按原色彩元数据统一转换为 BGRA，放入有界像素帧队列；按外部音频时钟取到期帧。暂停、起播和 seek 的首帧不能依赖先让声音跑起来。
- 显式源关闭、读取取消、解码错误、旧代次丢弃；解码 EOF 不直接当成音频播放结束。

### hongguo-nsh-video-api

增加与实现无关的帧来源契约和进程内注册句柄：元数据、源/seek 代次、带 PTS 的 BGRA 帧、读取/seek/呈现确认。
该层不持有 NSH UI 或平台 GPU 对象，不复制解码线程逻辑。注册由媒体租约持有，撤销后旧平台回调不能继续控制或读取。

固定边界为 `frames::{FrameInfo, PixelFrame, FrameSource, FrameRegistration, FrameLease}`：
`PixelFrame` 是有 serial/generation/PTS/尺寸/stride 的不可变 BGRA8 字节；
`FrameSource` 提供 `info/frame_at/seek/presented/holding/close`。
`register_frames/open_frames` 只接受本进程注册 token；registration 撤销会关闭会话，
单个 consumer lease 关闭只禁用该 consumer，不抢占另一个仍有效的消费者。
`Source.frame_source: Option<String>`（wire 为 `frameSource`）携带句柄；存在时 `url` 必须是音轨视图。

### hongguo-nsh 媒体装配

将本地文件和在线明文流适配给共享会话，并注册帧来源。
音轨 URL 继续由有 token/Host/Origin/Range 边界的现有本机供数器提供，对每个区间调用 `mask_video_tracks`。
帧色彩通过共享 `FrameColor/copy_to_bgra` 转换，避免各平台重新猜矩阵和范围。
共用播放服务固定为内置播放，无需壳声明或运行时选择；播放器消费 `PlayResponse.direct` 和 `error_kind`。

## 各平台

| 平台 | 保留 | 改动 |
| --- | --- | --- |
| macOS/iOS | AVPlayer 音频时钟、Metal 画布及增强 | 不再依赖 AVPlayer 视频帧；上传共享软解像素，原画与增强都走自有画布 |
| Android | MediaPlayer 音频/时钟、TextureView/GLES、音频焦点 | 不把原视频交给 MediaPlayer；GLES 上传 Rust 软解帧，JNI 保持代次与帧生命周期 |
| Windows | MediaEngine 音频/时钟、D3D/D2D 与增强 | 不再从引擎 TransferVideoFrame；上传 BGRA 像素，尺寸来自会话而非音轨引擎 |

增强关闭或失败仍显示同一软解帧的普通画面，不能回退到系统视频解码或转码。
seek 必须同时等音轨真实完成和本代视频帧实际提交，再确认请求、释放声音；保持用户暂停及倍速意图。

## 删除与验证顺序

1. 固定共享会话/帧来源契约，覆盖分块读取、取消、队列背压、暂停、seek 代次、旧帧拒绝和音轨遮罩。
2. 并行接入 Apple、Android、Windows，复用现有 UI 和画面增强，分别真实目标编译。
3. NSH 起播改用内置路径；删除 NativeCompat、compat_play、CompatProgress、grow/HLS/增长文件供数及无消费者的播放转码代码。
4. 保留下载和归档合并本身需要的转码能力，不把它们当成兼容播放删除。
5. 跑 workspace 严格检查、库测试、IPC/wire 契约和前端验证；重新验证 NSH 实际 HEVC/ByteVC2 首帧、暂停、倍速、双向 seek、切集与自动续播。
6. 验证实际像素和声画时间；只有音轨进度、帧队列或界面标题变化都不算视频播放通过。Windows 真机缺失等验证边界继续明确记录。

原 PR 来自已删除的 fork，GitHub 重新打开操作已失败；原始提交 `b61e06e` 已获取。
最终通过本地合并保留原作者提交历史，再推送本仓库；不把原 PR 仍为 CLOSED 的页面状态误报为按钮合并成功。

## 本轮验证（2026-11-13）

- 全 workspace 库测试 1,293 项通过、48 项按既有条件忽略；新增进度条触摸回归 4/4 通过。
  `make lint`、前端 166 项测试、类型检查、构建和 6 项 wire 契约测试通过。
  最终 NSH/Apple `--all-targets` 严格检查、Apple 库测试 3/3 通过。
- Android 完整 JNI 宿主严格检查、Java 编译与像素/门控回归通过；独立验证包实际播放
  ByteVC2 与 HEVC，系统媒体只解 AAC。暂停、1.5 倍速、双向 seek/归零、切集通过；
  最终暂停点击进度条从 00:18 到 00:32 后仍暂停，无残留缓冲。旧转码文件的哈希、大小、
  时间均不变，没有新增 `.play.mp4`。
- Windows 完整 NSH 的真实 MSVC `--all-targets` 严格检查通过；没有 Windows 真机播放证据。
- iOS 模拟器完整应用包、资源与签名校验通过；最终模拟器与真机两个 release 目标检查通过。
  本轮没有 iOS 实机播放证据。
- macOS 正式播放操作流程 1/1 通过，覆盖暂停、倍速、切集、续播、画质切换及双向定位。
  实际解码出的 BGRA 帧已确认是正常彩色画面；后续窗口图像验收遇到系统锁屏，
  IORegistry 的 `CGSSessionScreenIsLocked=Yes`，Metal 因不可见窗口拒绝 drawable。
  锁屏期间的黑色窗口图、CPU 帧或时钟不能当作最终屏幕画面验收通过。

运行日志、截图与验证 APK 位于忽略的 `target/nsh-commit-20261113/`，不随源码提交。
锁屏诊断用的临时日志、帧转储与等待逻辑均已从生产和测试源码移除。
