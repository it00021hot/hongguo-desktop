//! 在线播放的内存流（`hongguo-stream://`）。
//!
//! 缓存按 **(vid, 清晰度档位)** 分条目：一集有几个档位就有几份互不相干的数据。
//! 于是切清晰度**不用碰上一档的任何字节**——正在播的旧 `<video>` 继续从
//! 自己那份 buffer 取数，直到它被换掉为止。
//!
//! ⚠️ 早期版本只按 vid 分条目，切档必须原地清空 buffer，再靠「代次号」去识别
//!    上一代遗留的 Range 请求。被清空的那份恰好是还在播的那份，`<video>` 下一发
//!    请求要么被拒、要么切出错位片段，立刻 `onError` —— 表现为「视频处理失败」
//!    黑屏。分条目之后这套机制没有存在意义，整块拿掉。
//!
//! 两条供数路径，谁先就绪走谁：
//! - **渐进**：注册了 [`ProgressiveStream`]（稀疏密文 + 解密计划）后即可应答，
//!   按明文 Range 等齐依赖的密文区间、惰性解密拼装（见 `serve_progressive`）。
//!   首帧只需头 + 尾 + moov 就位。
//! - **整集**：明文一次性原子写进 `store`，`size` 从 0 变成真实值的那一刻起
//!   全部字节就都在内存里，任意 Range 直接切。渐进路径不可用（CDN 不支持
//!   Range 等）时的回落。
//!
//! 渐进模式的开放式 Range 必须开窗口（默认 4MB，`HONGGUO_STREAM_WINDOW`
//! 可覆盖）：给到底就等于要等整集，渐进失去意义。整集模式保持「一次给到底」。

mod cache;
mod progressive;

pub use cache::StreamCache;
pub use progressive::{ProgressiveStream, serve};

/// 仅 play_service 的填充调度测试直接构造它；lib 构建无人按名引用，
/// 无条件再导出会吃 unused_imports 告警，故随测试配置再导出。
#[cfg(test)]
pub use progressive::ReaderDemand;
