//! 快速合并前的编码一致性探测。
//!
//! [`crate::media::remux::concat_copy`] 是整文件字节级顺序拼接，不重写索引：
//! 产出的文件能不能播，全看各集的容器结构与编码参数是否对得上。同一份文件
//! 在别处解析出的轨道信息必须一致，否则这道闸门给出的结论和实际产物对不上。

use std::path::{Path, PathBuf};

use crate::media::demux::demux_file;

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

/// 检查一批分集能否直接字节拼接。
///
/// 以第 1 集为基准。解析失败（文件损坏、不是 MP4）算不一致并指向那一集——
/// 「读不出来」本身就是不能拼的信号，放过去等于把损坏留给用户。
///
/// 不返回 `Result`：它只回答「能不能拼」，解析失败落在哪个集上已经编码进
/// `mismatch_episode`。
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

/// 一个文件里全部轨道的指纹，按「视频在前、音频在后、同类按 track_id」排好。
///
/// 轨道顺序要归一化：不同集里 `trak` 的排列顺序不同不算不一致。
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::mp4::fixtures::{mp4, TrackSpec};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hg-codec-probe-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 造一集并落盘，返回 `(集号, 路径)`。
    fn episode(dir: &Path, vid_index: u32, tracks: &[TrackSpec]) -> (u32, PathBuf) {
        let path = dir.join(format!("{vid_index}.mp4"));
        std::fs::write(&path, mp4(tracks)).unwrap();
        (vid_index, path)
    }

    #[test]
    fn identical_episodes_are_consistent() {
        let dir = temp_dir("same");
        let inputs = vec![
            episode(&dir, 1, &[TrackSpec::video(1, b"hvc1", 1920, 1080)]),
            episode(&dir, 2, &[TrackSpec::video(1, b"hvc1", 1920, 1080)]),
        ];

        assert_eq!(
            check(&inputs),
            Consistency {
                consistent: true,
                mismatch_episode: None
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_codec_different_resolution_is_a_mismatch() {
        // 同样 hvc1，1080p 与 720p 拼起来是坏文件：四字符码一样不够
        let dir = temp_dir("resolution");
        let inputs = vec![
            episode(&dir, 1, &[TrackSpec::video(1, b"hvc1", 1920, 1080)]),
            episode(&dir, 2, &[TrackSpec::video(1, b"hvc1", 1280, 720)]),
        ];

        assert_eq!(
            check(&inputs),
            Consistency {
                consistent: false,
                mismatch_episode: Some(2)
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn different_codec_is_a_mismatch() {
        let dir = temp_dir("codec");
        let inputs = vec![
            episode(&dir, 1, &[TrackSpec::video(1, b"hvc1", 1920, 1080)]),
            episode(&dir, 2, &[TrackSpec::video(1, b"avc1", 1920, 1080)]),
        ];

        assert_eq!(
            check(&inputs),
            Consistency {
                consistent: false,
                mismatch_episode: Some(2)
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_the_first_mismatch_not_the_last() {
        let dir = temp_dir("first");
        let inputs = vec![
            episode(&dir, 1, &[TrackSpec::video(1, b"hvc1", 1920, 1080)]),
            episode(&dir, 2, &[TrackSpec::video(1, b"hvc1", 1920, 1080)]),
            episode(&dir, 3, &[TrackSpec::video(1, b"avc1", 1920, 1080)]),
            episode(&dir, 4, &[TrackSpec::video(1, b"avc1", 640, 360)]),
        ];

        assert_eq!(
            check(&inputs).mismatch_episode,
            Some(3),
            "只报第一个不一致的集，后面的不必再看"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn audio_parameters_are_part_of_the_fingerprint() {
        let dir = temp_dir("audio");
        let inputs = vec![
            episode(
                &dir,
                1,
                &[
                    TrackSpec::video(1, b"hvc1", 1920, 1080),
                    TrackSpec::audio(2, b"mp4a", 2, 44100),
                ],
            ),
            episode(
                &dir,
                2,
                &[
                    TrackSpec::video(1, b"hvc1", 1920, 1080),
                    TrackSpec::audio(2, b"mp4a", 2, 48000),
                ],
            ),
        ];

        assert_eq!(check(&inputs).mismatch_episode, Some(2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn track_order_does_not_matter() {
        // 第 2 集把音轨排在视频轨前面：轨道顺序不同不该算不一致
        let dir = temp_dir("order");
        let inputs = vec![
            episode(
                &dir,
                1,
                &[
                    TrackSpec::video(1, b"hvc1", 1920, 1080),
                    TrackSpec::audio(2, b"mp4a", 2, 44100),
                ],
            ),
            episode(
                &dir,
                2,
                &[
                    TrackSpec::audio(2, b"mp4a", 2, 44100),
                    TrackSpec::video(1, b"hvc1", 1920, 1080),
                ],
            ),
        ];

        assert!(check(&inputs).consistent);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn single_episode_has_nothing_to_compare() {
        let dir = temp_dir("one");
        let inputs = vec![episode(
            &dir,
            1,
            &[TrackSpec::video(1, b"hvc1", 1920, 1080)],
        )];

        assert_eq!(
            check(&inputs),
            Consistency {
                consistent: true,
                mismatch_episode: None
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_input_is_consistent() {
        assert_eq!(
            check(&[]),
            Consistency {
                consistent: true,
                mismatch_episode: None
            }
        );
    }

    #[test]
    fn garbage_file_is_a_mismatch() {
        let dir = temp_dir("garbage");
        let good = episode(&dir, 1, &[TrackSpec::video(1, b"hvc1", 1920, 1080)]);
        let bad_path = dir.join("2.mp4");
        std::fs::write(&bad_path, [0xffu8; 512]).unwrap();
        let inputs = vec![good, (2, bad_path)];

        // 解析不出来不是「一致」：那正是最该拦下的一集
        assert_eq!(
            check(&inputs),
            Consistency {
                consistent: false,
                mismatch_episode: Some(2)
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unparsable_first_episode_is_its_own_mismatch() {
        let dir = temp_dir("bad-first");
        let bad_path = dir.join("1.mp4");
        std::fs::write(&bad_path, [0xffu8; 512]).unwrap();
        let inputs = vec![
            (1, bad_path),
            episode(&dir, 2, &[TrackSpec::video(1, b"hvc1", 1920, 1080)]),
        ];

        assert_eq!(check(&inputs).mismatch_episode, Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_track_count_is_a_mismatch() {
        // 一集有音轨、另一集没有：轨道集合不同，拼出来的索引也对不上
        let dir = temp_dir("track-count");
        let inputs = vec![
            episode(&dir, 1, &[TrackSpec::video(1, b"hvc1", 1920, 1080)]),
            episode(
                &dir,
                2,
                &[
                    TrackSpec::video(1, b"hvc1", 1920, 1080),
                    TrackSpec::audio(2, b"mp4a", 2, 44100),
                ],
            ),
        ];

        assert_eq!(check(&inputs).mismatch_episode, Some(2));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
