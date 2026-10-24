# 自写 MP4 封装器：替换 vendor/muxide

> 状态：**已落地**（2026-10-24）。`vendor/muxide`、muxide 依赖与根 manifest 的
> `[patch.crates-io]` 条目都已删除，改用本仓自写的封装层——它复用 `remux` 已有的
> `moov` 写入能力（现已提到 `crates/hongguo-media/src/mp4.rs`，两条产出路径共用）。

## 为什么要换

`vendor/muxide` 是一份整份上游拷贝（1.47 MB / 76 个文件），真正的改动只有 **1 行**：
`compute_interleave_schedule` 里视频的排序键 `sample.pts` → `sample.dts`。

| 事实 | 证据 |
| --- | --- |
| 上游没修，升级无效 | muxide 最新 0.5.0（2026-10-19）仍是 `sample.pts`（`src/muxer/mp4.rs:1237`） |
| muxide 没有 interleave 策略开关 | Builder 只有 `fast_start` / `metadata` / `sps` / `pps` |
| 第三方替代品不支持 B 帧 | `mp4_muxer` 文档："B-frames / DTS ≠ PTS: not supported — encode with pts == dts"，且 AV1-only、无音轨 |
| 补丁是承重的 | macOS VT 原样透传编码器 PTS（`vt/pipeline.rs:99-102`），真机出过 B 帧流（标题帧被顶进转场中间） |
| 补丁只在 macOS 触发 | Windows MF 的输出 PTS 被显式强制单调（`mf/pipeline.rs` 的 `emit_outputs`） |
| muxide 只服务两条平台硬编路径 | ffmpeg 路径自己出 MP4（`ffmpeg/transcode.rs`），不碰 muxide |
| 我们自己已经有 moov 写入器 | `crates/hongguo-media/src/mp4.rs`（原 `remux/index/moov.rs`）：stts / ctts / elst / stsc / stsz / stco 全有，**ctts 是一等公民** |

最后一条是关键：**B 帧的 composition offset 在本仓的 moov 写入器里是原生支持的**，
正是 muxide 要打补丁才能做到的事。所以自写不是从零发明，而是把已有的能力接上另一条产出路径。

## 目标结构

```
crates/hongguo-media/src/
  mp4.rs            共用写出层：TrackSpec / Edit / build_moov / mdat 布局
                    ← 迁自 remux/index/moov.rs + plan.rs 的两个结构体
  mux.rs            封装门面：mux_h264 / with_audio（交织 + 偏移 + 写盘）
  mux/avcc.rs       Annex-B → AVCC；avcC 构造
  mux/audio.rs      音轨采集 + esds 原样搬运   ← 迁自 transcode/audio.rs
  mux/track.rs      从编码输出构造 TrackSpec（mvhd/tkhd/mdhd/hdlr/vmhd/smhd/dinf）
  remux.rs          改：引用 crate::mp4
  remux/index/      改：产出 crate::mp4::TrackSpec
  transcode.rs      瘦：只剩 TranscodeResult + read_range
```

删除：`remux/index/moov.rs`、`transcode/mux.rs`、`transcode/audio.rs`、`vendor/muxide/`。

## 关键技术决策

1. **faststart 恒开**（moov 在 mdat 前）。muxide 现状是「无 B 帧时开、B 帧时关」
   （它搬移 moov 时假设写入序≈pts 序）。本仓两遍写入不受此限，remux 也一直这么做。
2. **B 帧时间轴**：`dts[k]` = 第 k 小的 pts（解码槽位），`ctts[k]` = `pts[k] - dts[k]`
   （version 1，有符号）。`stts` 是 dts 序列的差分。与 muxide 的口径一致。
3. **交织**：视频按 **dts** 升序、音频按 pts 升序，同一时刻视频优先。视频这一侧
   必须是 dts——`stco` 按写入序记录而 `stsz`/`stts`/`ctts` 按样本序构建，按 pts 排
   会让表与数据互相矛盾（这正是 muxide 要打补丁的那一行）。
4. **音频**：裁掉 pts 早于视频首帧的样本（复刻现状）；`esds` 从源音轨**原样搬运**
   （音轨直通不重编码，采样率/声道不变，esds 天然一致）。
5. **avcC**：profile/level 取 SPS 的第 2~4 字节，SPS/PPS 原样写入，**不解析 SPS 宽高**
   （宽高由调用方给，写进 avc1 sample entry）。
6. **AVCC**：Annex-B → 4 字节长度前缀；参数集**不剥离**（与 muxide 现状一致，播放器行为不变）。
7. **每样本一个 chunk**（`stsc` 只有一条 `(1,1)`）——复用 remux 的既有策略。

## 验证

| 项 | 手段 | 状态 |
| --- | --- | --- |
| 容器结构 | 本仓 demuxer 回读产物：轨道数、codec、样本数、样本边界逐字节比对 | ✅ `mux/tests.rs` 7 条用例 |
| B 帧流 | 移植 mux.rs 的两条回归测试（解码序流 + 3000 帧真实规模 + AAC 交织），另加 `ctts` 合成偏移与音频裁剪两条 | ✅ |
| 快速合并兼容 | 产物经 `remux::concat_copy` 拼接后仍可解复用 | ✅ `remux` 既有 e2e |
| 真机产物 | `HONGGUO_E2E_DIR` 门控的 e2e（ffprobe 认时间轴、平台硬编产物可播） | ⏳ **未跑**：需 macOS + 真实剧集文件 |
| 门禁 | `cargo test --workspace` + clippy 三目标 + fmt + 前端 lint/typecheck | ✅ 全绿 |

## 风险

- **自写 MP4 结构的边界情况**（elst、stss、ctts 符号、时基）。缓解：复用 remux 已验证的
  moov 写入器 + 字节级回归测试 + 本仓 demuxer 回读。
- **产物字节结构变化**（faststart 恒开、moov 由我们生成）。缓解：快速合并读的是 moov
  语义而非字节位置，重建索引后不受影响；e2e 覆盖拼接链路。
- **macOS 平台层的 PTS 原样透传**依赖 B 帧支持，自写后这条契约保留。
