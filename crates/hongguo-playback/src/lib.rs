//! 播放引擎（hongguo-playback crate）——§5.3 帧流的正式落地。
//!
//! 输入 `(vid, 档位)`（流已由调用方经 `play_service` 注册进 hongguo-stream
//! 缓存），输出两条：**视频帧**（`latest_frame` 槽；macOS 为 NV12
//! CVPixelBuffer 零拷贝，Windows 为 NV12 系统内存）与**扬声器声音**
//! （cpal 输出）。音频硬件的可闻消费位置是主时钟（扣设备引线、含倍速
//! 换算），视频按源 PTS 对表上屏——与 Tauri 侧 `<video>` 同一「媒体自己走
//! 时间轴」的语义，同步逻辑不进宿主。
//!
//! ```text
//! serve() 按样本表逐段取明文 ─┬─ 视频样本 → 平台 HEVC 解码(macOS VT / Windows MF)
//!                             │   → 帧槽 ─→ 渲染（surface 零拷贝 / NV12→RGBA）
//!                             └─ 音频样本 → 平台 AAC 解码(AudioToolbox / MF)
//!                                → WSOLA 倍速 → 环形缓冲（水位状态机）→ cpal → 时钟
//! ```
//!
//! 取流编排（prepare/prefetch/预算升格）留在壳侧的 `play_service`：引擎与
//! 具体壳、具体编排入口解耦，只认「serve 能读到的流」。
//!
//! 生命周期：一次 [`open`](player::open) 对应一集（对齐 Tauri 切集重挂载的
//! 语义——连播/换剧就是换一个引擎实例）；句柄 Drop 即停（任务 abort、音频
//! 设备释放）。播放中 seek/倍速/音量/静音走 [`PlayerHandle`](player::PlayerHandle)。

mod audio;
pub mod player;
mod resample;
mod source;
#[cfg(test)]
mod source_tests;
mod video;
mod wsola;

pub use player::{OpenOptions, PlaySource, PlayerEvent, PlayerHandle, open};
