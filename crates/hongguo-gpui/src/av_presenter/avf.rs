//! AVFoundation 的最小 FFI 面（spike 专用）。
//!
//! 只包 [Phase 0](docs/gpui-video-pacing-plan.md) 要验证的四条指标用到的
//! 调用：`AVPlayer` / `AVPlayerItem` / `AVPlayerLayer` / `AVPlayerItemVideoOutput`。
//! 不引入 objc2-av-foundation 绑定 crate：类按名字从运行时取，方法用
//! `msg_send!` 直投；框架链接由 `#[link(name = "AVFoundation")]` 负责。
//!
//! `CMTime` 的 Objective-C 编码是匿名结构体 `{?=qiIq}`（不是 `{CMTime=...}`，
//! 本机实测 `-duration` 的签名就是 `{?=qiIq}`），编码写错 `msg_send!` 会在
//! debug 断言里 panic。

use std::ffi::CStr;
use std::ptr::NonNull;

use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVBuffer;
use objc2_foundation::{NSArray, NSString, NSURL};

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeVideo: *const AnyObject;
    /// `Spectral` 是 Apple 的变速不变调算法；不设置时它也默认是它，但显式
    /// 钉住才能让日志/验收有据可查。
    static AVAudioTimePitchAlgorithmSpectral: *const AnyObject;
}

/// CoreMedia `CMTime`（公开布局：value/timescale/flags/epoch）。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

unsafe impl Encode for CmTime {
    const ENCODING: Encoding = Encoding::Struct(
        "?",
        &[i64::ENCODING, i32::ENCODING, u32::ENCODING, i64::ENCODING],
    );
}

unsafe impl RefEncode for CmTime {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

impl CmTime {
    pub const ZERO: Self = Self {
        value: 0,
        timescale: 1,
        flags: 1,
        epoch: 0,
    };

    /// 24kHz 时基：1 帧 @25fps = 40ms 可精确表示。
    pub fn secs(seconds: f64) -> Self {
        Self {
            value: (seconds * 24_000.0).round() as i64,
            timescale: 24_000,
            flags: 1,
            epoch: 0,
        }
    }

    pub fn seconds(self) -> f64 {
        if self.timescale == 0 {
            0.0
        } else {
            self.value as f64 / self.timescale as f64
        }
    }
}

fn class(name: &CStr) -> &'static AnyClass {
    AnyClass::get(name).unwrap_or_else(|| panic!("缺少 Objective-C 类: {name:?}"))
}

/// `AVPlayer`，初始无 item（item 由测量线程按需挂上）。
pub fn player() -> Retained<AnyObject> {
    // SAFETY: AVPlayer 的 +new 是公开的 alloc/init 组合。
    unsafe { msg_send![class(c"AVPlayer"), new] }
}

pub fn player_layer(player: &AnyObject) -> Retained<AnyObject> {
    // SAFETY: 公开类方法 +playerLayerWithPlayer:，player 生命周期由调用方保证。
    unsafe { msg_send![class(c"AVPlayerLayer"), playerLayerWithPlayer: player] }
}

/// 自定义 scheme 的 asset（由 `loader` 的 delegate 供数）。
pub fn asset_with_url(url: &NSURL) -> Retained<AnyObject> {
    // SAFETY: AVURLAsset 公开初始化器；options 允许 nil。
    unsafe {
        msg_send![
            msg_send![class(c"AVURLAsset"), alloc],
            initWithURL: url,
            options: std::ptr::null_mut::<AnyObject>()
        ]
    }
}

pub fn item_with_asset(asset: &AnyObject) -> Retained<AnyObject> {
    // SAFETY: AVPlayerItem 公开初始化器。
    unsafe {
        msg_send![
            msg_send![class(c"AVPlayerItem"), alloc],
            initWithAsset: asset
        ]
    }
}

/// `AVQueuePlayer`：队列会预加载下一项，用来测切集接力。
pub fn queue_player(items: &NSArray<AnyObject>) -> Retained<AnyObject> {
    // SAFETY: 公开类方法 +queuePlayerWithItems:。
    unsafe { msg_send![class(c"AVQueuePlayer"), queuePlayerWithItems: items] }
}

pub fn advance_to_next_item(queue: &AnyObject) {
    // SAFETY: 公开实例方法。
    let _: () = unsafe { msg_send![queue, advanceToNextItem] };
}

