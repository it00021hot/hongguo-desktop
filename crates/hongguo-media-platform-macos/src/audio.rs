//! 平台 AAC 解码：AudioToolbox AudioConverter 主路 + symphonia 软解兜底。
//!
//! 主路走系统解码器——与 bilibili/抖音/ExoPlayer 的选择一致：一个解码器
//! 覆盖 AAC-LC 与 HE-AAC（SBR/PS）。`AudioConverter` + `DecompressionMagicCookie`，
//! mpv 的 coreaudio AAC 解码同款。**用法规矩**（三条缺一即 'fmt?'/'!dat'，
//! 全部经本机 C 探针与 Apple 自家 afconvert 产物对拍实锤）：
//!
//! 1. format ID 是 `'aac '`（0x6161_6320）。曾误写成 0x6163_6320（'acc '），
//!    一字之差 New 直接 'fmt?'——主路在任何 Mac 上都建不起来、全体静默回落
//!    软解，还被误判成「系统移除了 AAC 解码组件」（组件其实在，见 Info.plist）。
//! 2. magic cookie 是 **ESDS 描述符块**（[`synth_esds_cookie`]），不是裸 ASC；
//! 3. 输入声道按 ASC 的 channelConfiguration，且**单声道核必须显式给 Mono
//!    输入布局**（`kAudioConverterInputChannelLayout`），否则 cookie 一律 '!dat'；
//!    输出声道跟容器声明走（转换器上混，含 HE-AAC v2 的 PS 立体声还原）。
//!
//! 兜底走 symphonia 软解（ASC 经 `CodecParameters::extra_data` 注入，MP4 裸
//! 样本直喂），只解 AAC-LC，供主路真不可用时兜住。
//!
//! 输出 = 交错 f32，声道/采样率按 mp4 声明——调用方（时钟、设备流）只认
//! 声明格式，无需感知 profile 与后端。

mod ffi;

use std::os::raw::c_void;

use hongguo_common::{AppError, AppResult};

/// 一包 AAC 解出的最大帧数（LC/HE 均为 1024）。
const FRAMES_PER_PACKET: usize = 1024;

/// 从 ASC（AudioSpecificConfig）解析 channelConfiguration（bit 10–13）。
/// 返回原始配置值：1 = 单声道核，2 = 立体声，0 = 未指明；更高值/转义罕见。
///
/// 现行流的容器声明与 ASC 不一致很常见（mp4 标 2ch、ASC 是单声道核——
/// HE-AAC v2 的 PS 更是「单声道核 + 参数化还原立体声」的固定形态）。解码器
/// 的**输入**声道必须跟 ASC 走（magic cookie 校验会比对，不符报 '!dat'），
/// **输出**声道跟容器走（AudioConverter 自动上/下混，下游管线口径不变）。
fn asc_channel_config(asc: &[u8]) -> Option<u32> {
    let b0 = *asc.first()? as u32;
    let b1 = *asc.get(1)? as u32;
    let v = (b0 << 8) | b1;
    Some((v >> 3) & 0xF)
}

/// 解码器**输入**声道数：ASC 指明了就按 ASC（1/2），没指明回落容器声明。
fn decoder_input_channels(asc: &[u8], declared: u16) -> u16 {
    match asc_channel_config(asc) {
        Some(1) => 1,
        Some(2) => 2,
        _ => declared,
    }
}

/// 合成 AudioConverter 认的 magic cookie：**ESDS 描述符块**（ES_Descriptor ⊃
/// DecoderConfigDescriptor ⊃ DecoderSpecificInfo(=ASC) + SLConfigDescriptor），
/// 与 mp4 esds atom 体/`kAudioFilePropertyMagicCookieData` 给出的形态一致。
/// 裸 ASC 会被拒（'!dat'）——本机 C 探针对拍实锤（Apple 自家 afconvert 写出
/// 的 m4a 里 cookie 就是这个形态，39 字节）。
fn synth_esds_cookie(asc: &[u8]) -> Vec<u8> {
    let l = asc.len();
    let mut out = Vec::with_capacity(32 + l);
    // ES_Descriptor：ES_ID(2)+flags(1) + DCD(5+18+L) + SLConfig(6)
    out.extend_from_slice(&[0x03, 0x80, 0x80, 0x80, (32 + l) as u8]);
    out.extend_from_slice(&[0x00, 0x00, 0x00]);
    // DecoderConfigDescriptor：OTI 0x40（MPEG-4 Audio）+ streamType audio(0x14)
    // + bufferSizeDB(3) + max/avgBitrate(4+4，全零）+ DSI + SL
    out.extend_from_slice(&[0x04, 0x80, 0x80, 0x80, (18 + l) as u8]);
    out.extend_from_slice(&[0x40, 0x14]);
    out.extend_from_slice(&[0x00; 11]);
    // DecoderSpecificInfo = ASC 本体
    out.extend_from_slice(&[0x05, 0x80, 0x80, 0x80, l as u8]);
    out.extend_from_slice(asc);
    // SLConfigDescriptor
    out.extend_from_slice(&[0x06, 0x80, 0x80, 0x80, 0x01, 0x02]);
    out
}

