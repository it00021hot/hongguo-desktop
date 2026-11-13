//! 播放解码能力服务：HEVC 视频扩展的按需下载安装。
//!
//! 安装包不捆绑外部二进制（与 tauri.conf 的 longDescription 一致）——缺扩展
//! 的机器由应用现场下载微软商店签名的官方扩展包安装。用户侧手动指引见
//! `docs/faq.md`。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use hongguo_api::stream_pick::DecoderCapability;
use hongguo_common::model::ProxyConfig;
use hongguo_common::{AppError, AppResult};

static BUILTIN_PLAYBACK: AtomicBool = AtomicBool::new(false);

/// 壳在启动时声明：它的播放器会把选中的流交给内置软解器直接播放。
///
/// 这是壳的能力而不是机器的能力，所以由壳来说：声明之后播放选流只收内置解码器能解的编码，
/// 不再看系统有没有 HEVC 解码器，也不再把 H.264 当候选。没声明的壳（直播放还没接通的那些）
/// 保持原来按系统能力选流的口径——在它们能直接上屏之前改选流，只会选到播不出来的流。
pub fn enable_builtin_playback() {
    BUILTIN_PLAYBACK.store(true, Ordering::Relaxed);
}

/// 这个壳的播放能不能放 HEVC——GPUI「装 HEVC 视频扩展」引导的数据源。
///
/// 声明了内置软解（见 [`enable_builtin_playback`]）就恒为真：HEVC 都交内置解码器直播放，
/// 系统有没有扩展无关紧要；这时还走系统播放的只剩本地 H.264 成品，也不需要扩展。
/// 没声明时（GPUI Windows 默认）才看系统 HEVC 解码器（[`hevc_playable`]）。
pub fn hevc_playback_ready() -> bool {
    hevc_ready_for(BUILTIN_PLAYBACK.load(Ordering::Relaxed), hevc_playable)
}

fn hevc_ready_for(builtin: bool, system_hevc: impl FnOnce() -> bool) -> bool {
    builtin || system_hevc()
}

/// 在线与预取共用的选流能力。
pub(crate) fn playback_cap() -> DecoderCapability {
    let bytevc2 = hongguo_media_platform::capability::bytevc2_decode_available();
    // 内置模式不读系统 HEVC 探测：结论用不上，探测本身（Windows 上要枚举解码器）也没必要触发。
    cap_for(
        BUILTIN_PLAYBACK.load(Ordering::Relaxed),
        hevc_playable,
        bytevc2,
    )
}

fn cap_for(builtin: bool, system_hevc: impl FnOnce() -> bool, bytevc2: bool) -> DecoderCapability {
    if builtin {
        DecoderCapability::builtin().with_bytevc2(bytevc2)
    } else {
        DecoderCapability::from_probes(system_hevc(), false)
    }
}

/// 这个编码的流是否交给壳的直播放。`codec` 可以是接口的编码名，也可以是 MP4 四字码。
/// 只有声明了内置软解的壳上才可能为真；没声明的壳全部交系统播放器。
pub fn plays_direct(codec: &str) -> bool {
    direct_for(BUILTIN_PLAYBACK.load(Ordering::Relaxed), codec)
}

fn direct_for(builtin: bool, codec: &str) -> bool {
    builtin && hongguo_api::stream_pick::builtin_decodable(codec)
}

/// 本机 HEVC 播放能力（选流模式的裁决输入，见
/// `hongguo_api::stream_pick::DecoderCapability`）。壳层统一从这里取，
/// 不直接依赖平台层。
pub fn hevc_playable() -> bool {
    hongguo_media_platform::capability::playback::hevc_playable()
}

/// HEVC 视频扩展 appx 的下载直链与期望 SHA-256。换镜像时同步改这两个常量；
/// 哈希不一致一律拒装——防链接被换、传输损坏。
const HEVC_APPX_URL: &str =
    "https://gitee.com/it00021hot/hongguo-desktop/raw/main/HEVC视频扩展-双击安装.appx";
const HEVC_APPX_SHA256: &str = "46695112aa70a05faf71c69ecf4ff0502a3b82d152168d3ffb67219eafe51b85";

/// 按需下载并安装 HEVC 视频扩展，返回安装后的能力重探结果。
/// `Add-AppxPackage` 用户级安装（免管理员）；无论成败都清理临时文件。
pub async fn install_hevc_extension(proxy: ProxyConfig) -> AppResult<bool> {
    let path = temp_path()?;
    download(&path, proxy).await?;

    // PowerShell 会话可能跑几秒，扔阻塞线程池，不占 async 工作线程
    let install_path = path.clone();
    let installed = crate::runtime::spawn_blocking(move || verify_and_install(&install_path))
        .await
        .map_err(|e| AppError::Media(format!("HEVC 扩展安装任务异常: {e}")))?;
    // 成败都清临时文件；安装失败时错误向上抛给前端展示
    let _ = hongguo_store::paths::qa_remove_file(&path);
    installed?;

    Ok(hongguo_media_platform::capability::playback::redetect())
}

fn temp_path() -> AppResult<PathBuf> {
    let mut p = std::env::temp_dir();
    p.push("hongguo-hevc-extension.appx");
    let _ = hongguo_store::paths::qa_remove_file(&p);
    Ok(p)
}

