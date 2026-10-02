//! 落盘解密。
//!
//! 做法与现版一致：**整文件读入 → 原地覆盖视频轨样本的密文 → 整体写出**。
//! 采样数据解开后由 [`super::deprotect`] 摘掉加密标记（`encv`→`hvc1`、
//! 移除 `sinf`/`tenc`），否则产物仍被播放器当成加密流。
//!
//! ⚠️ 不要改成「重排文件 + 重建 moov」那套看似省内存的写法：
//!    那样必须同步改写 `stco` / `stsz`，漏一处 `stco` 就会让播放器
//!    读到错位数据——而且**不报错**。整文件进内存换来的是「布局不变」，
//!    音轨、`stco`、采样表全都是现成的。单集短剧 10~100 MB，这个代价可以接受。

use std::path::Path;

use crate::error::{AppError, AppResult};

/// 解密一个 MP4 文件。
///
/// # 参数
/// - `src`：加密源文件（通常是 `.enc.tmp`）
/// - `dst`：解密后的成品
/// - `key`：AES-128 密钥
///
/// 返回成品字节数。
pub fn decrypt_mp4_file(src: &Path, dst: &Path, key: &[u8; 16]) -> AppResult<u64> {
    let data = std::fs::read(src).map_err(|e| AppError::Io(format!("读取源文件失败: {e}")))?;
    let plain = super::decrypt_buffer::decrypt_mp4_buffer(&data, key)?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    std::fs::write(dst, &plain).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(plain.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::mp4::r#box::build_box;
    use crate::domain::mp4::sample_table::collect_tracks;

    /// 构造一个最小的可解析 MP4：`moov(trah(…stbl)) + mdat`。
    ///
    /// `hdlr` 的 handler type 必须是 `vide`，`collect_tracks` 才会认成视频轨。
    fn make_mp4(sample: &[u8]) -> Vec<u8> {
        // stsd: version+flags(4) | entry_count(4) | size(4) + format(4)
        let stsd = build_box(
            b"stsd",
            &[
                &[0u8; 4][..],
                &1u32.to_be_bytes()[..],
                &16u32.to_be_bytes()[..],
                b"hvc1",
            ]
            .concat(),
        );

        // stsz: version+flags(4) | sample_size(4) | sample_count(4) | sizes
        let stsz = build_box(
            b"stsz",
            &[
                &[0u8; 4][..],
                &(sample.len() as u32).to_be_bytes()[..],
                &1u32.to_be_bytes()[..],
                &(sample.len() as u32).to_be_bytes()[..],
            ]
            .concat(),
        );

        // stsc: first_chunk(1) | samples_per_chunk(1) | sample_description_index(1)
        let stsc = build_box(
            b"stsc",
            &[
                &[0u8; 4][..],
                &1u32.to_be_bytes()[..],
                &1u32.to_be_bytes()[..],
                &1u32.to_be_bytes()[..],
                &1u32.to_be_bytes()[..],
            ]
            .concat(),
        );

        // stco 偏移稍后回填
        let stco = build_box(
            b"stco",
            &[
                &[0u8; 4][..],
                &1u32.to_be_bytes()[..],
                &0u32.to_be_bytes()[..],
            ]
            .concat(),
        );

        // tkhd version 0: track_id 在载荷偏移 12
        let tkhd = build_box(
            b"tkhd",
            &[&[0u8; 12][..], &1u32.to_be_bytes()[..], &[0u8; 40][..]].concat(),
        );

        // hdlr: version+flags(4) | pre_defined(4) | handler_type(4)
        let hdlr = build_box(b"hdlr", &[&[0u8; 8][..], b"vide", &[0u8; 12][..]].concat());

        // stco 偏移字段在文件中的位置（**含前面的 ftyp**）：
        //   ftyp + moov头(8) + trak头(8) + tkhd
        //   + mdia头(8) + hdlr + minf头(8)
        //   + stbl头(8) + stsd + stsz + stsc
        //   + stco头(8) + version/flags(4) + count(4)
        let ftyp = build_box(b"ftyp", b"isom\x00\x00\x02\x00isomiso2");
        let stco_entry_at = ftyp.len()
            + 8 /* moov */
            + 8 /* trak */
            + tkhd.len()
            + 8 /* mdia */
            + hdlr.len()
            + 8 /* minf */
            + 8 /* stbl */
            + stsd.len()
            + stsz.len()
            + stsc.len()
            + 8 /* stco */
            + 4 /* version+flags */
            + 4 /* count */;

        let mut stbl = Vec::new();
        stbl.extend_from_slice(&stsd);
        stbl.extend_from_slice(&stsz);
        stbl.extend_from_slice(&stsc);
        stbl.extend_from_slice(&stco);
        let stbl = build_box(b"stbl", &stbl);

        let minf = build_box(b"minf", &stbl);
        let mdia = build_box(b"mdia", &[&hdlr[..], &minf[..]].concat());
        let trak = build_box(b"trak", &[&tkhd[..], &mdia[..]].concat());
        let moov = build_box(b"moov", &trak);
        let mdat = build_box(b"mdat", sample);

        let mut data = ftyp;
        let sample_at = (data.len() + moov.len() + 8) as u32; // ftyp + moov + mdat 头
        data.extend_from_slice(&moov);
        data.extend_from_slice(&mdat);
        data[stco_entry_at..stco_entry_at + 4].copy_from_slice(&sample_at.to_be_bytes());
        data
    }

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hg-dec-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sample_offsets_are_resolved_from_stsc_and_stco() {
        let mp4 = make_mp4(&[0xabu8; 32]);
        let tracks = collect_tracks(&mp4).expect("应能解析出轨道");
        let s = tracks[0].samples[0];
        assert!(s.0 > 0, "样本偏移应由 stsc+stco 推出，不能停在 0：{s:?}");
        assert_eq!(s.1, 32);
        // 偏移应指向 mdat 载荷（moov 之后）
        assert!(s.0 as usize + 32 <= mp4.len());
        assert_eq!(&mp4[s.0 as usize..s.0 as usize + 4], &[0xab; 4]);
    }

    #[test]
    fn decrypt_preserves_file_layout() {
        let dir = tmp_dir("layout");
        let mp4 = make_mp4(&[0xabu8; 32]);
        let src = dir.join("a.enc.tmp");
        let dst = dir.join("a.mp4");
        std::fs::write(&src, &mp4).unwrap();

        let n = decrypt_mp4_file(&src, &dst, &[0x42u8; 16]).expect("解密应成功");
        let out = std::fs::read(&dst).unwrap();

        // 关键性质：长度不变——moov / stco / 音轨都保持原样
        assert_eq!(n as usize, mp4.len());
        assert_eq!(out.len(), mp4.len());
        // 头部 box 结构必须完好
        assert_eq!(&out[..4], &mp4[..4], "ftyp 头不应被改动");
        assert!(collect_tracks(&out).is_ok(), "产物必须仍是可解析的 MP4");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_source_errors() {
        let r = decrypt_mp4_file(
            Path::new("/definitely/not/here.mp4"),
            Path::new("/tmp/out.mp4"),
            &[0u8; 16],
        );
        assert!(r.is_err());
    }

    #[test]
    fn garbage_input_reports_decode_error() {
        let dir = tmp_dir("bad");
        let src = dir.join("bad.enc.tmp");
        let dst = dir.join("bad.mp4");
        std::fs::write(&src, vec![0xffu8; 1000]).unwrap();

        let r = decrypt_mp4_file(&src, &dst, &[0u8; 16]);
        assert!(r.is_err());
        // 失败时不该留下一个半成品
        assert!(!dst.exists(), "解密失败不应留下产物");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