/// OSStatus 转可读错误：FourCC 可打印就按字符打（'fmt?'/'dmgc' 这类）。
fn status_msg(status: ffi::OSStatus) -> String {
    let u = status as u32;
    let bytes = u.to_be_bytes();
    if bytes.iter().all(u8::is_ascii) {
        format!("0x{u:08X}('{}')", String::from_utf8_lossy(&bytes))
    } else {
        format!("0x{u:08X}")
    }
}

/// 输入回调的取数状态。回调发生在 `AudioConverterFillComplexBuffer` 内部、
/// 与 `decode` 同一线程串行调用，无并发；用裸结构体 + 指针传递即可。
struct PendingPacket {
    data: Vec<u8>,
    taken: bool,
    channels: u32,
    desc: ffi::AudioStreamPacketDescription,
}

/// 输入数据回调：每次调用交出至多一包；包交完后回 0 包表示输入用尽。
unsafe extern "C" fn input_proc(
    _converter: ffi::AudioConverterRef,
    io_packets: *mut u32,
    io_data: *mut ffi::AudioBufferList,
    out_desc: *mut *mut ffi::AudioStreamPacketDescription,
    user: *mut c_void,
) -> ffi::OSStatus {
    unsafe {
        let pending = &mut *(user as *mut PendingPacket);
        if pending.taken || pending.data.is_empty() {
            *io_packets = 0;
            return ffi::noErr;
        }
        pending.taken = true;
        *io_packets = 1;
        // 交错给自家缓冲（头文件明示 ioData 为空时回调自备缓冲）
        let list = &mut *io_data;
        list.mNumberBuffers = 1;
        list.mBuffers[0].mNumberChannels = pending.channels;
        list.mBuffers[0].mDataByteSize = pending.data.len() as u32;
        list.mBuffers[0].mData = pending.data.as_mut_ptr() as *mut c_void;
        if !out_desc.is_null() {
            pending.desc = ffi::AudioStreamPacketDescription {
                mStartOffset: 0,
                mVariableFramesInPacket: 0,
                mDataByteCount: pending.data.len() as u32,
            };
            *out_desc = &mut pending.desc;
        }
        ffi::noErr
    }
}

/// AudioToolbox 后端（主路）。
struct AudioToolboxDecoder {
    converter: ffi::AudioConverterRef,
    /// 解码器输入声道（ASC 口径，供输入回调标注包声道数）。
    input_channels: u16,
}

