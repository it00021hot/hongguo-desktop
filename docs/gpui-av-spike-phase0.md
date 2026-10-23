# GPUI 播放架构 Phase 0 / 1a：AVPlayer spike 结论

日期：2026-10-20｜平台：macOS 27.0.1 / Apple M6 / 3440×1440@100Hz
状态：Phase 0 四条硬指标通过；Phase 1a（渐进 Range 供数 + 真实在线剧集）通过，Phase 1b 可以开始。

## 1. 怎么跑

```bash
# 1) 生成样片（仓库内脚本，自研画面 + 440Hz 正弦，无版权内容）
swiftc -O scripts/make-av-spike-sample.swift -o /tmp/make-av-spike-sample
/tmp/make-av-spike-sample target/av-spike/sample-fmp4.mp4

# 2) 跑 spike（普通 GPUI 窗口 + AVPlayerLayer，默认启动路径不受影响）
HONGGUO_AV_SPIKE=1 cargo run -p hongguo-gpui
# 样片路径可覆盖：HONGGUO_AV_SPIKE_SAMPLE=/path/to/foo.mp4

# Phase 1a：真实在线剧集（推荐流 → prepare / prefetch，独立数据根）
HONGGUO_AV_SPIKE=1 HONGGUO_AV_SPIKE_SOURCE=online cargo run -p hongguo-gpui
```

样片：25fps / 1280×720 / H.264 + 48kHz AAC / 20s / 1,573,749 字节；
fMP4 结构 = `ftyp+moov`（init 1128B，`mvex`、空样本表）+ 21 个 `moof+mdat`（1s 分片）。
播放路径：`AVURLAsset` 自定义 scheme → `AVAssetResourceLoaderDelegate` 按 offset/length 回内存字节 → `AVPlayer` → `AVPlayerLayer` 挂到 GPUI 窗口的 `NSView`。

## 2. 实测数据

采样方式：独立线程 2ms 轮询 `AVPlayerItemVideoOutput`，只在
`hasNewPixelBufferForItemTime:` 为真时取帧并记录显示时间（播放器产出的帧序列，
不是 GPUI 的 layout/paint）。`drops` = 相邻帧墙钟间隔 > 期望间隔 1.5 倍的次数。

| 场景 | 产出帧率 | 期望帧率 | 间隔中位 | p99 | 最大 | drops | 刷新/帧 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1x | 25.0fps | 25.0fps | 40.1ms | 41.0ms | 41.4ms | 0 | 3.99 |
| **2x** | **50.0fps** | **50.0fps** | **20.1ms** | **20.8ms** | **21.4ms** | **0** | **2.00** |
| 3x | 75.0fps | 75.0fps | 12.8ms | 15.4ms | 15.7ms | 0 | 1.33 |

- 原基线（自研上屏，release，1080p HEVC）：2x 只有 45.1–47.1fps、2–13 次合并/5s、最大间隔 49–80ms。
- 本 spike：2x 连续 8s / 401 帧，帧率与内容帧率完全一致，无一次超过 1.5 倍间隔；100Hz 下每帧正好占 2 个刷新。5 次完整运行的波动：2x 最大间隔 21–30ms（最差一次只多占 1 个刷新），切集中位 58–61ms。
- 3x 也零掉帧（75fps，刷新/帧 1.33），说明瓶颈确实不在解码。

### seek 精度（暂停后零容差；25fps，1 帧 = 40ms）

| 目标帧 | 0 | 1 | 60 | 61 | 299 | 300 | 301 | 450 | 499 | 300（回跳） |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 显示帧 | 0 | 1 | 60 | 61 | 299 | 300 | 301 | 450 | 499 | 300 |
| 误差 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

- 10 次 seek 最大帧误差 **0 帧**；`currentTime` 与目标时间最大误差 0.0ms。
- 从发起 seek 到目标帧可用：中位 **2.7ms**，首次（冷启动）40.6ms；2x 播放中 seek 到 200 帧：落点 0 误差，端到端 47.2ms。

### 切集空档（AVQueuePlayer 预加载 + advanceToNextItem，首帧为 0 帧）

