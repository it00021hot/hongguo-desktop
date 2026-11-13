# HEVC 归档迁移

保留原流无损合并；编码不一致的输入统一转为 HEVC，再沿既有合并索引链封装。不新增动态库或链接 libx265。

播放与合并能力同时迁移。下载记录中的旧文件失效时重新获取在线原流；显式打开 AVC/H.264 文件则报不支持。
不再保留系统视频解码入口；系统媒体仅承担 AAC 音频与时钟。旧文件不自动转换或删除。

## 整合与迁移

- service 归档只接受有有效视频样本的 HEVC 输出，旧 H.264 完成缓存失效重建，防止误用旧中间文件。
- Tauri/GPUI/NSH 的能力字段同步为 `hevcHwEncoder`；设置与合并文案明确 HEVC 输出，移除本地 H.264 例外。
- GPUI 的系统视频工厂与 macOS/Windows 原生视频 presenter 删除，保留软解帧呈现、音频控制、暂停/倍速/定位。
- NSH Apple/Android/Windows 只接受已注册的软解帧源；缺帧源报错，不把原视频交给系统播放器。
- HEVC 编码必须真实试编并核对输出；不能将 HEVC 解码能力当成编码能力。缺编码器时仍可播放和原流无损合并，
  统一编码合并明确报告后端不可用，不回退 H.264，也不自动下载安装扩展。
- 使用既有平台编码器和现有 ffmpeg 调试路径，不增加新外部依赖。Windows/iOS 实机缺失等验证边界继续单独记录。

## 共享媒体归属

- `hongguo-media::mux`：移除 H.264/avcC 写出，实现 `hvc1`/`hvcC`，完整 VPS/SPS/PPS 数组；从实际 SPS 读取 profile/tier/level、色度、位深和时域层数。`hvc1` 样本剥离参数集，其余 NAL 转四字节长度前缀。
- 写出入口为 `mux_hevc` / `ArchiveHevc`；后者分批落盘、仅累积样本元数据，必须调用 `finish` 封存为带正常 `stbl` 的 faststart MP4。完成时顺序拷贝样本，不累积整集压缩载荷，不对外暴露渐进播放/片段读取接口。平台共用 `with_audio` 入口签名保留，AAC 字节、PTS/DTS、ctts、样本时长与 edit 语义保持。
- `hongguo-media-platform`：ByteVC2 解帧后输入各平台 HEVC encoder；ffmpeg 探测/参数只选择 HEVC encoder，输出标记 `hvc1`。平台门面使用 `transcode_hevc` / `hevc_hw_encoder_available`，能力字段使用 `hevc_hw_encoder`（wire `hevcHwEncoder`）。
- `hongguo-media-platform-api`：`I420View` / `EncodedVideo` 字段保持，编码输出统一 HEVC Annex-B；删除 AVC 输入解码变体，H.264 输入由业务层明确拒绝。
- 删除零消费者 `hongguo-media::h264` 参数集读取模块，拒绝 AVC 输出的测试允许保留 AVC 标识。

## 缓存与平台边界

平台 encoder 保持 `new`、`push`、`finish`、`take_publish_run` 契约，由 service 归档管线验证输出并封存完成标识。marker 升为 4，旧 H.264 归档中间文件失效后重新生成，不迁移、不回退。所有旧 H.264 编码别名删除。

## 验证

先以真实 HEVC 参数集封装的失败回归钉住旧 avcC 缺口；再验证普通 MP4 封存、HEVC profile/位深元数据、非法参数集与拒绝 H.264。原分片归档的空样本表曾使 concat 产物为 0 帧，现有真实回归明确要求 96 帧及全部 AAC 样本保留。ffmpeg 参数与能力仅选 HEVC，其实际外部编码执行不由下面的参数单元测试代替。跨目标整体检查由主集成统一记录。

2026-11-13 本域实际结果，日志位于 `target/nsh-commit-20261113/`：

| 范围 | 结果 | 证据 |
| --- | --- | --- |
| 真实 HEVC 封装缺口 | 旧 avcC 拒绝真实 HEVC，修复后 hvc1 通过 | `hevc-mux-{red,green}.log` |
| media lib | 96 通过，含 hvcC Main/Main10、IRAP、参数集边界、AAC/重排/尾时长 | `hevc-media-all-tests.log` |
| 归档与合并集成 | 3 通过：Main/Main10 每帧尺寸/位深/YUV 一致、AAC 原字节；原流视频/AAC 无损复制；分批归档正常封存后 concat 全样本 | `hevc-archive-roundtrip.log`、`hevc-fragment-merge-{red,green}.log` |
| Apple 归档时长 | AVFoundation 将 1 秒 HEVC 归档读取为 1 秒 | `hevc-media-all-tests.log` |
| 平台门面/API | 24 + 1 通过；真实 raw 用例默认显式忽略 | `hevc-platform-tests.log` |
| service/cache | 14 通过；格式 3 AVC 缓存与 AVC 输出先红后绿，AVC 输入明确拒绝；marker 4 无兼容回退 | `hevc-service-contract-{red,green}.log`、`hevc-archive-input-red.log` |
| merge service | 32 通过 | `hevc-merge-service-tests.log` |
| 共享媒体与 service 严格检查 | Clippy lib/tests exit 0 | `hevc-shared-service-strict.log` |

另一次显式 `HONGGUO_BYTEVC2_ARCHIVE_RAW=/tmp/hg-bytevc2-speed-core-30861/plain.266` 回归实际通过（`hevc-real-bytevc2-archive.log`）：真实 ByteVC2/VVdec 解出 60 帧，平台 HEVC 编出 60 帧，按测试定义的 30fps 时间轴封存 2 秒普通 `hvc1` MP4，concat 后为 120 帧。它证明真实解核→编码→封装→合并链；输入是无容器 raw、无 AAC，不伪造音轨，也不宣称完整 ByteVC2 MP4 容器端到端验证。真实内容不提交，生成的媒体产物已清理。

hvcC 字段布局参考 FFmpeg 官方 [`libavformat/hevc.c`](https://github.com/FFmpeg/FFmpeg/blob/master/libavformat/hevc.c)，仅对照规范字段，不引入其库依赖。

整合验证同样通过：`make lint`（含三目标 Clippy）、Rust 库分组回归、前端 166 项测试及 typecheck/build、Windows 六包 MSVC 严格检查、Android 全 JNI 宿主严格检查及 Java 编译、iOS 真机与模拟器目标检查。NSH 新增 H.264 拒绝和已有音轨遮罩/软解帧测试通过，失效下载转在线回归通过。GPUI 离屏预览完成，合并文案已目检。

本轮未重新打包或执行四平台完整设备验收；Windows/Android/iOS 的 HEVC 编码尚未实机验证。NSH 原生窗口预览未重跑，当前 macOS 会话锁定，页面布局未在本轮改动。完整 ByteVC2 MP4 输入的归档端到端验证仍未执行，以上 raw 回归不代替该项。