impl AudioToolboxDecoder {
    fn open(asc: &[u8], channels: u16, sample_rate: u32) -> AppResult<Self> {
        // 解码器输入声道按 ASC（cookie 校验比对它），输出按容器声明——
        // 两侧不一致由 AudioConverter 的混音段消化，见 asc_channel_config
        let input_channels = decoder_input_channels(asc, channels);
        let src = ffi::AudioStreamBasicDescription {
            mSampleRate: f64::from(sample_rate),
            mFormatID: ffi::kAudioFormatMPEG4AAC,
            mFormatFlags: 0,
            mBytesPerPacket: 0,
            mFramesPerPacket: FRAMES_PER_PACKET as u32,
            mBytesPerFrame: 0,
            mChannelsPerFrame: u32::from(input_channels),
            mBitsPerChannel: 0,
            mReserved: 0,
        };
        // 输出：交错 f32（Packed + Float，NonInterleaved 位不设）；声道按容器
        // 声明——单声道核的流由转换器上混，下游管线只认声明格式
        let bytes_per_frame = 4 * u32::from(channels);
        let dst = ffi::AudioStreamBasicDescription {
            mSampleRate: f64::from(sample_rate),
            mFormatID: ffi::kAudioFormatLinearPCM,
            mFormatFlags: ffi::kAudioFormatFlagIsFloat | ffi::kAudioFormatFlagIsPacked,
            mBytesPerPacket: bytes_per_frame,
            mFramesPerPacket: 1,
            mBytesPerFrame: bytes_per_frame,
            mChannelsPerFrame: u32::from(channels),
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        let mut converter = std::ptr::null_mut();
        let st = unsafe { ffi::AudioConverterNew(&src, &dst, &mut converter) };
        if st != ffi::noErr {
            return Err(AppError::Media(format!(
                "AudioConverterNew 失败: {}（入参 ch={input_channels}→{channels} sr={sample_rate} asc={}B 首字节 {:02x?}）",
                status_msg(st),
                asc.len(),
                asc.iter().take(6).collect::<Vec<_>>()
            )));
        }
        // 单声道核的流必须显式给输入声道布局：没有布局时解码器按默认假设
        // 校验，单声道 cookie 一律被拒（'!dat'）——C 探针对拍实锤
        if input_channels == 1 {
            let layout = ffi::AudioChannelLayout {
                mChannelLayoutTag: ffi::kAudioChannelLayoutTag_Mono,
                mChannelBitmap: 0,
                mNumberChannelDescriptions: 0,
            };
            let st = unsafe {
                ffi::AudioConverterSetProperty(
                    converter,
                    ffi::kAudioConverterInputChannelLayout,
                    std::mem::size_of::<ffi::AudioChannelLayout>() as u32,
                    &layout as *const ffi::AudioChannelLayout as *const c_void,
                )
            };
            if st != ffi::noErr {
                unsafe { ffi::AudioConverterDispose(converter) };
                return Err(AppError::Media(format!(
                    "设置输入声道布局失败: {}",
                    status_msg(st)
                )));
            }
        }
        // magic cookie 用 ESDS 描述符块（裸 ASC 被拒，见 synth_esds_cookie）
        let cookie = synth_esds_cookie(asc);
        let st = unsafe {
            ffi::AudioConverterSetProperty(
                converter,
                ffi::kAudioConverterDecompressionMagicCookie,
                cookie.len() as u32,
                cookie.as_ptr() as *const c_void,
            )
        };
        if st != ffi::noErr {
            unsafe { ffi::AudioConverterDispose(converter) };
            return Err(AppError::Media(format!(
                "设置 AAC magic cookie 失败: {}",
                status_msg(st)
            )));
        }
        Ok(Self {
            converter,
            input_channels,
        })
    }

    fn decode(&mut self, packet: &[u8], channels: usize) -> AppResult<Vec<f32>> {
        if packet.is_empty() {
            return Ok(Vec::new());
        }
        let mut pending = PendingPacket {
            data: packet.to_vec(),
            taken: false,
            channels: u32::from(self.input_channels),
            desc: ffi::AudioStreamPacketDescription {
                mStartOffset: 0,
                mVariableFramesInPacket: 0,
                mDataByteCount: 0,
            },
        };
        // 双包容量：本包 1024 帧 + 前包残余一次带出
        let mut out = vec![0f32; FRAMES_PER_PACKET * 2 * channels];
        let mut filled = 0usize;
        loop {
            let free = out.len() - filled;
            let free_frames = free / channels;
            // 每次只请求 1 包的量：请求更多时转换器解完本包还会再拉一包凑数，
            // 而输入回调在包耗尽时回「0 包 + noErr」会被它当成**输入流结束**
            //（EOS），之后所有 Fill 永远产出 0——只解出首包静音的元凶
            let mut io_packets = free_frames.min(FRAMES_PER_PACKET) as u32; // PCM 输出 1 帧 = 1 包
            let data_ptr = unsafe { out.as_mut_ptr().add(filled) };
            let mut list = ffi::AudioBufferList {
                mNumberBuffers: 1,
                mBuffers: [ffi::AudioBuffer {
                    mNumberChannels: channels as u32,
                    mDataByteSize: (free * 4) as u32,
                    mData: data_ptr as *mut c_void,
                }],
            };
            let st = unsafe {
                ffi::AudioConverterFillComplexBuffer(
                    self.converter,
                    input_proc,
                    &mut pending as *mut PendingPacket as *mut c_void,
                    &mut io_packets,
                    &mut list,
                    std::ptr::null_mut(),
                )
            };
            if st != ffi::noErr {
                return Err(AppError::Media(format!("AAC 解包失败: {}", status_msg(st))));
            }
            let produced_frames = io_packets as usize;
            filled += produced_frames * channels;
            // 无进展（既没拿包也没产出）或包已交出：等下一包再继续
            if produced_frames == 0 || pending.taken {
                break;
            }
        }
        out.truncate(filled);
        Ok(out)
    }
}

impl Drop for AudioToolboxDecoder {
    fn drop(&mut self) {
        unsafe { ffi::AudioConverterDispose(self.converter) };
    }
}

// ---------------------------------------------------------------- symphonia 软解兜底

/// symphonia 软解后端（兜底）：ASC 经 `extra_data` 注入，MP4 裸样本直喂。
/// 只支持 AAC-LC（symphonia 的能力边界，见模块注释）。
struct SymphoniaDecoder {
    decoder: Box<dyn symphonia_core::codecs::Decoder>,
    track_id: u32,
    pts: u64,
    /// 每包帧数（从 ASC 解出；LC 恒 1024）。
    frames_per_packet: u64,
    sample_buf: Option<symphonia_core::audio::SampleBuffer<f32>>,
}

impl SymphoniaDecoder {
    fn open(asc: &[u8], channels: u16, sample_rate: u32) -> AppResult<Self> {
        use symphonia_core::audio::Channels;
        use symphonia_core::codecs::{CODEC_TYPE_AAC, CodecParameters, CodecRegistry};

        // ASC 里的声道数 → symphonia 声道位标志（现行流恒单/双声道）。
        let channels_flag = match channels {
            1 => Channels::FRONT_LEFT,
            2 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
            _ => {
                return Err(AppError::Media(format!(
                    "symphonia 兜底不支持 {channels} 声道"
                )));
            }
        };
        let asc_owned: Box<[u8]> = asc.into();
        let mut params = CodecParameters::new();
        params
            .for_codec(CODEC_TYPE_AAC)
            .with_sample_rate(sample_rate)
            .with_channels(channels_flag)
            .with_extra_data(asc_owned);

        let mut registry = CodecRegistry::new();
        registry.register_all::<symphonia_codec_aac::AacDecoder>();
        let decoder = registry
            .make(&params, &symphonia_core::codecs::DecoderOptions::default())
            .map_err(|e| AppError::Media(format!("symphonia AAC 解码器建立失败: {e}")))?;

        Ok(Self {
            decoder,
            track_id: 1,
            pts: 0,
            frames_per_packet: 1024,
            sample_buf: None,
        })
    }

