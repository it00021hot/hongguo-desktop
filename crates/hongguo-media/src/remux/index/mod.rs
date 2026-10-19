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
//! - **非视频轨逐集锁齐视频主轴**。各集独立编码，每集音/视频轨时长有毫秒级
//!   差值（被丢弃的 `edts` 本该跳过的 AAC priming 也算在内），按轨独立累加
//!   会让它越积越大——声音越播越偏（issue #11「合并后图音不同步」）。
//!   每集收尾把差值补进该集末样本的 `stts` 时长（[`plan`] 的
//!   `lock_tail_to`），两轨时间轴逐集对齐，偏差不再跨集累积。
//!
//! `edts` / `mvex` / `udta` 一律丢弃：编辑列表指向各集自己的片段，合并后语义
//! 已不成立；时长改由样本表决定，对本地播放没有影响。

mod moov;
mod plan;

pub use moov::{build_moov, mdat_data_start, mdat_header_len, needs_wide_offsets};
pub use plan::{MergeIndex, MergedTrack, SourceFile, plan, read_source_file};

#[cfg(test)]
use plan::{MDHD_DURATION, MVHD_DURATION, TKHD_DURATION, duration_of};

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
