//! ffmpeg 可选加速（分流链的调试层）：完整分流顺序见壳内 `service::transcode_service::pipeline`——**平台硬编（`platform`）→（仅当显式 `HONGGUO_ALLOW_FFMPEG=1`）ffmpeg**。
//! 默认关闭外部 ffmpeg 进程。打开调试开关后：HEVC → HEVC 转码可交给 ffmpeg，可用 NVENC/QSV/AMF/MF 硬编码接近实时；未打开且平台编码器也没有时转码做不了，直接报错。
//! 快速合并**不经过这里**（走 `hongguo_media::remux` 的纯 Rust 索引重写拼接，任何后端都不调它）。探测结果全局缓存，只查一次。

use std::path::PathBuf;
use std::process::Command;

use parking_lot::RwLock;

/// 常见安装位置（PATH 之外）。
#[cfg(target_os = "windows")]
const EXTRA_PATHS: &[&str] = &[
    r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
    r"C:\ffmpeg\bin\ffmpeg.exe",
    r"C:\ProgramData\chocolatey\bin\ffmpeg.exe",
];

#[cfg(target_os = "macos")]
const EXTRA_PATHS: &[&str] = &[
    // librempeg（带 bvc2）优先于 Homebrew 原版 ffmpeg
    "/opt/librempeg/bin/ffmpeg",
    "/opt/homebrew/bin/ffmpeg",
    "/usr/local/bin/ffmpeg",
    "/usr/bin/ffmpeg",
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const EXTRA_PATHS: &[&str] = &["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg"];

/// 硬件编码器，从快到慢逐个试。能编出帧就算硬件。
const HARDWARE_ENCODERS: &[&str] = &["hevc_nvenc", "hevc_qsv", "hevc_amf", "hevc_mf"];

/// 可用的软件 HEVC 编码器。
const SOFTWARE_ENCODERS: &[&str] = &["libx265", "hevc_mf"];

/// 递归扫描的层数上限：winget 包目录形如
/// `Packages/Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe/ffmpeg-9.0.2-full_build/bin/ffmpeg.exe`（中间隔两层），给 6 足够宽松又不至于空转。
#[cfg(target_os = "windows")]
const WINGET_SCAN_DEPTH: usize = 6;

/// 外部 ffmpeg 调试 fallback 是否打开。
///
/// 默认关闭：默认播放和倍速不拉起外部 ffmpeg 进程。仅当显式设置
/// `HONGGUO_ALLOW_FFMPEG`（非空，且不是 `0` / `false` / `off` / `no`），
/// 并且未设置 `HONGGUO_NO_FFMPEG` 时为真。`HONGGUO_NO_FFMPEG` 仍强制关闭，
/// 兼容此前无 ffmpeg 验收开关。
pub fn ffmpeg_explicitly_allowed() -> bool {
    ffmpeg_allowed_from(
        std::env::var_os("HONGGUO_ALLOW_FFMPEG"),
        std::env::var_os("HONGGUO_NO_FFMPEG"),
    )
}

fn env_flag_on(value: Option<&std::ffi::OsStr>) -> bool {
    let Some(raw) = value else {
        return false;
    };
    let Some(s) = raw.to_str() else {
        // 非 UTF-8：按「已设置」处理，与其余 HONGGUO_* 的 is_some 约定一致。
        return true;
    };
    let t = s.trim();
    !t.is_empty()
        && t != "0"
        && !t.eq_ignore_ascii_case("false")
        && !t.eq_ignore_ascii_case("off")
        && !t.eq_ignore_ascii_case("no")
}

fn ffmpeg_allowed_from(
    allow: Option<impl AsRef<std::ffi::OsStr>>,
    no_ffmpeg: Option<impl AsRef<std::ffi::OsStr>>,
) -> bool {
    if no_ffmpeg.is_some() {
        return false;
    }
    env_flag_on(allow.as_ref().map(|v| v.as_ref()))
}

fn detect() -> Option<PathBuf> {
    // 默认关闭外部 ffmpeg：默认播放/倍速走进程内 vvdec 或平台编码器。
    // 调试 fallback 必须显式 HONGGUO_ALLOW_FFMPEG=1；HONGGUO_NO_FFMPEG 仍强制关闭。
    if !ffmpeg_explicitly_allowed() {
        log::info!(
            "[FFmpeg] 默认关闭外部 ffmpeg 进程；调试 fallback 需显式设置 HONGGUO_ALLOW_FFMPEG=1"
        );
        return None;
    }
    // 0) 显式覆盖 / 用户级 librempeg（带 bvc2）
    if let Some(p) = std::env::var_os("HONGGUO_FFMPEG") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let local = PathBuf::from(home).join(".local/librempeg/bin/ffmpeg");
        if local.exists() {
            return Some(local);
        }
    }
    // 1) PATH
    if let Some(p) = probe_command("ffmpeg") {
        return Some(p);
    }
    // 2) 常见安装位置
    for candidate in EXTRA_PATHS {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(path);
        }
    }
    // 3) winget 包目录
    find_in_winget()
}

