# 内置播放后的媒体接口清理

> 本文保留第一轮清理及测试的历史记录。其中本地 H.264 系统播放例外、H.264 归档编码和 `GrowingH264` 均已淘汰。
> 当前统一使用内置视频解码；下载旧文件无效时重新获取在线原流，混合素材合并输出 HEVC。
> ByteVC2 归档封装已使用 `ArchiveHevc` 完成封存，旧 `GrowingH264` 名称及播放增长供数接口不再使用。
> 现行编码、缓存与能力契约见 [HEVC 归档迁移](hevc-archive-migration.md)，下文旧实验结果不代表新路线验收。

HEVC/ByteVC1/ByteVC2 内置播放经 `hongguo-softdec::DirectSession` 与各壳帧呈现；H.264 沿现有系统播放路径。本次移除旧播放转码专属接口，保留合并归档编码。

共用播放服务固定使用内置能力，不再保留壳声明、系统播放模式或 Windows opt-in。
GPUI、Tauri、NSH 的 HEVC/ByteVC1/ByteVC2 均直接内置解码；失败明确报错。
删除 HEVC 扩展安装服务、GPUI 安装引导与双语旧文案。已选本地文件解析失败、无视频轨或编码未知时返回错误，
只有明确的本地 H.264 保留系统路径，不把失败转换成系统解码回退。

## 范围与归属

- `hongguo-media-platform-api`：删除 `RateProfile`、`bitrate_for_profile` 及 `PlatformRequest.rate`，仅保留归档 `bitrate_for`（0.12 bit/像素/帧，0.8–12 Mbps）。移除无生产消费者的首段、提交及取消回调；归档 `on_progress` 保留。
- `hongguo-media-videotoolbox`、`media-platform-{windows,android}`：删除 profile 透传与播放低延迟分支，保持原 Archive 编码参数（包括 VT GOP250、非实时设置）、缩放、音频直通及收尾顺序。
- `hongguo-media-platform`：删除 ffmpeg 播放质量档和 growing 播放输出分支；保留归档 ffmpeg 参数、临时文件原子替换及显式 debug fallback 策略。删除三个仅用于兼容播放的 example（`p03_ffmpeg_grow`、`p03_throughput`、`bytevc2_stages`）。
- `hongguo-service::transcode_service::pipeline` 与 Tauri/MF 测试调用方：同步精简请求构造，不改四参归档入口。合并经 `merge_service::compat` 调用此入口；下载仍保存原编码，不引入播放转码。
- `hongguo-media::mp4::fragment_index`：旧 HLS 删除后仅剩自身测试，删除模块及对应自测。
- `hongguo-media-platform::capability::playback`：删除无消费者的系统播放 HEVC 探测缓存、重探及 `HONGGUO_NO_HEVC` 开关；归档转码的真实平台能力探测与 `selected_backend` 保留。
- 删除无消费者的 `platform::hevc_decode_available` 门面，以及仅为它服务的 Android WebView 探测缓存、SDK 版本读取 FFI、Windows 单独 `hevc_decoder_present` 与上层导出。保留 Android 归档管线调用的 `probe::hevc_decoder_present`，以及 Windows `probe_encoder_kind` 中归档编码所需的真实 HEVC MFT 枚举。

## 必须保留

`GrowingH264` / `LiveFile` 是实际 ByteVC2 合并归档链的封装器，并非残留播放接口。保留其分片批次、视频时序、AAC 搬运、完成封存，以及 `read_fragment_video` / `prefix_is_independently_decodable` 产物校验。合并任务自身取消、DirectSession/HTTP 读取取消不属于本次删除范围。

## 验证

先用码率标定与 ffmpeg 归档参数特征测试钉住现有行为，再精简接口；复跑归档音视频分片与时长测试、相关 crate 严格 Clippy。平台 API 冻结后由主集成统一运行 Windows/Android/iOS 实际目标检查，避免与旧 ABI 并发构建。不进行 GUI、账号操作或 Git 操作。

2026-11-13 定向结果（`target/nsh-commit-20261113/`）：

| 检查                                          | 结果        | 日志                                          |
| --------------------------------------------- | ----------- | --------------------------------------------- |
| 清理前码率 / ffmpeg 归档参数特征测试          | 1 + 1 通过  | `archive-{bitrate,ffmpeg}-before-cleanup.log` |
| platform / platform-api lib                   | 24 + 1 通过 | `archive-cleanup-tests.log`                   |
| media lib（含保留的归档时序、音频与封装校验） | 99 通过     | `archive-media-cleanup-tests.log`             |
| VideoToolbox lib（含实际灰帧 H.264 编码）     | 6 通过      | `archive-vt-cleanup-tests.log`                |
| 三个相关 crate Clippy lib/tests               | exit 0      | `archive-cleanup-strict.log`                  |

跨目标整体验证由主集成记录，此表不将本机测试推定为 Windows/Android/iOS 通过。

最终集成结果：

- `make lint`、前端 166 项测试、类型检查与构建通过。
- 33 个 Rust 库测试套件分组执行，共 1,288 项通过、48 项按既有条件忽略。
  首轮全库执行遇到既有 `device_proto_is_non_empty_and_stable` 时间敏感断言；
  同一 workspace 测试二进制的该用例复验通过，其余签名测试及剩余库套件也通过。
  签名算法、常量及测试源码未改动。
- GPUI 29 项集成测试、77 个离屏预览场景通过；移除安装浮层后没有增加占位或修改舞台布局。
- 最终 GPUI/player/NSH 的真实 MSVC 全目标 strict 通过；Android 完整 NSH JNI 宿主全目标 strict 通过。
- iOS 模拟器与真机两个 release 目标检查通过。本轮没有重新打包或操作实际播放窗口；
  Windows/iOS 实机播放及 macOS 锁屏后的屏幕画面验收边界仍保留，不用编译结果代替。

媒体归档请求现仅含 `input` / `output` / `scale_to` / `on_progress`，不再暴露播放码率、
可播首段或提交回调。应用内更新安装本轮不实施。