    fn decode(&mut self, packet: &[u8]) -> AppResult<Vec<f32>> {
        use symphonia_core::audio::SampleBuffer;
        use symphonia_core::formats::Packet;

        let pkt = Packet::new_from_slice(self.track_id, self.pts, self.frames_per_packet, packet);
        self.pts += self.frames_per_packet;
        let buf_ref = self
            .decoder
            .decode(&pkt)
            .map_err(|e| AppError::Media(format!("symphonia AAC 解码失败: {e}")))?;
        // symphonia 产出 planar/原生格式，统一转交错 f32
        let spec = *buf_ref.spec();
        let cap = buf_ref.capacity() as u64;
        let sample_buf = self
            .sample_buf
            .get_or_insert_with(|| SampleBuffer::<f32>::new(cap, spec));
        sample_buf.copy_interleaved_ref(buf_ref);
        Ok(sample_buf.samples().to_vec())
    }

    fn reset(&mut self) {
        // symphonia 无 in-place reset：解码器带状态重建（seek 场景本来就该丢）
        self.decoder.reset();
    }
}

// ---------------------------------------------------------------- 双后端门面

/// AAC 逐包解码器。一次 `decode` 喂一包（MP4 样本原样字节），返回本次产出
/// 的交错 f32（内部流水线未满时可返回空；残余帧由后续包带出）。
pub struct AacDecoder {
    backend: AacBackend,
    channels: usize,
    sample_rate: u32,
}

enum AacBackend {
    AudioToolbox(AudioToolboxDecoder),
    Symphonia(SymphoniaDecoder),
}

// AudioConverter 对象可跨线程搬移（Apple 文档：非重入但线程间传递安全），
// symphonia 解码器为纯 Rust 数据。使用纪律（始终单任务串行调用）由调用方保证。
unsafe impl Send for AacDecoder {}

impl AacDecoder {
    /// 用 mp4 声明的声道/采样率与 esds 里的 AudioSpecificConfig 建解码器。
    /// 输出格式 = 交错 f32 × 声明声道 × 声明采样率。
    ///
    /// 主路 AudioToolbox（HE-AAC 也覆盖）；系统解码器建不起来时回退
    /// symphonia 软解（仅 AAC-LC）。
    pub fn open(asc: &[u8], channels: u16, sample_rate: u32) -> AppResult<Self> {
        if asc.is_empty() || channels == 0 || sample_rate == 0 {
            return Err(AppError::Media(format!(
                "AAC 参数不全（asc={}B ch={channels} sr={sample_rate}）",
                asc.len()
            )));
        }
        match AudioToolboxDecoder::open(asc, channels, sample_rate) {
            Ok(at) => Ok(Self {
                backend: AacBackend::AudioToolbox(at),
                channels: usize::from(channels),
                sample_rate,
            }),
            Err(at_err) => match SymphoniaDecoder::open(asc, channels, sample_rate) {
                Ok(sp) => {
                    log::warn!(
                        "[Platform/audio] AudioToolbox AAC 不可用（{at_err}），回退 symphonia 软解（仅 LC）"
                    );
                    Ok(Self {
                        backend: AacBackend::Symphonia(sp),
                        channels: usize::from(channels),
                        sample_rate,
                    })
                }
                Err(sp_err) => Err(AppError::Media(format!(
                    "AAC 解码器两个后端都失败：主路 {at_err}；软解 {sp_err}"
                ))),
            },
        }
    }

