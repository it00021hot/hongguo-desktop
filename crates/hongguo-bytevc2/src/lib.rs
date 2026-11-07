//! ByteVC2（MP4 fourcc `bvc2` / `video_meta.codec_type=bytevc2`）识别与比特流转换。
//!
//! 对照：
//! - librempeg `ff_bvc2_decoder`（与 VVC 同核，`cbs_bvc2_split_fragment` 固定 4 字节长度前缀）
//! - 官方 `libttmplayer` 的 `convertByteVC2Extradata`（配置记录 → Annex-B 参数集）
//!
//! 本 crate **不解帧**——解码交给带 `bvc2` 的 ffmpeg（librempeg）；这里只回答「是不是
//! bytevc2」以及把官方/容器形态的参数集、样本转成解码器吃的 Annex-B。

mod codec;
mod extradata;
mod sample;

pub use codec::{CONFIG_BOX, FOURCC, WIRE_NAME, is_bytevc2, is_bytevc2_fourcc};
pub use extradata::{annexb_from_extradata, length_size_from_extradata};
pub use sample::sample_to_annexb;
