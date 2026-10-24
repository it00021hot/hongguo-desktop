//! 拼接产物的 `moov` 重建。
//!
//! 快速合并不解码也不编码，但**必须重写索引**：把 N 个各自从 t=0 开始的样本表
//! 拼成一条从 0 连续到全集末尾的时间轴。字节级顺序拼接做不到这件事——第 1 集的
//! `stco` 只覆盖第 1 集的 `mdat`，播放器读完就停，后面的集拿不到索引。
//!
//! 三条设计决定：
//!
//! - **每样本一个 chunk**（`stsc` 只有一条 `(1,1)`）。换来的是不必按源 `stsc`
//!   重新给 chunk 编号，也就不必处理「各集 stsc 不一致」这一整类分支。
//!   10 集约 2.2 万样本 → `stco` 约 88KB；只有合集超过 4GB 才升级成 `co64`。
//! - **交织顺序照抄源文件**。把一集内所有轨道的样本按源偏移归并排序后写出，
//!   就精确复刻了原文件的音视频排布，不需要对交错方式额外做假设。
//! - **`ctts` 的 offset 原样照搬**。合成时间 = 解码时间 + offset，而拼接后
//!   解码时间本身已随各集时长整体平移，两者同步平移后差值不变。再额外加一次
//!   起点偏移，会让每集首个 B 帧的错位逐集累积。
//! - **逐集保留 `edts/elst`**。各集独立编码，AAC priming/视频起始偏移由 edit
//!   list 表达；丢弃它再改样本表末尾补时长，只能补掉一个样本（约 23ms），
//!   补不掉开头的 2048 tick（46ms）priming，10 集约残留 240ms（issue #11
//!   「合并后图音不同步」）。现在把源 edit 的 `media_time` 平移到合并轨的
//!   集起点，`segment_duration` 照旧用 movie timescale，逐集写回 `edts`；
//!   `stts` / 样本数 / `ctts` / `stss` / 偏移全部不动。
//!
//! `mvex` / `udta` 丢弃；`edts` 是时间轴语义的一部分，必须保留。
//!
//! `moov` 的字节组装在 [`crate::mp4`]——快速合并与重新封装共用同一份写出层。

mod plan;

pub use plan::{SourceFile, plan, read_source_file};

#[cfg(test)]
use plan::{MDHD_DURATION, MVHD_DURATION, TKHD_DURATION, duration_of};

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
