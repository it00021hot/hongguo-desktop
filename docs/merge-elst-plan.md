# 快速合并保留逐集 edit list（elst）方案

> 状态：已实施（2026-10-19）。对应测试发现 P0-1：合并后音画不同步，10 集音频
> 容器时长比视频长 239.8ms，日志每集 `收尾对齐差…超出末样本时长`。
> 归属：`crates/hongguo-media/src/remux/index/`。
>
> 真机结果（同一 10 集源）：产物 `elst` 10 段；AVFoundation 读
> asset=968.275s、video=968.269s、audio=968.275s；音频原始 `stts` 仍为
> 968.738s（含 priming），presentation 由 edit list 跳过，播放器侧对齐。

## 1. 问题

平台短剧每集独立编码，音频 AAC priming 由容器的 `edts/elst` 表达：

- 音频 `elst media_time=2048`（44.1kHz = 46.4ms）；
- 视频 `elst media_time=2048`（15.36kHz = 133ms）；
- `elst` 的 `segment_duration` 是该轨的 presentation 时长（movie timescale）。

当前 `read_source_file` 只搬 `tkhd/mdhd/hdlr/stsd/header/dinf`，`edts` 被丢弃；
`plan` 用原始 `stts` 逐集累加，再用 `lock_tail_to` 把差值补进末样本。末样本最多
只能改 1024 tick=23.2ms，补不掉开头的 2048 tick priming，于是每集残留约 23ms，
10 集约 240ms；warn 每集都触发，说明这是正常源，不是病态源。

## 2. 决策

**保留/重建逐集 edit list，不裁剪样本表。**

`elst` 是容器表达 priming/起始偏移的原生机制；丢弃它再用 `stts` 改末尾，等于
用错误层的补丁修另一个层的语义。保留 edit list 后：

- `stts/mdhd` 保持源媒体时间轴，样本数、`ctts`、`stss`、偏移全部不动；
- `tkhd/mvhd` 用 presentation 时长；
- 播放器按 `elst` 跳过每集开头的 priming；
- 不再需要 `lock_tail_to`，删除 `alignment_targets` 与逐集末尾钳制。

不采用“丢掉每集开头 2048 个音频样本”的路线：会改样本数、`stss/ctts/stco` 与
解密样本边界，且视频轨同样有 edit，范围更大。

## 3. 数据模型

```rust
pub struct Edit {
    pub segment_duration: u64, // movie timescale
    pub media_time: i64,       // media timescale；-1 = 空 edit
    pub rate: i32,             // 16.16，正常 0x0001_0000
}
```

- `SourceTrack` 增加 `edits: Vec<Edit>`：从 `trak/edts/elst` 读全部条目；
- `MergedTrack` 增加 `edits: Vec<Edit>`：逐集合成；
- `moov.rs` 的 `build_trak` 在 `tkhd` 后写 `edts/elst`（version 1）；
- 无 `elst` 的源补一条默认 edit：`media_time=该集 media 起点`，
  `segment_duration=源 tkhd duration`（0 时按 stts 时长换算到 movie timescale）。

## 4. 逐集合成

对每条轨、第 n 集：

1. `media_start = 该轨此前各集 stts 累计`（media ticks）；
2. 源有 edit：每条 `media_time >= 0` 的条目加 `media_start`，`-1` 空 edit 原样保留；
   源无 edit：补一条 `(segment_duration, media_start, 1.0)`；
3. `tkhd` 时长 = 各集全部 edit 的 `segment_duration` 之和；
4. `mdhd` 时长 = 原始 stts 累计（媒体时长）；
5. `mvhd` 维持现有“各集 mvhd 求和”（源 mvhd 已表达每集 presentation 上界）。

不直接改 `stts`。如果某个源没有 edit 且音视频时长天然有差，保留真实媒体时长，
由 presentation 层表现；不再用末尾样本制造假时长。

## 5. 测试

- fixture `TrackPlan` 增加 `edit_media_time: Option<i64>`，`build_trak` 写
  `edts/elst`，`tkhd` 写 presentation 时长；
- 新增：`audio_priming_edit_is_skipped_in_every_episode`（audio media 6 tick、edit 2、
  video 4 tick；断言 audio `mdhd=12`、`tkhd=8`、edits `[(4,2),(4,8)]`，视频
  edits `[(4,0),(4,4)]`）；
- 新增：`edit_media_times_accumulate_across_three_episodes` 与
  `tracks_without_edits_get_contiguous_default_edits`；
- 新增：`moov_writes_audio_edits_per_episode` 直接解析产物 `elst` 字节；
- 删除旧的 `audio_tail_is_padded…`、`audio_much_longer…`（它们钉的是尾巴钳制，
  不是 edit 语义）；
- 真机：10 集快速合并后用 `stts`/`elst` 解析核对，并播尾段核对口型。

## 6. 风险

- 多段 `elst` 的播放器兼容性：Tauri WebView、GPUI 本地播放、macOS QuickTime 各验一次；
- 源 edit 的 `segment_duration` 之和与源 `tkhd` 不一致时，以 edit 为准（presentation
  的真实定义），`tkhd` 用 edit 和；
- `mvhd` 仍用源 mvhd 求和，若以后遇到跨轨时长长期不一致的源，再单独评估是否改成
  “逐集跨轨最大值求和”。
