//! 播放解码能力探测：本机 WebView2 能否解 HEVC——回答「在线播放能不能把 HEVC 系流直接喂给 `<video>`」（转码侧走哪条路见 [`super::probe`]）。
//! Windows：Chromium/WebView2 的 HEVC 解码走 Media Foundation，解码 MFT 来自系统「HEVC 视频扩展」——MFT 不在位则 h265/bytevc1 一律黑屏
//! （容器解析正常、样本照常缓冲，唯独参数集送不进解码器，MediaError code=3 或长时间 `videoWidth === 0`）；判据与平台转码探测同源（[`crate::platform`]）。macOS WKWebView 原生支持 HEVC，恒可解。
//! `HONGGUO_NO_HEVC` 强制按不可解处理（对齐 `HONGGUO_NO_FFMPEG` / `HONGGUO_NO_PLATFORM` 约定），开发机即可全链验收无扩展机器的行为。

use parking_lot::RwLock;

static PLAYBACK: RwLock<Option<bool>> = RwLock::new(None);

/// 本机 WebView2 能否解 HEVC。结果进程级缓存。
pub fn hevc_playable() -> bool {
    let mut slot = PLAYBACK.write();
    if slot.is_none() {
        *slot = Some(probe());
    }
    slot.unwrap_or(true)
}

/// 丢弃缓存重探。装完扩展后不必重启应用即可拿到新结论（安装命令的返回值）。
pub fn redetect() -> bool {
    *PLAYBACK.write() = None;
    hevc_playable()
}

fn probe() -> bool {
    if std::env::var_os("HONGGUO_NO_HEVC").is_some() {
        log::info!("[Playback] HONGGUO_NO_HEVC 已设置，本次按无 HEVC 处理");
        return false;
    }
    crate::platform::hevc_decode_available()
}