    /// 解一包。包解不出来返回 `Err`（调用方决定补静音等策略），正常返回
    /// 本包带出的样本（可为空——解码器流水线未满）。
    pub fn decode(&mut self, packet: &[u8]) -> AppResult<Vec<f32>> {
        match &mut self.backend {
            AacBackend::AudioToolbox(at) => at.decode(packet, self.channels),
            AacBackend::Symphonia(sp) => sp.decode(packet),
        }
    }

    /// 源流不连续（seek 跳点）时冲内部状态。
    pub fn reset(&mut self) {
        match &mut self.backend {
            AacBackend::AudioToolbox(at) => {
                let st = unsafe { ffi::AudioConverterReset(at.converter) };
                if st != ffi::noErr {
                    log::warn!("[Platform/audio] AudioConverterReset: {}", status_msg(st));
                }
            }
            AacBackend::Symphonia(sp) => sp.reset(),
        }
    }

    /// 声明采样率（诊断日志用）。
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

impl hongguo_media_platform_api::AudioDecoder for AacDecoder {
    fn decode(&mut self, packet: &[u8]) -> AppResult<Vec<f32>> {
        AacDecoder::decode(self, packet)
    }

    fn reset(&mut self) {
        AacDecoder::reset(self);
    }
}

// ————————————————————————————————————————————————————————————
// 默认输出设备的混音格式：见 `audio_output::MacAudioOutput::default_output_format`
// （那是 AudioToolbox 输出单元的同一套能力，进 `audio_output` 模块后本文件
// 只管 AAC 解码；两端查询走的是同一段 CoreAudio 属性读法）。
// ————————————————————————————————————————————————————————————

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归：主路必须能解真实流的两种 profile（ASC 取自运行日志）。
    /// 历史事故：format ID 常量拼错（'acc ' ≠ 'aac '）时这里两条都回
    /// 'fmt?'。LC 兜底还有 symphonia；HE-AAC v2 只有主路能解——任何一条
    /// 退回都是用户可感知的回归（无声/起播失败）。
    #[test]
    fn audio_toolbox_opens_lc_and_he_aac() {
        // AAC-LC（AOT 2，44.1kHz 立体声）
        AudioToolboxDecoder::open(&[0x12, 0x10], 2, 44_100)
            .expect("AudioToolbox 主路应能解 AAC-LC 立体声");
        // AAC-LC 单声道核（现行流的真实形态：容器标 2ch、ASC 是 config=1）
        AudioToolboxDecoder::open(&[0x12, 0x08], 2, 44_100)
            .expect("AudioToolbox 主路应能解 AAC-LC 单声道核");
        // HE-AAC v2（AOT 29，SBR+PS，symphonia 兜底解不了、只有主路能解）
        AudioToolboxDecoder::open(&[0xeb, 0x8a, 0x08, 0x00], 2, 44_100)
            .expect("AudioToolbox 主路应能解 HE-AAC v2");
    }