/// 扫 `%LOCALAPPDATA%\Microsoft\WinGet\Packages` 下名字带 ffmpeg 的包。
/// winget 装 ffmpeg 靠把 `WinGet\Links` 写进**用户** PATH——早就启动着的进程环境变量不会更新，表现是
/// 「明明装了，应用却说没装，重启一下应用又好了」；按包目录找一遍绕开这个坑。
#[cfg(target_os = "windows")]
fn find_in_winget() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let packages = PathBuf::from(local)
        .join("Microsoft")
        .join("WinGet")
        .join("Packages");
    let exe = "ffmpeg.exe";

    for entry in std::fs::read_dir(&packages).ok()?.flatten() {
        let dir = entry.path();
        if !dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().contains("ffmpeg"))
        {
            continue;
        }
        if let Some(found) = search_dir(&dir, exe, WINGET_SCAN_DEPTH) {
            return Some(found);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn search_dir(dir: &std::path::Path, file_name: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path.file_name().is_some_and(|n| n == file_name) {
                return Some(path);
            }
        } else if let Some(found) = search_dir(&path, file_name, depth - 1) {
            return Some(found);
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn find_in_winget() -> Option<PathBuf> {
    None
}

/// GUI 进程拉起控制面子进程会闪黑窗：Windows 侧一律 CREATE_NO_WINDOW
/// （启动预热就会跑 `-version` / `-encoders` / 试编一帧）。
#[cfg(target_os = "windows")]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 尝试执行 `ffmpeg -version`，成功则返回其路径。
#[cfg(target_os = "windows")]
fn probe_command(name: &str) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    let out = Command::new(name)
        .arg("-version")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status.success().then(|| PathBuf::from(name))
}

#[cfg(not(target_os = "windows"))]
fn probe_command(name: &str) -> Option<PathBuf> {
    let out = Command::new(name).arg("-version").output().ok()?;
    out.status.success().then(|| PathBuf::from(name))
}

/// 选中的 HEVC 编码器。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoder {
    /// ffmpeg 的编码器名，转码时直接当 `-c:v` 的值
    pub name: &'static str,
    /// 是否**真的**走硬件。绝大多数编码器能编出帧就必然是硬件（nvenc/qsv/amf 直接对接厂商驱动）；
    /// 唯一例外 `hevc_mf`——`-hw_encoding` 默认 **false**，编出来的是微软软件 MediaFoundation 编码，当硬件报会让人误以为有 GPU 加速。
    pub hardware: bool,
}

/// 探测结果的缓存。用 `RwLock` 而不是 `OnceLock`：装完 ffmpeg 不重启应用看不到变化，
/// 提供「重新检测」就得能丢弃缓存（`OnceLock` 没有清空接口）；探测一次要跑 `-version` + `-encoders` + 试编，全套一到两秒。
/// 外层 `Option` 区分「还没探过」与「探过、没找到」。
type Cache<T> = RwLock<Option<Option<T>>>;

static FFMPEG: Cache<PathBuf> = RwLock::new(None);
static ENCODER: Cache<Encoder> = RwLock::new(None);

/// 丢掉全部探测缓存，下次读取时重新探测。
pub fn reprobe() {
    *FFMPEG.write() = None;
    *ENCODER.write() = None;
}

/// 探测本机 ffmpeg 路径。找不到返回 `None`。
pub fn ffmpeg_path() -> Option<PathBuf> {
    FFMPEG.write().get_or_insert_with(detect).clone()
}

