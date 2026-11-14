# NSH Video 编码补齐与 FFmpeg 移除方案

2026-11-14。状态：已替换播放和归档的 FFmpeg 依赖，正在完成各目标验证。实际实现与验证边界见本文及 [验证记录](nsh-decoder-validation.md)。

**播放以 NSH 0.5.0 的 Video 为基础，只补官方 APK 实际下发而 Video 无法解码的格式；移除 FFmpeg。** 官方 APK 各清晰度提供什么流，我们就支持对应的流、画质和音频。客户安装即可播放，Windows 范围是普通 Windows 10 1809+ / Windows 11。

## 1. 保留和补齐的边界

| 部分 | 新方案 |
| --- | --- |
| 页面与播放控制 | 统一使用 Video 的 source、暂停、seek、音量、倍速、进度、缓冲、结束和错误契约 |
| 普通媒体播放 | 使用 NSH 原生 Video 的完整播放能力 |
| 网络、缓存、MP4、CENC | 保留现有 Rust 供数、索引及解密；不另建下载或解密链 |
| 系统缺失的视频编码 | 仅增加对应的解码器及 Video 平台接入，复用已有播放引擎 |
| AAC、音频输出、倍速、主时钟 | 复用原生播放引擎；不保留 FFmpeg PCM 播放链 |
| FFmpeg | 删除播放库、FFI、构建下载、运行库包装及相关专用工具；归档的外部 FFmpeg 回退也一并清理 |

不通过换编码、降清晰度或降低位深来避开格式支持。播放不增加转码，不恢复 `.play.mp4`、grow 或 HLS 链路。壳内保留一个播放器组件和一套控制语义，特殊编码的处理收在 Video 的内部适配层。

## 2. 已核实的基础

NSH 0.5.0 的固定源码为 `f4d68f7`，本次检查了实际实现：

| 平台 | Video 当前实现 | 需要补齐或验证的部分 |
| --- | --- | --- |
| Windows | Media Foundation MediaEngine，frame-server，通过 D3D11/DXGI 接 NSH 画布 | ByteVC2；系统 HEVC 解码器不可用时的 HEVC/ByteVC1；Main10/HDR 输出 |
| macOS / iOS | AVPlayer / AVPlayerLayer | ByteVC2；实际 ByteVC1 参数、音频变体及倍速 |
| Android | MediaPlayer + TextureView | ByteVC2；实际设备缺失的 HEVC profile/位深及音频变体 |

Windows 原生 Video 当前固定 `B8G8R8A8_UNORM` 输出，不能直接视为已保留 Main10/HDR。需要在组件输出端补上对应的高精度路径；现有颜色、元数据及呈现实现有用的部分继续复用，不能为了删 FFmpeg 丢失画质。

现有真实取流记录确认 ByteVC1 和 ByteVC2。ByteVC1 的已验证样本可按 HEVC 解码；ByteVC2 使用现有配置/样本转换与 VVdeC。H.264 在候选流合成测试里出现，不把服务器提供 H.264 当作本方案的前提。以上观测也不等于官方所有剧集、清晰度、profile 和音频变体已经穷尽。

实施矩阵按实际 APK 响应及媒体轨道逐项记录：清晰度、尺寸、帧率、编码、sample entry、profile/level、位深/采样、颜色/HDR、AAC AOT/ASC、声道及采样率。源码中能识别的标签不能代替实际样本证据。

## 3. 缺失编码的具体补齐方式

### Windows

默认完整复用 NSH Video / MediaEngine。系统能够正确解码的流由原生引擎调度软硬解，不再通过 libavcodec 包装 D3D11VA。

HEVC/ByteVC1 的扩展路径优先使用应用内 D3D11 VLD 解码；Main 输出 NV12，Main10 输出 P010。
直接向显卡提交 HEVC 图片参数、参考帧、缩放矩阵和压缩切片，不经过系统 HEVC MFT，
因此无需 HEVC Store 扩展。复用项目已有 rusty_h265 0.7.1 的语法解析，不运行其软件像素解码。
GPU/驱动不支持该规格、初始化/提交/表面导入失败时，才回退 **libde265**；
ByteVC2 仍使用 **VVdeC 3.2.1**。不引入 FFmpeg 或新的原生构建工具。