    /// 解析 ADTS 流：返回 (ASC, 包列表)。头 7 字节（无 CRC）：
    /// profile(2)+采样率序号(4)+私有(1)+声道高位(1) | 帧长分散在 3–5 字节。
    fn adts_packets(data: &[u8]) -> (Vec<u8>, Vec<&[u8]>) {
        let mut i = 0;
        let mut packets = Vec::new();
        let (mut profile, mut sfidx, mut config) = (0u32, 0u32, 0u32);
        while i + 7 <= data.len() {
            assert_eq!(data[i], 0xFF, "ADTS 同步字丢失 @ {i}");
            assert_eq!(data[i + 1] & 0xF0, 0xF0);
            profile = u32::from((data[i + 2] >> 6) & 0x3);
            sfidx = u32::from((data[i + 2] >> 2) & 0xF);
            config = (u32::from(data[i + 2] & 0x1) << 2) | u32::from((data[i + 3] >> 6) & 0x3);
            let length = ((u32::from(data[i + 3]) & 0x3) << 11)
                | (u32::from(data[i + 4]) << 3)
                | (u32::from(data[i + 5]) >> 5);
            let end = i + length as usize;
            assert!(end <= data.len(), "ADTS 帧长越界 @ {i}");
            packets.push(&data[i..end]);
            i = end;
        }
        // profile 位 = AOT−1；ASC = AOT(5) | 采样率序号(4) | 声道配置(4) | 补零(3)
        let asc_v = ((profile + 1) << 11) | (sfidx << 7) | (config << 3);
        (vec![(asc_v >> 8) as u8, (asc_v & 0xFF) as u8], packets)
    }

    /// 断言一段 PCM 有真实能量（正弦 fixture 的 RMS 应远大于静音）。
    fn assert_audible(samples: &[f32], label: &str) {
        let energy: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        let rms = (energy / samples.len().max(1) as f64).sqrt();
        assert!(
            rms > 0.01,
            "{label}: PCM 近乎静音（rms={rms}）——解码路径坏了"
        );
    }

    /// 端到端：fixture（afconvert 真实编码的正弦音）逐包喂进主路解码器，
    /// 全流必须产出足量、有能量的 PCM（首包允许是流水线预充静音）。open
    /// 通过但 cookie/布局/fill 任一环节坏掉都会在这里暴露——「建得起解
    /// 不出」的主路等于没有主路。
    #[test]
    fn audio_toolbox_decodes_real_packets() {
        let (asc, packets) =
            adts_packets(include_bytes!("../tests/fixtures/aac-lc-44100-mono.adts"));
        assert_eq!(asc, vec![0x12, 0x08], "fixture 参数变了要同步测试");
        let mut dec = AacDecoder::open(&asc, 1, 44_100).expect("open 单声道");
        let mut pcm: Vec<f32> = Vec::new();
        for pkt in packets.iter() {
            pcm.extend(dec.decode(&pkt[7..]).expect("包解码不应失败"));
        }
        assert!(
            pcm.len() >= 44_100 * 3 / 10,
            "单声道产出过少：{} 样本",
            pcm.len()
        );
        assert_audible(&pcm, "单声道全流");

        let (asc, packets) =
            adts_packets(include_bytes!("../tests/fixtures/aac-lc-44100-stereo.adts"));
        assert_eq!(asc, vec![0x12, 0x10]);
        let mut dec = AacDecoder::open(&asc, 2, 44_100).expect("open 立体声");
        let mut pcm: Vec<f32> = Vec::new();
        for pkt in packets.iter() {
            pcm.extend(dec.decode(&pkt[7..]).expect("包解码不应失败"));
        }
        assert!(
            pcm.len() >= 44_100 * 2 * 3 / 10,
            "立体声产出过少：{} 样本",
            pcm.len()
        );
        assert_audible(&pcm, "立体声全流");
    }
}
