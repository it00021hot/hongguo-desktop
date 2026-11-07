//! ByteVC2（`bvc2`）解码能力：依赖带 librempeg `bvc2` 解码器的 ffmpeg。
//! WebView / 系统播放器不解 `bvc2`——有解码器时走转码兼容路径才算「能播」。

use std::process::Command;
use std::sync::OnceLock;

use super::super::ffmpeg::probe::ffmpeg_path;

/// 本机 ffmpeg 是否带 `bvc2` 解码器（librempeg 8.1+）。
pub fn bytevc2_decode_available() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(probe)
}

fn probe() -> bool {
    if std::env::var_os("HONGGUO_NO_BYTEVC2").is_some() {
        log::info!("[Playback] HONGGUO_NO_BYTEVC2 已设置，本次按无 bytevc2 处理");
        return false;
    }
    let Some(ffmpeg) = ffmpeg_path() else {
        return false;
    };
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-decoders"]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(super::super::ffmpeg::probe::CREATE_NO_WINDOW);
    }
    let Ok(out) = cmd.output() else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    // librempeg 行形如：` V..... bvc2                 BVC2`
    let ok = text.lines().any(|line| {
        let t = line.trim_start();
        t.contains(" bvc2 ") || t.ends_with(" bvc2") || t.split_whitespace().any(|w| w == "bvc2")
    });
    log::info!("[Playback] ffmpeg bvc2 解码器：{}", if ok { "有" } else { "无" });
    ok
}

#[cfg(test)]
mod tests {
    #[test]
    fn probe_does_not_panic() {
        let _ = super::bytevc2_decode_available();
    }
}