| 倍速 | 次数 | 中位 | 最大 |
| --- | --- | --- | --- |
| 1x | 5 | 60.9ms | 63.3ms |
| 2x | 3 | 58.1ms | 63.0ms |

远低于 200ms 门槛。注意：切之前下一项的 `AVPlayerItem.status` 仍是 `0 (unknown)`，但资源字节已经被队列预取（loader 累计回了 2× 全片），advance 后 ~60ms 出首帧——所以「status ready」不能当作切集就绪的判据。

### 2x 变速不变调

`AVPlayerItem.audioTimePitchAlgorithm` = `Spectral`（显式设置并回读确认），音轨正常输出、无欠载。
**未做客观音高测量**：进程内没有 AVPlayer 音频的公开 tap，系统录音回环需要额外权限/设备；这一条只到「配置 + 听感」级别，Phase 1 验收时需人工听一次。

## 3. 踩到的坑

1. `AVAssetWriter.movieFragmentInterval` 在 macOS 27 上**不产出分片**（.mp4 / .mov 都写成单个 `moov`）。可用路径是 `AVAssetWriter(contentType:)` + `AVAssetWriterDelegate` 分段代理；CMAF profile 拒绝混合多轨（"More than one track is not allowed"），改用 `AVFileTypeProfileMPEG4AppleHLS`（init 段 + `moof+mdat` 段）。
2. `AVPlayerItem` 只有关联到某个 `AVPlayer` 后才进入 ready；单独创建 item 等不到 ready。`AVQueuePlayer` 会预取字节，但下一项 status 仍是 unknown。
3. `AVAssetResourceLoader.delegate` 是 **weak**；必须自己强引用，且 `contentType` 是 UTI（`public.mpeg-4`）而不是 MIME，且必须落在 `allowedContentTypes` 内，否则 `finishLoading` 抛 ObjC 异常（Rust 侧无法 catch，会 abort）。
4. `contentInformationRequest` 必须给 `contentLength` + `byteRangeAccessSupported=YES`；少一个都会让播放/seek 失败或退化成不可随机访问。
5. `CMTime` 的 ObjC 编码是匿名结构 `{?=qiIq}`，写成 `{CMTime=...}` 会在 debug 断言里 panic。
6. `NSView.setAutoresizingMask:` 的编码是 `I`（u32），传 `usize` 会因编码不匹配 panic。
7. objc2 0.6.5 已把 `declare_class!` 改名 `define_class!`、`msg_send_id!` 标记废弃；spike 用 `define_class!` + `msg_send!` 直投，未新增任何依赖。
8. 主线程死等 asset 加载会永远等不到 ready（AVFoundation 的加载回调走 main queue）；真实 GPUI 主循环在跑，所以 spike 的测量线程不能阻塞主线程。

## 4. `AVAssetResourceLoader` 供数落地建议

结论：**复用 `hongguo-stream` 的数据面，跳过 `hongguo-protocol` 的 URI 面**。

1. 在 `hongguo-stream` 增加字节级只读 API（AVFoundation 只有 offset/length，没有 HTTP Range 头，`serve()` 的 `(status, headers, body)` 形态不该被复用）：
   ```rust
   pub struct StreamChunk { pub total: u64, pub start: u64, pub bytes: Vec<u8> }
   pub enum StreamReadError {
       Missing { vid: String, definition: u32 },
       Replaced,
       Timeout,
       Cancelled,
       OutOfRange { start: u64, total: u64 },
   }
   pub fn total_len(vid: &str, definition: u32) -> Option<u64>;
   pub fn read_range(vid: &str, definition: u32, start: u64, len: Option<u64>)
       -> Result<StreamChunk, StreamReadError>;
   pub fn read_range_cancellable(vid: &str, definition: u32, start: u64, len: Option<u64>,
       cancelled: impl Fn() -> bool) -> Result<StreamChunk, StreamReadError>;
   ```
   已按此落地：`serve()` 保留 HTTP 语义，内部改成「解析 Range → `wait_for_entry` → `read_plain_range` → 拼 206 头」，两条路径共用同一段等待/解密/读者登记逻辑（见 §6）。