async fn download(path: &Path, proxy: ProxyConfig) -> AppResult<()> {
    // 复用应用的代理配置：微软源/镜像源在部分网络下同样需要代理
    hongguo_api::client::refuse_external_send(HEVC_APPX_URL)?;
    let client = hongguo_api::client::build_client(&proxy)?;
    let bytes = client
        .get(HEVC_APPX_URL)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .and_then(|resp| resp.error_for_status())
        .map_err(|e| AppError::Network(format!("下载 HEVC 扩展失败: {e}")))?
        .bytes()
        .await
        .map_err(|e| AppError::Network(format!("下载 HEVC 扩展失败: {e}")))?;
    hongguo_store::paths::qa_write(path, &bytes).map_err(|e| AppError::Io(e.to_string()))
}

fn verify_and_install(path: &Path) -> AppResult<()> {
    let actual = sha256_hex(path)?;
    if !actual.eq_ignore_ascii_case(HEVC_APPX_SHA256) {
        return Err(AppError::Media(format!(
            "HEVC 扩展包校验失败（SHA-256 不匹配）：{actual}"
        )));
    }
    run_add_appx(path)
}

fn sha256_hex(path: &Path) -> AppResult<String> {
    use std::io::Read;

    use sha2::{Digest, Sha256};

    let mut file = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| AppError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// `Add-AppxPackage` 走 PowerShell：失败时把 stderr 原文带回（含 0x 开头的
/// 错误码，如侧载被组策略禁用的 0x80073CFF），前端据此展示手动指引。
#[cfg(target_os = "windows")]
fn run_add_appx(path: &Path) -> AppResult<()> {
    use std::os::windows::process::CommandExt;

    // 不弹控制台窗口（对齐 ffmpeg 侧的 CREATE_NO_WINDOW）
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = format!(
        "Add-AppxPackage -Path '{}' -ErrorAction Stop",
        path.display()
    );
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| AppError::Io(format!("启动 PowerShell 失败: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    Err(AppError::Media(format!("HEVC 扩展安装失败: {detail}")))
}

#[cfg(not(target_os = "windows"))]
fn run_add_appx(_path: &Path) -> AppResult<()> {
    Err(AppError::InvalidArgs(
        "HEVC 视频扩展只需在 Windows 上安装（macOS 原生支持 HEVC）".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_playback_selects_by_the_builtin_decoders_and_never_probes_the_system() {
        let cap = cap_for(true, || panic!("内置模式不该探测系统 HEVC"), true);
        assert_eq!(cap, DecoderCapability::builtin());
        // ByteVC2 解码器被显式关掉时如实带出去，选流才不会挑到解不了的流。
        assert!(!cap_for(true, || true, false).bytevc2);
    }

    #[test]
    fn shells_without_direct_playback_keep_selecting_by_system_capability() {
        assert_eq!(
            cap_for(false, || false, true),
            DecoderCapability::from_probes(false, false)
        );
        assert!(!cap_for(false, || true, true).builtin_only);
        // 系统播放器解不了 ByteVC2、播放也不转码：没声明内置软解的壳不收它。
        assert!(!cap_for(false, || true, true).bytevc2);
    }

    #[test]
    fn playback_modes_offer_only_the_gears_their_route_can_consume() {
        use hongguo_api::stream_pick::{list_definitions, pick_stream_at};
        let model = serde_json::json!({ "video_list": [
            { "main_url": "h264", "video_meta": { "codec_type": "h264", "definition": "1080p" } },
            { "main_url": "bvc2", "video_meta": { "codec_type": "bytevc2", "definition": "720p" } },
            { "main_url": "hevc", "video_meta": { "codec_type": "bytevc1", "definition": "480p" } }
        ] });
        for (mode, expected) in [(false, vec![1080, 480]), (true, vec![720, 480])] {
            let cap = cap_for(mode, || true, true);
            let menu: Vec<_> = list_definitions(&model, cap)
                .iter()
                .map(|gear| gear.value)
                .collect();
            assert_eq!(menu, expected);
            let chosen = pick_stream_at(&model, None, cap).unwrap();
            assert_eq!(chosen.definition, expected[0]);
        }
    }

    #[test]
    fn a_bytevc2_only_episode_requires_a_real_decoder_and_an_explicit_playback_route() {
        use hongguo_api::stream_pick::pick_stream_at;
        let model = serde_json::json!({ "video_list": [
            { "main_url": "bvc2", "video_meta": { "codec_type": "bytevc2", "definition": "720p" } }
        ] });
        for mode in [false, true] {
            for available in [false, true] {
                let cap = cap_for(mode, || false, available);
                let chosen = pick_stream_at(&model, None, cap);
                assert_eq!(chosen.is_some(), mode && available);
            }
        }
    }

    #[test]
    fn builtin_playback_sends_hevc_and_bytevc2_to_the_direct_player() {
        for codec in ["bytevc1", "h265", "hvc1", "hev1", "bytevc2", "bvc2"] {
            assert!(direct_for(true, codec), "{codec} 应走直播放");
        }
        for codec in ["h264", "avc1", ""] {
            assert!(!direct_for(true, codec), "{codec:?} 不是内置解码器能解的");
        }
    }

    #[test]
    fn builtin_playback_needs_no_hevc_extension_and_never_probes_for_one() {
        assert!(hevc_ready_for(true, || panic!("内置模式不该探测系统 HEVC")));
        assert!(
            !hevc_ready_for(false, || false),
            "未声明的壳仍按系统能力提示装扩展"
        );
        assert!(hevc_ready_for(false, || true));
    }

    #[test]
    fn without_builtin_playback_nothing_is_direct() {
        for codec in ["bytevc2", "bytevc1", "h264"] {
            assert!(
                !direct_for(false, codec),
                "未声明的壳把 {codec} 交给系统播放器"
            );
        }
    }
}
