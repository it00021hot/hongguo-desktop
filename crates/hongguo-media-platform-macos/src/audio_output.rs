//! macOS 音频输出：AudioToolbox **AudioQueue**（output-only）。
//!
//! ## 为什么不用 AudioUnit / cpal
//!
//! 本机实测（macOS 15.7.7，Intel iMac）逐 API 二分：
//!
//! | 调用 | 麦克风授权 |
//! | --- | --- |
//! | `AudioComponentInstanceNew`（DefaultOutput） | **触发**（只创建就触发，与格式/回调/初始化无关） |
//! | `AudioComponentInstanceNew`（HALOutput） | **触发** |
//! | `AudioQueueNewOutput` | **不触发** |
//!
//! 也就是说系统把「创建输出 AudioUnit」一并当成音频设备访问来审，而
//! AudioQueue 这条更高层的输出路径不在此列。cpal 0.15 的 macOS 后端内部
//! 正是 `AudioUnit::new` + `AudioUnitInitialize`，因此每次起播都会弹授权。
//!
//! 播放器只要输出、从不碰输入，所以这里用 AudioQueue：它天然是「往设备
//! 送 PCM」的队列模型，没有输入作用域概念。
//!
//! ## 时钟口径
//!
//! AudioQueue 的回调不提供 playback/callback 时间戳（AudioUnit 才有）。
//! 这里用**一个缓冲的时长**作为设备引线：队列里通常压着 1–2 个缓冲，
//! 刚填进去的这一块要到下一个周期才出声，量与 AudioUnit 的
//! `in_number_frames` 同阶（约 20–40ms），足够让时钟不跑到耳朵前面。

use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use hongguo_common::{AppError, AppResult};
use hongguo_media_platform_api::audio_output::{AudioOutputHost, AudioOutputStream, OutputFormat};

// ————————————————————————————————————————————————————————————
// CoreAudio 属性查询：默认输出设备的混音格式
// ————————————————————————————————————————————————————————————

type OSStatus = i32;
type AudioObjectId = u32;

const AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectId = 1;
/// FourCC `'dOut'`（大写 O）：默认输出设备。
const AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE: u32 = 0x644F_7574;
/// FourCC `'glob'`：全局作用域。
const AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = 0x676C_6F62;
const AUDIO_OBJECT_PROPERTY_ELEMENT_MASTER: u32 = 0;
/// FourCC `'sfmt'`：设备流格式。
const AUDIO_DEVICE_PROPERTY_STREAM_FORMAT: u32 = 0x7366_6D74;
/// FourCC `'outp'`：输出作用域。
const AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT: u32 = 0x6F75_7470;

#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[repr(C)]
#[derive(Default)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(
        object_id: AudioObjectId,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const c_void,
        data_size: *mut u32,
        data: *mut c_void,
    ) -> OSStatus;
}

fn default_output_format() -> AppResult<OutputFormat> {
    let device_id = default_output_device()?;
    let address = AudioObjectPropertyAddress {
        selector: AUDIO_DEVICE_PROPERTY_STREAM_FORMAT,
        scope: AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
        element: AUDIO_OBJECT_PROPERTY_ELEMENT_MASTER,
    };
    let mut format = AudioStreamBasicDescription::default();
    let mut size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
    // SAFETY：format/size 都是本次调用自造的合法缓冲。
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            &mut format as *mut _ as *mut c_void,
        )
    };
    if status != 0 || format.channels_per_frame == 0 || format.sample_rate <= 0.0 {
        return Err(AppError::Media(format!(
            "读默认输出设备格式失败（OSStatus {status}，{}ch {}Hz）",
            format.channels_per_frame, format.sample_rate
        )));
    }
    Ok(OutputFormat {
        sample_rate: format.sample_rate.round() as u32,
        channels: format.channels_per_frame as u16,
    })
}

fn default_output_device() -> AppResult<AudioObjectId> {
    let address = AudioObjectPropertyAddress {
        selector: AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE,
        scope: AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
        element: AUDIO_OBJECT_PROPERTY_ELEMENT_MASTER,
    };
    let mut device_id: AudioObjectId = 0;
    let mut size = std::mem::size_of::<AudioObjectId>() as u32;
    // SAFETY：device_id/size 都是本次调用自造的合法缓冲。
    let status = unsafe {
        AudioObjectGetPropertyData(
            AUDIO_OBJECT_SYSTEM_OBJECT,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            &mut device_id as *mut _ as *mut c_void,
        )
    };
    if status != 0 || device_id == 0 {
        return Err(AppError::Media(format!(
            "没有默认音频输出设备（OSStatus {status}）"
        )));
    }
    Ok(device_id)
}