/// 选一个可用的 HEVC 编码器。
pub fn hevc_encoder() -> Option<Encoder> {
    let mut slot = ENCODER.write();
    if slot.is_none() {
        *slot = Some(pick_encoder());
    }
    slot.clone().flatten()
}

/// 探测顺序：**先挑真的硬件编码器，挑不到再退软件**。「列表里有」不等于「能用」（无 N 卡时 `hevc_nvenc` 直接失败），逐个实际试编一帧。
fn pick_encoder() -> Option<Encoder> {
    let ffmpeg = ffmpeg_path()?;

    let list = run(&ffmpeg, &["-hide_banner", "-encoders"])?;
    let listed = |enc: &str| list.contains(enc);

    for enc in HARDWARE_ENCODERS {
        if !listed(enc) {
            continue;
        }
        if let Some(found) = try_encoder(&ffmpeg, enc, true) {
            return Some(found);
        }
        log::debug!("[FFmpeg] 硬件编码器不可用，跳过: {enc}");
    }

    // 软件编码仍实际试编，不能只凭编码器列表判定。
    for enc in SOFTWARE_ENCODERS {
        if !listed(enc) {
            continue;
        }
        if let Some(found) = try_encoder(&ffmpeg, enc, false) {
            return Some(found);
        }
        log::debug!("[FFmpeg] 编码器不可用，跳过: {enc}");
    }

    // 静态构建有时不列全，最后裸试一次 libx265
    try_encoder(&ffmpeg, "libx265", false)
}

/// 试编一帧，返回带硬件标记的结果；`force_hardware` 为真时额外加 `-hw_encoding 1`，只有它能编出帧才算真硬件。
/// 测试帧用 256×256 且**不能更小**：nvenc 对低于最小支持分辨率的帧直接拒绝（`Frame Dimension less than the minimum supported value`，
/// GTX 1650 + 617 驱动上 64×64 实测触发），会把完全能用的 nvenc 误判成不可用、整机被压在软解上；256×256 对各家下限都安全，耗时毫秒级。
fn try_encoder(ffmpeg: &PathBuf, encoder: &'static str, force_hardware: bool) -> Option<Encoder> {
    let mut args: Vec<&str> = vec![
        "-hide_banner",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=256x256:d=0.1",
        "-frames:v",
        "1",
        "-c:v",
        encoder,
    ];
    if force_hardware && encoder == "hevc_mf" {
        args.push("-hw_encoding");
        args.push("1");
    }
    args.extend(["-f", "null", "-"]);

    run(ffmpeg, &args).map(|_| Encoder {
        name: encoder,
        hardware: force_hardware,
    })
}