当前实现保留 NSH Video 的原生 MediaEngine 入口；原生错误才按同一媒体源和当前位置创建专用视频会话。
ByteVC2 与原生输出精度不足的 HEVC 在组件内部直接选择扩展帧路径；原生 MediaEngine 读取同一 MP4 的音轨视图并提供时钟。
音轨视图只屏蔽视频 trak，文件长度、音频样本、时间偏移和 edit list 均不改动。
固定 NSH Video 没有媒体源/MFT 注入的公开接口，本轮没有实现或注册本地 MFT；不能把现有内部帧适配描述为 MFT 插件。

显式开启增强时复用现有高精度帧处理路径。关闭增强会关闭处理，但当前媒体会话继续使用扩展解码，下一媒体源重新选择原生路径；因此该切换的能耗仍需优化和实测。

最终 Windows 包无需客户安装 HEVC 扩展、编解码包或 VC++ Redistributable。
系统缺少 HEVC MFT 时，普通 Video 报错后仍会先尝试 D3D11，失败才使用随包软件核。
Main10/HDR 和显式增强直接进入同一硬解优先的扩展路径，保留原始位深、VUI、mastering display、内容亮度、crop、SAR 和旋转。
P010 在 GPU 内以原精度采样；HDR/scRGB 与增强使用高精度颜色链，SDR 屏仅在最终显示阶段做映射与抖动。
硬解失败会撤销旧代次，暂停并定位原生音轨，从当前媒体位置的关键帧重建软解；
同设备同会话不反复尝试失败后端，设备重建后可重新尝试。