// ————————————————————————————————————————————————————————————
// AudioQueue
// ————————————————————————————————————————————————————————————

type AudioQueueRef = *mut c_void;
type AudioQueueBufferRef = *mut AudioQueueBuffer;

/// `AudioQueueBuffer`（只需要前三个字段；后续字段由系统维护）。
#[repr(C)]
struct AudioQueueBuffer {
    audio_data_bytes_capacity: u32,
    audio_data: *mut c_void,
    audio_data_byte_size: u32,
    /// 系统私有字段，**布局必须保留**（AudioQueueBuffer 是可变长结构）。
    user_data: *mut c_void,
    packet_description_capacity: u32,
    packet_descriptions: *mut c_void,
    packet_description_count: u32,
}

/// `AudioQueueOutputCallback`。
type AudioQueueOutputCallback =
    unsafe extern "C" fn(user_data: *mut c_void, queue: AudioQueueRef, buffer: AudioQueueBufferRef);

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioQueueNewOutput(
        format: *const AudioStreamBasicDescription,
        callback: AudioQueueOutputCallback,
        user_data: *mut c_void,
        callback_run_loop: *mut c_void,
        callback_run_loop_mode: *mut c_void,
        flags: u32,
        queue_out: *mut AudioQueueRef,
    ) -> OSStatus;
    fn AudioQueueAllocateBuffer(
        queue: AudioQueueRef,
        buffer_byte_size: u32,
        buffer_out: *mut AudioQueueBufferRef,
    ) -> OSStatus;
    fn AudioQueueEnqueueBuffer(
        queue: AudioQueueRef,
        buffer: AudioQueueBufferRef,
        num_packet_descriptions: u32,
        packet_descs: *const c_void,
    ) -> OSStatus;
    fn AudioQueueStart(queue: AudioQueueRef, start_time: *const c_void) -> OSStatus;
    fn AudioQueuePause(queue: AudioQueueRef) -> OSStatus;
    fn AudioQueueStop(queue: AudioQueueRef, immediate: u8) -> OSStatus;
    fn AudioQueueDispose(queue: AudioQueueRef, immediate: u8) -> OSStatus;
}

/// 线性 PCM FourCC `'lpcm'`。
const FORMAT_LINEAR_PCM: u32 = 0x6C70_636D;
/// float + packed。
const FORMAT_FLAG_FLOAT_PACKED: u32 = 0x1 | 0x8;

/// 每个缓冲的时长（秒）。3 个缓冲轮转：一个在播、一个在队列、一个在填。
/// 20ms 让端到端延迟保持在几十毫秒量级（直播/短剧的常规取值）。
const BUFFER_SECS: f64 = 0.02;
/// 缓冲个数（AudioQueue 的经典三缓冲：填 → 入队 → 播放循环）。
const BUFFER_COUNT: usize = 3;

fn os(status: OSStatus, what: &str) -> AppResult<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(AppError::Media(format!(
            "AudioToolbox {what} 失败（OSStatus {status}）"
        )))
    }
}

/// 渲染回调：`(缓冲样本位, 设备引线帧数)`。提为命名类型（clippy
/// type_complexity）。
type RenderCallback = Box<dyn FnMut(&mut [f32], u32) + Send>;

/// 回调闭包与引线参数的载体（`user_data` 指向它）。
struct QueueState {
    render: RenderCallback,
    /// 一个缓冲的帧数（设备引线口径）。
    lead_frames: u32,
}

unsafe extern "C" fn queue_callback(
    user_data: *mut c_void,
    queue: AudioQueueRef,
    buffer: AudioQueueBufferRef,
) {
    unsafe {
        if user_data.is_null() || buffer.is_null() {
            return;
        }
        let state = &mut *(user_data as *mut QueueState);
        let ptr = (*buffer).audio_data as *mut f32;
        let capacity = (*buffer).audio_data_bytes_capacity as usize;
        if ptr.is_null() || capacity == 0 {
            return;
        }
        let samples = capacity / std::mem::size_of::<f32>();
        let data = std::slice::from_raw_parts_mut(ptr, samples);
        (state.render)(data, state.lead_frames);
        (*buffer).audio_data_byte_size = capacity as u32;
        // 填完立刻重新入队：AudioQueue 靠「回调 → 填充 → 入队」维持播放。
        let _ = AudioQueueEnqueueBuffer(queue, buffer, 0, std::ptr::null());
    }
}

/// AudioQueue 输出流（output-only）。
pub struct AudioQueueStream {
    queue: AudioQueueRef,
    state: *mut QueueState,
    playing: AtomicBool,
}