/// 帧时间戳出口：`hasNewPixelBufferForItemTime:` 判定后由
/// [`frame_display_time`] 取走帧（同时消费掉“新帧”状态）。
pub fn video_output() -> Retained<AnyObject> {
    // SAFETY: attributes 传 nil 表示让 AVFoundation 自选像素格式；spike 只
    // 用时间戳，不读像素内容。
    unsafe {
        msg_send![
            msg_send![class(c"AVPlayerItemVideoOutput"), alloc],
            initWithPixelBufferAttributes: std::ptr::null_mut::<AnyObject>()
        ]
    }
}

pub fn add_output(item: &AnyObject, output: &AnyObject) {
    // SAFETY: 公开实例方法；output 由 item 持有。
    let _: () = unsafe { msg_send![item, addOutput: output] };
}

pub fn set_spectral_pitch(item: &AnyObject) {
    let algorithm = unsafe { AVAudioTimePitchAlgorithmSpectral };
    if algorithm.is_null() {
        return;
    }
    // SAFETY: 常量是框架导出的 NSString *；参数为 copy 属性，调用后不保留借用。
    let _: () = unsafe { msg_send![item, setAudioTimePitchAlgorithm: algorithm] };
}

pub fn audio_pitch_algorithm(item: &AnyObject) -> Option<String> {
    // SAFETY: 只读取 item 的属性，返回对象由 autorelease 池管理，Retained 接管。
    let algorithm: *mut AnyObject = unsafe { msg_send![item, audioTimePitchAlgorithm] };
    if algorithm.is_null() {
        return None;
    }
    let description: *mut AnyObject = unsafe { msg_send![algorithm, description] };
    let text: *mut NSString = description.cast();
    if text.is_null() {
        return None;
    }
    // SAFETY: description 返回 NSString（或 nil，已判空），生命周期覆盖本次拷贝。
    Some(unsafe { (*text).to_string() })
}

pub fn item_status(item: &AnyObject) -> isize {
    // SAFETY: 只读属性，返回 AVPlayerItemStatus 的 NSInteger 值。
    unsafe { msg_send![item, status] }
}

pub fn play(player: &AnyObject) {
    // SAFETY: 公开实例方法。
    let _: () = unsafe { msg_send![player, play] };
}

pub fn pause(player: &AnyObject) {
    // SAFETY: 公开实例方法。
    let _: () = unsafe { msg_send![player, pause] };
}

pub fn set_rate(player: &AnyObject, rate: f32) {
    // SAFETY: rate 是 float 属性。
    let _: () = unsafe { msg_send![player, setRate: rate] };
}

pub fn set_volume(player: &AnyObject, volume: f32) {
    // SAFETY: volume 是 0..=1 的 float 属性。
    let _: () = unsafe { msg_send![player, setVolume: volume] };
}

pub fn current_time(player: &AnyObject) -> CmTime {
    // SAFETY: 只读 CMTime 属性。
    unsafe { msg_send![player, currentTime] }
}

pub fn replace_item(player: &AnyObject, item: &AnyObject) {
    // SAFETY: 公开实例方法。
    let _: () = unsafe { msg_send![player, replaceCurrentItemWithPlayerItem: item] };
}

/// 零容差 seek：目标帧前的关键帧由系统解码补齐，落点精确到帧。
///
/// 这里用的是已废弃的 `seekToTime:toleranceBefore:toleranceAfter:`：spike
/// 不需要 completion block（block2 会引入新依赖），完成后由调用方轮询
/// `currentTime` 判定。
pub fn seek_exact(player: &AnyObject, to: CmTime) {
    // SAFETY: 公开实例方法；两个 tolerance 都是 CMTime 值类型。
    let _: () = unsafe {
        msg_send![
            player,
            seekToTime: to,
            toleranceBefore: CmTime::ZERO,
            toleranceAfter: CmTime::ZERO
        ]
    };
}

pub fn has_new_frame(output: &AnyObject, item_time: CmTime) -> bool {
    // SAFETY: 只读查询。
    unsafe { msg_send![output, hasNewPixelBufferForItemTime: item_time] }
}

