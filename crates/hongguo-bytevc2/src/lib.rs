//! ByteVC2（MP4 fourcc `bvc2` / `video_meta.codec_type=bytevc2`）识别、比特流转换与解帧。
//!
//! 对照（只作规格，运行时不调用）：
//! - librempeg `ff_bvc2_decoder`（与 VVC 同核，`cbs_bvc2_split_fragment` 固定 4 字节长度前缀）
//! - 官方 `libttmplayer` 的 `convertByteVC2Extradata`（配置记录 → Annex-B 参数集）
//!
//! 解帧在本 crate 内完成，不启动 ffmpeg / librempeg 进程，也不链接其库。
//! 默认解核为 Fraunhofer libvvdec（feature `vvdec`）；可用
//! `HONGGUO_BYTEVC2_DECODER=oxideav` 回退到 oxideav-h266（feature `oxideav`）。

mod codec;
mod container;
mod decode;
mod extradata;
mod sample;

pub use codec::{CONFIG_BOX, FOURCC, WIRE_NAME, is_bytevc2, is_bytevc2_fourcc};
pub use decode::{
    DecodedFrame, Plaintext, active_decoder_name, decode_plaintext_mp4, open_plaintext,
    presentation_tick,
};
pub use extradata::{annexb_from_extradata, length_size_from_extradata};
pub use sample::sample_to_annexb;