2. **不要套用渐进模式的 4MB 开放 Range 窗口**：AVFoundation 要的是精确 offset 的字节段（fMP4 的 `moof`/`mdat` 偏移）。`read_range(..., Some(len))` 必须凑满 len 或明确失败；`requestsAllDataToEndOfResource=YES` 时分块回、全部回完再 `finishLoading`。窗口语义只保留给 `<video>` 的 `bytes=start-`。
3. `shouldWait` 必须**立即返回**，把 `read_range_cancellable` 的回包放到独立工作线程/池：AVFoundation 要等回调返回才消费已 respond 的字节，同步阻塞到整段读完会让渐进流首帧永远起不来（Phase 1a 实测：25MB B 首帧 2s 超时；改异步后 70ms）。`didCancelLoadingRequest:` 置位取消位，旧读取在 200ms 切片内退场并注销读者登记。
4. 每个 asset 一个 delegate；dispatch queue 必须是**并发**的（或用每请求独立 worker），否则 `didCancelLoadingRequest:` 会被阻塞中的 `shouldWait` 堵住、取消永远送不进来。delegate 由播放状态强引用（weak 属性）。`contentLength` 用 `total_len()`，在创建 `AVURLAsset` 之前必须已知（渐进流的 `plain_len` 注册后已知；不要用 size=0 的整集条目起播）。
5. 切集预加载：创建下一集 item 前先 `StreamCache::mark_prefetch(next_vid, def)`，让 `retain_playing_set` 同时保住当前集与下一集；`set_current` 只在真正切过去时调用（它会把该 vid 的 prefetch 标记清掉）。loader 的 delegate 绑定 `(vid, definition)` 句柄，不要从 URL path 反解析——`hongguo-protocol::respond_stream` 的 `/s/{档位}/{vid}` 形态是 Tauri URI 专用的。
6. 生产上先按「单播放器 + AVQueuePlayer」验证；若渐进填数导致切集空档逼近 200ms，再升级为双 `AVPlayer` + 双 `AVPlayerLayer` 预热接力（B 播 0 帧后暂停、切换时只换可见图层），这是比 item 级接力更稳的兜底。

## 5. 测量边界（结论的可信范围）

- 帧计数来自 `AVPlayerItemVideoOutput`（播放器渲染管线出口），不是窗口服务器逐刷新呈现计数；「无合并」的判据是「产出帧率 = 内容帧率 × 倍速，且没有 >1.5 倍期望间隔的空档」。`AVPlayerLayer.ready_for_display=true` 已确认图层有内容，但逐刷新合成无法从进程内观测。
- Phase 1a 已用真实在线剧集补测渐进供数（见 §6）；在线 2x/3x 是在 A 填满后测的，半途 2x 仍需 Phase 1b 实机限速补测。
- 音高未做声学测量（见 §2）。

## 6. Phase 1a：渐进 Range 解密供数（2026-10-20 补测）

### 6.1 落地改动

- `hongguo-stream` 新增 [`read_range`] / [`read_range_cancellable`] / [`total_len`]（`StreamChunk` / `StreamReadError`）；`serve()` 改成「解析 Range 头 → `wait_for_entry` → `read_plain_range` → 拼 206 头」，HTTP 与字节读取共用同一套等待/解密/`ReaderDemand`。新增 6 条字节级单测（整集/渐进/取消/EOF/越界/未注册）。
- `av_spike` loader 改为 `Source::{Bytes, Stream}`：`Stream` 直接绑 `(vid, 档位)`，`contentLength = plain_len`，精确回 offset/length（4MB 分块），`didCancelLoadingRequest:` 置取消位并调 `read_range_cancellable`。
- 在线模式：推荐流 → 详情页下一集 → `prepare(A)` / `prefetch_stream(B)`；用 `Range: bytes=0-0` 只探长度、挑最大的一集（探测不触发正片填充）；独立数据根 `target/av-spike/data`（不碰用户档案）。

