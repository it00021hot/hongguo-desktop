//! AVFoundation 音轨控制与时钟；视频仅经 CoreMedia 软解帧显示层。

use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};
use objc2_foundation::{NSArray, NSString, NSURL};
use std::ffi::CStr;

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVAudioTimePitchAlgorithmSpectral: *const AnyObject;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}
// SAFETY: Public CMTime layout; Objective-C encodes this imported structure anonymously.
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
    pub const INVALID: Self = Self {
        value: 0,
        timescale: 0,
        flags: 0,
        epoch: 0,
    };
    pub fn secs(seconds: f64) -> Self {
        Self::ticks((seconds * 24_000.0).round() as i64, 24_000)
    }
    pub fn ticks(value: i64, timescale: i32) -> Self {
        Self {
            value,
            timescale,
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

pub fn item_timebase(item: &AnyObject) -> *mut crate::cv::OpaqueTimebase {
    // SAFETY: AVPlayerItem retains its read-only timebase.
    unsafe { msg_send![item, timebase] }
}
pub fn asset_with_url(url: &NSURL) -> Retained<AnyObject> {
    // SAFETY: Public AVURLAsset initializer; nil options are supported.
    unsafe {
        msg_send![msg_send![class(c"AVURLAsset"), alloc], initWithURL: url, options: std::ptr::null_mut::<AnyObject>()]
    }
}
pub fn item_with_asset(asset: &AnyObject) -> Retained<AnyObject> {
    // SAFETY: Public AVPlayerItem initializer retains the asset.
    unsafe { msg_send![msg_send![class(c"AVPlayerItem"), alloc], initWithAsset: asset] }
}
pub fn set_preferred_forward_buffer_duration(item: &AnyObject, seconds: f64) {
    let _: () = unsafe { msg_send![item, setPreferredForwardBufferDuration: seconds] };
}
pub fn queue_player(items: &NSArray<AnyObject>) -> Retained<AnyObject> {
    unsafe { msg_send![class(c"AVQueuePlayer"), queuePlayerWithItems: items] }
}
pub fn cancel_loading(asset: &AnyObject) {
    let _: () = unsafe { msg_send![asset, cancelLoading] };
}
pub fn clear_resource_loader_delegate(asset: &AnyObject) {
    let loader: *mut AnyObject = unsafe { msg_send![asset, resourceLoader] };
    if !loader.is_null() {
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = unsafe { msg_send![loader, setDelegate: nil, queue: nil] };
    }
}
pub fn set_spectral_pitch(item: &AnyObject) {
    let algorithm = unsafe { AVAudioTimePitchAlgorithmSpectral };
    let _: () = unsafe { msg_send![item, setAudioTimePitchAlgorithm: algorithm] };
}
pub fn item_status(item: &AnyObject) -> isize {
    unsafe { msg_send![item, status] }
}
pub fn pause(player: &AnyObject) {
    let _: () = unsafe { msg_send![player, pause] };
}
pub fn set_rate(player: &AnyObject, rate: f32) {
    let _: () = unsafe { msg_send![player, setRate: rate] };
}
pub fn set_volume(player: &AnyObject, volume: f32) {
    let _: () = unsafe { msg_send![player, setVolume: volume] };
}
pub fn current_time(player: &AnyObject) -> CmTime {
    unsafe { msg_send![player, currentTime] }
}
pub fn replace_item(player: &AnyObject, item: &AnyObject) {
    let _: () = unsafe { msg_send![player, replaceCurrentItemWithPlayerItem: item] };
}
pub fn seek_exact(player: &AnyObject, time: CmTime) {
    let _: () = unsafe {
        msg_send![player, seekToTime: time, toleranceBefore: CmTime::ZERO, toleranceAfter: CmTime::ZERO]
    };
}
pub fn custom_url(episode: &str) -> Option<Retained<NSURL>> {
    NSURL::URLWithString(&NSString::from_str(&format!(
        "hongguo-audio://localhost/{episode}.mp4"
    )))
}
pub fn current_item(player: &AnyObject) -> Option<&AnyObject> {
    let item: *mut AnyObject = unsafe { msg_send![player, currentItem] };
    // SAFETY: The caller retains the player while borrowing its current item.
    unsafe { item.as_ref() }
}
pub fn item_duration(item: &AnyObject) -> CmTime {
    unsafe { msg_send![item, duration] }
}
pub fn item_error(item: &AnyObject) -> Option<String> {
    let error: *mut AnyObject = unsafe { msg_send![item, error] };
    if error.is_null() {
        return None;
    }
    let text: Retained<NSString> = unsafe { msg_send![error, localizedDescription] };
    Some(text.to_string())
}
pub fn player_rate(player: &AnyObject) -> f32 {
    unsafe { msg_send![player, rate] }
}
pub fn set_muted(player: &AnyObject, muted: bool) {
    let _: () = unsafe { msg_send![player, setMuted: muted] };
}
pub fn item_likely_to_keep_up(item: &AnyObject) -> bool {
    unsafe { msg_send![item, isPlaybackLikelyToKeepUp] }
}
pub fn disable_auto_advance(player: &AnyObject) {
    let _: () = unsafe { msg_send![player, setActionAtItemEnd: 0isize] };
}
pub fn disable_wait_to_minimize_stalling(player: &AnyObject) {
    let _: () = unsafe { msg_send![player, setAutomaticallyWaitsToMinimizeStalling: false] };
}
pub fn play_immediately(player: &AnyObject, rate: f32) {
    let _: () = unsafe { msg_send![player, playImmediatelyAtRate: rate] };
}