/// 取走该时刻的帧并返回它的显示时间；帧本身只用来推进“新帧”状态，取完即释放。
pub fn frame_display_time(output: &AnyObject, item_time: CmTime) -> Option<CmTime> {
    let mut display = CmTime::ZERO;
    // SAFETY: copy 语义返回 +1 的 CVPixelBuffer；出参是本地 CMTime。
    let buffer: *mut CVBuffer = unsafe {
        msg_send![
            output,
            copyPixelBufferForItemTime: item_time,
            itemTimeForDisplay: &mut display
        ]
    };
    if buffer.is_null() {
        return None;
    }
    // SAFETY: 上面是 copy 语义，必须 CFRelease；NonNull 已判空。
    unsafe { drop(CFRetained::from_raw(NonNull::new_unchecked(buffer))) };
    Some(display)
}

pub fn nominal_fps(asset: &AnyObject) -> f32 {
    // SAFETY: 读 tracks 数组；asset 已 ready 时轨道已加载。
    let tracks: *mut AnyObject = unsafe { msg_send![asset, tracksWithMediaType: AVMediaTypeVideo] };
    if tracks.is_null() {
        return 0.0;
    }
    let count: usize = unsafe { msg_send![tracks, count] };
    if count == 0 {
        return 0.0;
    }
    let track: *mut AnyObject = unsafe { msg_send![tracks, objectAtIndex: 0usize] };
    if track.is_null() {
        return 0.0;
    }
    unsafe { msg_send![track, nominalFrameRate] }
}

pub fn duration(asset: &AnyObject) -> CmTime {
    // SAFETY: 只读 CMTime 属性。
    unsafe { msg_send![asset, duration] }
}

/// 自定义 scheme 的 URL（host 与 path 只用于区分不同的“集”，delegate 不解析）。
pub fn custom_url(episode: &str) -> Option<Retained<NSURL>> {
    NSURL::URLWithString(&NSString::from_str(&format!(
        "hongguo-spike://localhost/{episode}.mp4"
    )))
}

// —— 系统播放器壳需要的补充读取（Phase 1b） ——

use objc2_foundation::NSSize;

pub fn current_item(player: &AnyObject) -> Option<&AnyObject> {
    // SAFETY: currentItem 可空；调用期间 player 持有着 item。
    let item: *mut AnyObject = unsafe { msg_send![player, currentItem] };
    unsafe { item.as_ref() }
}

pub fn item_presentation_size(item: &AnyObject) -> (usize, usize) {
    // SAFETY: presentationSize 是只读 CGSize；未 ready 时为 0×0。
    let size: NSSize = unsafe { msg_send![item, presentationSize] };
    (size.width.max(0.0) as usize, size.height.max(0.0) as usize)
}

pub fn item_duration(item: &AnyObject) -> CmTime {
    // SAFETY: 只读 CMTime 属性。
    unsafe { msg_send![item, duration] }
}

pub fn item_error(item: &AnyObject) -> Option<String> {
    // SAFETY: error 可空；localizedDescription 为 NSString。
    let error: *mut AnyObject = unsafe { msg_send![item, error] };
    if error.is_null() {
        return None;
    }
    let description: *mut AnyObject = unsafe { msg_send![error, localizedDescription] };
    let text: *mut NSString = description.cast();
    if text.is_null() {
        return None;
    }
    // SAFETY: 返回字符串在调用期间有效。
    Some(unsafe { (*text).to_string() })
}

pub fn player_rate(player: &AnyObject) -> f32 {
    // SAFETY: 只读 float 属性。
    unsafe { msg_send![player, rate] }
}

pub fn set_muted(player: &AnyObject, muted: bool) {
    // SAFETY: 公开 bool 属性。
    let _: () = unsafe { msg_send![player, setMuted: muted] };
}

pub fn item_likely_to_keep_up(item: &AnyObject) -> bool {
    // SAFETY: 只读 bool 属性。
    unsafe { msg_send![item, isPlaybackLikelyToKeepUp] }
}

/// AVPlayerActionAtItemEndNone = 0：队列播完不自动跳下一项，由壳按
/// `PlayerEvent::Ended` 显式接力（连播锁定/剧终接力语义都在壳里）。
pub fn disable_auto_advance(player: &AnyObject) {
    // SAFETY: 公开 NSInteger 属性。
    let _: () = unsafe { msg_send![player, setActionAtItemEnd: 0isize] };
}