/// 执行 ffmpeg，成功返回 stdout。
fn run(ffmpeg: &PathBuf, args: &[&str]) -> Option<String> {
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new(ffmpeg);
    cmd.args(args);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// 当前解码/编码后端的可读描述，供内部日志与转码流水线使用。
/// 只留日志真要用的两样（编码器名与整条链路）：**不**带 ffmpeg 路径（[`ffmpeg_path`] 给）与「是否硬件」（在 [`Encoder::hardware`] 上），避免两处口径。
pub struct BackendInfo {
    /// 选中的 HEVC 编码器名
    pub encoder: String,
    /// 实际生效的转码方式
    pub transcode_with: String,
}

/// 取当前后端信息。
pub fn backend_info() -> BackendInfo {
    match (ffmpeg_path(), hevc_encoder()) {
        (Some(_), Some(enc)) => BackendInfo {
            encoder: enc.name.to_string(),
            transcode_with: format!("ffmpeg ({})", enc.name),
        },
        _ => BackendInfo {
            encoder: String::new(),
            transcode_with: "none (ffmpeg not found)".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_does_not_panic() {
        let _ = ffmpeg_path();
    }

    #[test]
    fn the_test_frame_exceeds_encoder_minimums() {
        // nvenc 拒绝低于最小分辨率的帧（GTX 1650 + 617 驱动实测 64×64 直接 InitializeEncoder failed）——
        // 试编帧太小的后果是「明明有硬编却报软件」，钉住分辨率别再改小
        let src = include_str!("probe.rs");
        assert!(
            src.contains("color=c=black:s=256x256"),
            "试编测试帧必须 ≥256×256，当前源码里找不到该尺寸"
        );
    }

    #[test]
    fn backend_info_has_transcode_target() {
        let info = backend_info();
        assert!(!info.transcode_with.is_empty(), "必须说明当前用什么转码");
    }

    #[test]
    fn backend_info_matches_detection() {
        let info = backend_info();
        if ffmpeg_path().is_none() {
            assert_eq!(info.encoder, "", "无 ffmpeg 时不应报告编码器");
            assert!(
                info.transcode_with.contains("none"),
                "无 ffmpeg 时应说明没有转码链路"
            );
        } else {
            assert!(!info.encoder.is_empty(), "有 ffmpeg 时应选到编码器");
        }
    }

    #[test]
    fn a_discovered_path_must_actually_run() {
        // 「找得到」不等于「能用」：缺 DLL、被安全软件隔离、指向别的同名程序都可能，后面每一步以「转码失败」形式报出。
        // PATH 分支返回的是**裸名** `ffmpeg`（由系统解析）不是完整路径，只能验「跑不跑得起来」，不能对返回值做 `exists()`
        let Some(p) = ffmpeg_path() else { return };
        let out = Command::new(&p).arg("-version").output();
        assert!(
            out.as_ref().is_ok_and(|o| o.status.success()),
            "探测到的 ffmpeg 跑不起来: {}",
            p.display()
        );
    }

    #[test]
    fn ffmpeg_is_off_by_default_and_requires_explicit_allow() {
        assert!(
            !ffmpeg_allowed_from(None::<&str>, None::<&str>),
            "默认不得打开外部 ffmpeg"
        );
        assert!(
            ffmpeg_allowed_from(Some("1"), None::<&str>),
            "HONGGUO_ALLOW_FFMPEG=1 应打开调试 fallback"
        );
        assert!(
            ffmpeg_allowed_from(Some("yes"), None::<&str>),
            "非空真值应打开"
        );
        assert!(
            !ffmpeg_allowed_from(Some(""), None::<&str>),
            "空字符串不算打开"
        );
        assert!(!ffmpeg_allowed_from(Some("0"), None::<&str>));
        assert!(!ffmpeg_allowed_from(Some("false"), None::<&str>));
        assert!(!ffmpeg_allowed_from(Some("OFF"), None::<&str>));
        assert!(!ffmpeg_allowed_from(Some("no"), None::<&str>));
        assert!(
            !ffmpeg_allowed_from(Some("1"), Some("1")),
            "HONGGUO_NO_FFMPEG 必须压过允许开关"
        );
        assert!(
            !ffmpeg_allowed_from(Some("1"), Some("")),
            "HONGGUO_NO_FFMPEG 只要存在就强制关闭"
        );
    }

    #[test]
    fn ffmpeg_path_is_none_on_the_default_path() {
        // 本机即使装着 ffmpeg / librempeg，默认探测也必须是 None。
        // 测试进程若已显式打开开关则跳过，以免误伤调试环境。
        if ffmpeg_explicitly_allowed() {
            return;
        }
        assert!(ffmpeg_path().is_none(), "默认路径不得暴露外部 ffmpeg");
    }

    /// winget 扫描：包目录里 ffmpeg 埋在两层下必须能挖出来；不走真实 `%LOCALAPPDATA%`（取决于装没装），自己搭目录树验扫描逻辑本身。
    #[cfg(target_os = "windows")]
    #[test]
    fn winget_scan_finds_a_buried_executable() {
        let root = std::env::temp_dir().join(format!("hg-winget-{}", std::process::id()));
        let pkg = root.join("Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe");
        let deep = pkg.join("ffmpeg-9.0.2-full_build").join("bin");
        std::fs::create_dir_all(&deep).unwrap();
        let target = deep.join("ffmpeg.exe");
        std::fs::write(&target, b"stub").unwrap();

        let other = root.join("SomeOther.Package_abc");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("ffmpeg.exe"), b"stub").unwrap();

        assert_eq!(
            search_dir(&pkg, "ffmpeg.exe", WINGET_SCAN_DEPTH),
            Some(target)
        );
        assert_eq!(search_dir(&pkg, "ffmpeg.exe", 1), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