### 6.2 全量字节 vs 渐进供数：切集空档

| 模式 | 样本 | 1x 中位 / 最大 | 2x 中位 / 最大 |
| --- | --- | --- | --- |
| 全量字节（file） | 25fps / 20s / 1.57MB | 63.3ms / 65.8ms | 61.5ms / 62.9ms |
| **渐进供数（online）** | 30fps / 238s / A 35.59MB、B 20.34MB | **75.5ms / 80.3ms** | **70.8ms / 79.7ms** |

- 两种模式都远低于 200ms；渐进比全量多约 **10–15ms**（B 首帧经 `read_range` 等数据 + 解密）。多次运行渐进切集中位在 63–77ms、最大 80.5ms。
- 本轮在线 A：《世子凶猛》第 1 集，`plain_len=35,590,554`；开播时 loader 只供了 **4,194,306B（约 11.8%）**，**2x 测量期间**填充才完成（所以本轮 2x 是真正的「边填边播」）；1x/3x 时 A 已填满。
- 本轮在线 B：第 2 集 `plain_len=20,337,684`；`prefetch_stream` 只填了 8MB 头部（约全集 40%），`advance` 前 status 仍是 `0 (unknown)`，首帧 70.8–80.3ms 拿到。
- 整轮 loader 统计：**45 次请求 / 127,313,330B / 0 errors**（A+B 合计，跨 8 次切集）。
- 对照的同步版 delegate（`shouldWait` 里阻塞到整段读完）在同样 25MB B 上 **2001ms 内拿不到首帧**，随后 `8MB..12MB` 读取等满 30s 超时；改成异步回包后同一场景 70–80ms。**`shouldWait` 必须立即返回、回包放独立线程/工作池**，这是本轮最重要的坑。

### 6.3 其他指标（在线渐进）

| 场景 | 实测 |
| --- | --- |
| 1x | 30.0fps（期望 30.0），drops 0 |
| 2x（填充中） | 60.1fps（期望 60.0），gap 中位 17.0ms / p99 19.4ms / max 29.6ms，8s 内 1 次多占 1 个刷新（drops 1） |
| 3x | 90.0fps（期望 90.0），drops 0–1（最差一次多占 1 个刷新） |
| seek（10 次，含 ±1 帧与回跳） | 显示帧误差全部 0；到帧中位 6.1ms、最大 103.5ms（冷启 seek 到 0），单次 3–104ms |
| 音高 | `Spectral` 配置确认，仍未做声学测量 |

### 6.4 结论：Phase 1b 可以开始

1. **GO**：真实在线 30fps 剧集在渐进供数下，2x/3x 零掉帧、seek 零帧误差、切集 70–80ms。
2. 生产 loader 必须异步 + 有界工作池（spike 是每请求一线程，只适合诊断）；`contentLength` 必须在建 asset 前拿到（等 `plain_len`）。
3. 8MB 预取头部 + 异步流式供数覆盖了本轮 20–36MB 的单集；若后续遇到首帧依赖头部以外数据、或网络更慢，优先在切集前 `prepare(B)` / `resume_fill(B)` 把 B 转正续填，再考虑把预取预算调大；双 player + 双 layer 仍是空档逼近 200ms 时的兜底。
4. 错误路径必须对 `AVAssetResourceLoadingRequest` 调 `finishLoadingWithError:`（不是 `dataRequest`，后者没有该方法，会 abort）。
5. 取消路径已验证：旧读取在 5s 内退场并注销 `ReaderDemand`，条目数据随后补齐仍可重读。

### 6.5 测量边界（Phase 1a 新增）

- 本轮把 2x 放在最前面测，2x 期间 A 仍在填充（开播时仅 11.8%）；1x/3x 在填充完成后测。更慢网络下的「半途 2x」仍需 Phase 1b 实机补测。
- 切集 B 只用到 8MB 预取头部 + 按需流式；若首帧需要头部以外的 moov/分片，空档会退化到等待 `read_range`，必须靠转正续填解决。
- 单集样本来自推荐流当次下发，剧名/大小会随服务端轮换变化。
