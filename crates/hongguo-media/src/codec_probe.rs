//! 快速合并前的编码一致性探测：[`crate::remux::concat_copy`] 会**重写索引**（合并后的样本表覆盖全集、时长累加到全集末尾），
//! 但样本描述、编解码参数这些 box 是从第 1 集原样搬过去的——产物能不能播，取决于各集除「样本表」之外的部分是否一致。

use std::path::{Path, PathBuf};

use crate::demux::demux_file;

/// 某一条轨道的编码指纹。全部字段相同才算同一条编码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackSignature {
    pub is_video: bool,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub channels: u16,
    pub sample_rate: u32,
}

/// 一次一致性检查的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consistency {
    /// 所有集是否完全一致
    pub consistent: bool,
    /// 第一个与第 1 集不一致的集号（`consistent` 为 true 时是 None）
    pub mismatch_episode: Option<u32>,
}

/// 检查一批分集能否直接字节拼接，以第 1 集为基准；解析失败（文件损坏、不是 MP4）算不一致并指向那一集——「读不出来」本身就是不能拼的信号。
/// 不返回 `Result`：它只回答「能不能拼」，解析失败落在哪个集上已经编码进 `mismatch_episode`。
pub fn check(inputs: &[(u32, PathBuf)]) -> Consistency {
    // 少于 2 集无从比较
    if inputs.len() < 2 {
        return Consistency {
            consistent: true,
            mismatch_episode: None,
        };
    }
    let (first_episode, first_path) = &inputs[0];
    let Some(base) = signature_of(first_path) else {
        return Consistency {
            consistent: false,
            mismatch_episode: Some(*first_episode),
        };
    };

    for (episode, path) in &inputs[1..] {
        if signature_of(path).as_ref() != Some(&base) {
            return Consistency {
                consistent: false,
                mismatch_episode: Some(*episode),
            };
        }
    }
    Consistency {
        consistent: true,
        mismatch_episode: None,
    }
}

/// 一个文件里全部轨道的指纹，按「视频在前、音频在后、同类按 track_id」归一化（不同集里 `trak` 排列顺序不同不算不一致）。
fn signature_of(path: &Path) -> Option<Vec<TrackSignature>> {
    let mut tracks: Vec<(u32, TrackSignature)> = demux_file(path)
        .ok()?
        .tracks
        .into_iter()
        .map(|t| {
            let info = t.info;
            (
                info.track_id,
                TrackSignature {
                    is_video: info.is_video,
                    codec: info.codec,
                    width: info.width,
                    height: info.height,
                    channels: info.channels,
                    sample_rate: info.sample_rate,
                },
            )
        })
        .collect();
    tracks.sort_by_key(|(id, sig)| (!sig.is_video, *id));
    Some(tracks.into_iter().map(|(_, sig)| sig).collect())
}