实现检查实际 profile/输出格式和解码配置，采用驱动推荐的纹理数组或独立纹理布局。
DPB、显示重排与消费端共享表面租约，GPU 消费结束前禁止复用；解码繁忙重试可取消且有 250ms 上限。
不支持的 HEVC profile/色度格式、格式变化或无法提交的码流会按同一媒体源回退，不能降位深凑成功。
Windows 真机硬解出帧、软硬像素对拍、连续播放和功耗仍需实测，交叉链接不能替代这些结论。
接口依据：[D3D11 解码设备](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11videodevice-createvideodecoder)、
[微软 HEVC DXVA 规范](https://www.microsoft.com/en-us/download/details.aspx?id=39947)。

### Apple 和 Android

系统能播放的流完整使用原生 Video。ByteVC2 等缺失编码通过 Video 内部的平台适配接入现有专用核，原生引擎继续承担 AAC、倍速和主时钟。

Apple / Android 不能照搬 Windows 的 MFT 注册方式。特殊视频轨需使用现有样本索引解帧，并在组件内部接对应平台的帧呈现，与原生音轨播放时钟同步。这是仅服务缺失编码的组件适配，不另建全格式 PCM 播放器；必须明确接入原生时钟，不能靠 UI 的低频 onProgress 事件同步视频。

普通流与特殊编码路径共用暂停、seek、倍速、缓冲和生命周期契约；切集和 seek 撤销旧代次、释放帧租约。首帧、欠载、恢复与音频是否前进要保持一致。实际音频变体若发现原生引擎无法支持，再补该具体变体，不预先重建全格式音频处理链。

## 4. 代码与开发依赖的收敛

1. `hongguo-nsh/src/player/renderer` 恢复 Video 主入口；`media` 提供解密后的原 MP4 地址和必要的特殊媒体源描述，不再为所有流创建 `hongguo-playback` FFmpeg 会话。
2. 平台 crate 只保留缺失编码的适配、实际所需的帧呈现和颜色处理。删除普通格式也强制走 PixelView 的分派；保留组件外观、叠层和控制行为。
3. 移除 `hongguo-codec-ffi` 及其所有调用、FFmpeg decoder audit、FFmpeg PCM/重采样/atempo 路径。现有 `hongguo-playback` 中 MP4 时间轴和索引等必要逻辑按真实使用者收敛；无消费者的会话和专用 PCM 设备 crate 删除，不留闲置的第二套播放框架。
4. 清理 Cargo feature/lock、运行期 DLL/dylib 路径、FFmpeg 四库包装、FFmpeg 来源清单及专用重链接工具。PE 审计按最终实际运行库调整，继续验证静态 CRT、明确的系统依赖及随包库闭包。
5. VVdeC 和所需 HEVC 核提供固定版本、校验和及准确许可证材料的预编译开发包。日常开发不要求 FFmpeg、MSYS、GPG、NASM、CMake 或 bindgen；源码重建只作为维护者流程。预编译包的发布是独立对外操作，不在本次方案梳理中执行。
6. 下载原流、同规格无损合并保留；异规格归档使用既有原生 HEVC 编码层。去除外部 FFmpeg 查找、回退及安装引导。原生 HEVC 编码不可用的机器明确报告归档能力不足，不能影响在线播放，也不能把播放库删除等同于解决了编码能力。

正式媒体地址为带临时访问范围的本机 HTTP MP4 Range，供 NSH/AVPlayer/MediaEngine/MediaPlayer 直接读取；不使用 Tauri 自定义 URI。
`hongguo-frames://` 仅为内部帧注册句柄，不是网络媒体地址。六目标开发 SDK 已随 `vendor/native-codecs/` 提供，正常启动直接解包和验证，无需等待外部发布。
7. GPUI/Tauri 不扩建新播放器，做共享依赖清理所必需的编译适配；删除旧壳仍为独立任务。

libde265 与 VVdeC 各自遵守对应许可证及分发要求；不能直接沿用此前 FFmpeg 的许可证或静态重链接判断。专用核的 SDK 不再携带 FFmpeg，现有尚未发布的 FFmpeg SDK 归档不继续发布。

## 5. 验证与完成条件

先在 Windows 独立 Video 实例验证系统流和缺失 HEVC/ByteVC2 的接入，证明 MediaEngine 实际使用扩展且正常输出原生音轨，再迁移主入口。这个依赖顺序不代表接受减少格式的最终产品。

- 所有实际 APK 清晰度与编码组合均以真实样本验收，包含当前已知 ByteVC1、ByteVC2、8/10 位与实际 AAC 变体。完整解帧、随机 seek、尾帧和原位深对拍通过；特殊编码不能只证明单帧解码。
- 0.75、1、1.25、1.5、2、3× 下，验证原生保音调倍速、连续进度、暂停、seek、缓冲恢复、切集与设备变化。连续记录视频呈现和实际音频时钟；报告掉帧与音画偏差，不把离线解码 fps 当作实播结果。
- 普通 Windows 10 1809+ / Windows 11 干净系统，无 HEVC 扩展和开发工具，仍可播放上述样本；macOS 无 Homebrew，移动端使用正式宿主的安装包。Windows N 不验收。
- 播放依赖图及最终安装包均没有 FFmpeg/libav*，运行与开发入口不查找 FFmpeg；所有随包库均包含来源、校验与分发材料。
- 四平台真实目标编译、相关业务/组件测试和包依赖审计通过。Windows 原生播放、Mac 可见窗口、移动端真机结果分开报告；锁屏或缺设备不能算实播通过。
- 包体、开发工具要求、首帧/seek、持续 CPU/GPU/内存和能耗以新旧同样本、同画质、同倍率测量。没有实测前，不宣称移除 FFmpeg 必然更快或更省电。

English summary: NSH Video remains the playback foundation. Windows HEVC extensions prefer direct D3D11 VLD with NV12/P010 surfaces, independently of the HEVC Store MFT; libde265 is the fallback. ByteVC2 uses VVdeC. Audio and clocks remain native. Codecs are vendored and FFmpeg stays removed. Physical hardware decoding, pixel accuracy, sustained playback and energy require Windows validation.
