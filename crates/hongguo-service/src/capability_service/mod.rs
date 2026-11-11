//! 播放解码能力服务：HEVC 视频扩展的按需下载安装。
//!
//! 安装包不捆绑外部二进制（与 tauri.conf 的 longDescription 一致）——缺扩展
//! 的机器由应用现场下载微软商店签名的官方扩展包安装。用户侧手动指引见
//! `docs/faq.md`。

use std::path::{Path, PathBuf};

use hongguo_common::model::ProxyConfig;
use hongguo_common::{AppError, AppResult};

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
