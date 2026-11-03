//! 拼接产物的 `moov` 重建：快速合并不解码不编码，但**必须重写索引**——把 N 个各自从 t=0 起的样本表拼成一条 0 起连续时间轴（字节级拼接做不到）。
//! 设计决定：**每样本一个 chunk**（`stsc` 只有一条 `(1,1)`，免处理各集 stsc 不一致）；**交织顺序照抄源文件**（按源偏移归并排序，精确复刻原排布）；
//! **`ctts` offset 原样照搬**（合成时间与解码时间同步平移、差值不变，额外加起点偏移会让 B 帧错位逐集累积）；**逐集保留 `edts/elst`**（AAC priming 补不掉，源 edit 的 media_time 平移到集起点）。
//! `mvex` / `udta` 丢弃；`edts` 是时间轴语义的一部分必须保留；`moov` 字节组装在 [`crate::mp4`]，快速合并与重新封装共用。

mod plan;

pub use plan::{SourceFile, plan, read_source_file};

#[cfg(test)]
use plan::{MDHD_DURATION, MVHD_DURATION, TKHD_DURATION, duration_of};

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