// SAFETY：AudioQueueRef 是不透明句柄，AudioQueueStart/Pause/Stop 明确可从
// 任意线程调用；回调闭包要求 Send。跨执行器搬运的约束由本声明给出。
unsafe impl Send for AudioQueueStream {}

impl AudioOutputStream for AudioQueueStream {
    fn set_playing(&self, playing: bool) -> AppResult<()> {
        if self.playing.swap(playing, Ordering::Relaxed) == playing {
            return Ok(());
        }
        let status = unsafe {
            if playing {
                AudioQueueStart(self.queue, std::ptr::null())
            } else {
                AudioQueuePause(self.queue)
            }
        };
        os(status, if playing { "启动输出" } else { "暂停输出" })
    }
}

impl Drop for AudioQueueStream {
    fn drop(&mut self) {
        unsafe {
            // 先停（immediate）再 dispose：dispose 会等回调线程收尾，顺序反了
            // 可能死锁。state 必须在队列销毁**之后**释放——回调还在用它。
            AudioQueueStop(self.queue, 1);
            AudioQueueDispose(self.queue, 1);
            if !self.state.is_null() {
                // SAFETY：指针由 open() 里的 Box::into_raw 给出，只在此释放一次。
                drop(Box::from_raw(self.state));
                self.state = std::ptr::null_mut();
            }
        }
    }
}

/// macOS 的输出宿主（门面按目标 re-export）。
pub struct MacAudioOutput;

impl AudioOutputHost for MacAudioOutput {
    type Stream = AudioQueueStream;

    fn default_output_format() -> AppResult<OutputFormat> {
        default_output_format()
    }

    fn open<F>(format: OutputFormat, render: F) -> AppResult<Self::Stream>
    where
        F: FnMut(&mut [f32], u32) + Send + 'static,
    {
        if format.channels == 0 || format.sample_rate == 0 {
            return Err(AppError::Media("输出格式非法（0 声道或 0 采样率）".into()));
        }
        let channels = u32::from(format.channels);
        let asbd = AudioStreamBasicDescription {
            sample_rate: f64::from(format.sample_rate),
            format_id: FORMAT_LINEAR_PCM,
            format_flags: FORMAT_FLAG_FLOAT_PACKED,
            bytes_per_packet: 4 * channels,
            frames_per_packet: 1,
            bytes_per_frame: 4 * channels,
            channels_per_frame: channels,
            bits_per_channel: 32,
            reserved: 0,
        };
        let lead_frames = (f64::from(format.sample_rate) * BUFFER_SECS).round() as u32;
        let state = Box::into_raw(Box::new(QueueState {
            render: Box::new(render),
            lead_frames,
        }));

        let mut queue: AudioQueueRef = std::ptr::null_mut();
        let status = unsafe {
            AudioQueueNewOutput(
                &asbd,
                queue_callback,
                state as *mut c_void,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                &mut queue,
            )
        };
        if let Err(e) = os(status, "建输出队列") {
            unsafe { drop(Box::from_raw(state)) };
            return Err(e);
        }

        let byte_size =
            (f64::from(format.sample_rate) * BUFFER_SECS).round() as u32 * 4 * channels;
        for index in 0..BUFFER_COUNT {
            let mut buffer: AudioQueueBufferRef = std::ptr::null_mut();
            let status =
                unsafe { AudioQueueAllocateBuffer(queue, byte_size, &mut buffer) };
            if let Err(e) = os(status, "分配输出缓冲") {
                unsafe {
                    AudioQueueDispose(queue, 1);
                    drop(Box::from_raw(state));
                }
                return Err(e);
            }
            // 先垫满静音再入队：设备启动瞬间队列里有货，不会有第一拍的爆音。
            unsafe {
                let ptr = (*buffer).audio_data as *mut u8;
                std::ptr::write_bytes(ptr, 0, byte_size as usize);
                (*buffer).audio_data_byte_size = byte_size;
                let _ = AudioQueueEnqueueBuffer(queue, buffer, 0, std::ptr::null());
            }
            log::debug!("[Platform/audio] 输出缓冲 {index}/{BUFFER_COUNT} 已入队");
        }

        log::info!(
            "[Platform/audio] 输出队列已建（{}ch {}Hz，AudioQueue，{}×{}ms）",
            format.channels,
            format.sample_rate,
            BUFFER_COUNT,
            (BUFFER_SECS * 1000.0) as u32
        );
        Ok(AudioQueueStream {
            queue,
            state,
            // 未启动：与 AudioDevice 的起播闸配合，首帧出画后才真正 start。
            playing: AtomicBool::new(false),
        })
    }
}
