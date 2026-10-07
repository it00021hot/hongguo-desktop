//! 流式解密的黄金等价测试。
//!
//! 唯一可信的正确性标准：**惰性渲染出的每个字节都与
//! [`super::decrypt_buffer::decrypt_mp4_buffer`] 的整文件解密一致**——
//! 两条路径产出不同，说明流式路径在某个区间算错了偏移或 keystream，
//! 而那种错法在真机上表现为「播得动前几秒、一 seek 就花屏」。
//!
//! fixture 覆盖三类边角：音视频样本交错、**跨轨样本重叠**（音频样本压在
//! 视频样本上，双轨都解——XOR 组合律的用武之地）、非 16 字节对齐的
//! 样本长度（37/129 这类奇数，检验区间解密的首块对齐）。

use super::streaming::{locate_moov, SparseBuffer, StreamingPlan};
use crate::domain::crypto::cenc;
use crate::domain::mp4::decrypt_buffer::decrypt_mp4_buffer;
use crate::domain::mp4::r#box::build_box;

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

fn u32b(v: u32) -> Vec<u8> {
    v.to_be_bytes().to_vec()
}

/// 一条加密轨：样本内容 + 逐样本 IV。
struct EncTrack {
    video: bool,
    /// 样本明文（也用于算大小）
    samples: Vec<Vec<u8>>,
    ivs: Vec<[u8; 8]>,
    /// 样本在 mdat 载荷里的相对起点（由排布函数回填）
    rel_offsets: Vec<u64>,
}

/// 造一个 stsd 加密入口（encv/enca + sinf(frma)）。
fn enc_entry(video: bool) -> Vec<u8> {
    let kind: &[u8; 4] = if video { b"encv" } else { b"enca" };
    let orig: &[u8; 4] = if video { b"hvc1" } else { b"mp4a" };
    let frma = build_box(b"frma", orig);
    let schm = build_box(b"schm", &cat(&[&[0u8; 4], b"cenc", &[0u8; 4]]));
    let schi = build_box(b"schi", &build_box(b"tenc", &[0u8; 4]));
    let sinf = build_box(b"sinf", &cat(&[&frma, &schm, &schi]));
    #[allow(non_snake_case)]
    let hvcC = build_box(b"hvcC", &[0u8; 8]);

    let mut entry = Vec::new();
    entry.extend_from_slice(&[0u8; 4]); // 大小占位
    entry.extend_from_slice(kind);
    entry.extend_from_slice(&[0u8; 6]); // 保留
    entry.extend_from_slice(&[0u8; 2]); // data_ref
    if video {
        entry.extend_from_slice(&hvcC);
    }
    entry.extend_from_slice(&sinf);
    let total = entry.len() as u32;
    entry[..4].copy_from_slice(&total.to_be_bytes());
    entry
}

/// 造一条 trak（含 senc 与单 chunk 样本表）。`payload_base` 是 mdat 载荷的
/// 绝对起点，用于把相对偏移换算成 stco 里的绝对偏移。
fn build_trak(t: &EncTrack, payload_base: u64) -> Vec<u8> {
    let entry = enc_entry(t.video);
    let stsd = build_box(b"stsd", &cat(&[&[0u8; 4], &u32b(1), &entry]));

    let mut stsz_payload = cat(&[&[0u8; 4], &u32b(0), &u32b(t.samples.len() as u32)]);
    for s in &t.samples {
        stsz_payload.extend_from_slice(&u32b(s.len() as u32));
    }
    let stsz = build_box(b"stsz", &stsz_payload);

    // 单 chunk 装下全部样本：stsc = (first=1, per=样本数)
    let stsc = build_box(
        b"stsc",
        &cat(&[
            &[0u8; 4],
            &u32b(1),
            &u32b(1),
            &u32b(t.samples.len() as u32),
            &u32b(1),
        ]),
    );
    let stco = build_box(
        b"stco",
        &cat(&[
            &[0u8; 4],
            &u32b(1),
            &u32b((payload_base + t.rel_offsets[0]) as u32),
        ]),
    );

    let mut senc_payload = u32b(t.ivs.len() as u32);
    for iv in &t.ivs {
        senc_payload.extend_from_slice(iv);
    }
    let senc = build_box(b"senc", &senc_payload);

    let stbl = build_box(b"stbl", &cat(&[&stsd, &stsz, &stsc, &stco, &senc]));
    let vmhd = build_box(b"vmhd", &[0u8; 12]);
    let smhd = build_box(b"smhd", &[0u8; 4]);
    let dref = build_box(
        b"dref",
        &cat(&[&[0u8; 4], &u32b(1), &u32b(12), b"url ", &[0u8; 4]]),
    );
    let dinf = build_box(b"dinf", &dref);
    let media_header = if t.video { &vmhd } else { &smhd };
    let minf = build_box(b"minf", &cat(&[media_header, &dinf, &stbl]));
    let handler: &[u8; 4] = if t.video { b"vide" } else { b"soun" };
    let hdlr = build_box(b"hdlr", &cat(&[&[0u8; 8], handler, &[0u8; 12]]));
    let mdhd = build_box(b"mdhd", &[0u8; 24]);
    let mdia = build_box(b"mdia", &cat(&[&mdhd, &hdlr, &minf]));
    let tkhd = build_box(b"tkhd", &cat(&[&[0u8; 12], &u32b(1), &[0u8; 40]]));
    build_box(b"trak", &cat(&[&tkhd, &mdia]))
}

/// 排布样本并回填相对偏移。视频三段顺序排；音频两段故意压在视频段上
/// （跨轨重叠），重叠字节要被两条轨的 keystream 各异或一次。
fn layout_tracks() -> (EncTrack, EncTrack) {
    let mut video = EncTrack {
        video: true,
        samples: vec![
            (0..37u32).map(|i| (i % 251 + 1) as u8).collect(),
            (0..64u32).map(|i| (i * 7 % 253 + 2) as u8).collect(),
            (0..129u32).map(|i| (i * 13 % 241 + 3) as u8).collect(),
        ],
        ivs: vec![[0x11; 8], [0x22; 8], [0x33; 8]],
        rel_offsets: vec![],
    };
    video.rel_offsets = vec![0, 37, 101];

    let mut audio = EncTrack {
        video: false,
        samples: vec![
            (0..24u32).map(|i| (i * 11 % 239 + 5) as u8).collect(),
            (0..48u32).map(|i| (i * 17 % 227 + 6) as u8).collect(),
        ],
        ivs: vec![[0xAA; 8], [0xBB; 8]],
        rel_offsets: vec![],
    };
    // 与视频第 2、3 段重叠
    audio.rel_offsets = vec![29, 111];

    (video, audio)
}

/// 组装一份完整 fixture：明文文件、密文文件、密钥。
/// `moov_at_tail` 切换两种布局（平台流是尾部；头部布局检验平移映射）。
pub(crate) fn assemble(moov_at_tail: bool) -> (Vec<u8>, Vec<u8>, [u8; 16]) {
    let key = [0x5Fu8; 16];
    let (video, audio) = layout_tracks();

    let ftyp = build_box(b"ftyp", b"isom\x00\x00\x02\x00isomiso2");
    let trak_v = build_trak(&video, 0); // payload_base 稍后重算
    let trak_a = build_trak(&audio, 0);
    let moov_len = 8 + trak_v.len() + trak_a.len();

    // mdat 载荷长度 = 最大样本末端
    let end_of_samples = |t: &EncTrack| {
        t.rel_offsets
            .iter()
            .zip(&t.samples)
            .map(|(o, s)| o + s.len() as u64)
            .max()
            .unwrap()
    };
    let payload_len = end_of_samples(&video).max(end_of_samples(&audio));

    let mdat_payload_start = if moov_at_tail {
        ftyp.len() as u64 + 8
    } else {
        ftyp.len() as u64 + moov_len as u64 + 8
    };

    // 用正确的绝对基址重建 trak（stco 要绝对偏移）
    let trak_v = build_trak(&video, mdat_payload_start);
    let trak_a = build_trak(&audio, mdat_payload_start);
    let moov = build_box(b"moov", &cat(&[&trak_v, &trak_a]));
    assert_eq!(moov.len(), moov_len, "两次构建的 moov 应等长");

    // 明文 mdat：非样本字节填 0x5A，样本按偏移放入
    let mut payload = vec![0x5Au8; payload_len as usize];
    for t in [&video, &audio] {
        for (o, s) in t.rel_offsets.iter().zip(&t.samples) {
            payload[*o as usize..(*o as usize + s.len())].copy_from_slice(s);
        }
    }
    let mdat = build_box(b"mdat", &payload);

    let mut plain = ftyp.clone();
    if moov_at_tail {
        plain.extend_from_slice(&mdat);
        plain.extend_from_slice(&moov);
    } else {
        plain.extend_from_slice(&moov);
        plain.extend_from_slice(&mdat);
    }

    // 密文 = 明文经各轨样本按序异或（与 decrypt_mp4_buffer 的循环同构）
    let mut cipher = plain.clone();
    for t in [&video, &audio] {
        for (i, (o, s)) in t.rel_offsets.iter().zip(&t.samples).enumerate() {
            let abs = (mdat_payload_start + o) as usize;
            let mut d = cenc::new_decryptor(&key, &t.ivs[i]);
            cenc::decrypt_sample(&mut d, &mut cipher[abs..abs + s.len()]);
        }
    }

    (plain, cipher, key)
}

/// 从密文里截出 moov 区域（模拟尾部预取 / 头部缓冲）。
fn moov_region_of(cipher: &[u8]) -> (Vec<u8>, u64) {
    let (abs, size) = locate_moov(cipher, 0, cipher.len() as u64).expect("fixture 里应能定位 moov");
    (cipher[abs as usize..(abs + size) as usize].to_vec(), abs)
}

/// 全量惰性渲染，拼出整份明文。
fn render_all(plan: &StreamingPlan, sparse: &SparseBuffer) -> Vec<u8> {
    let mut out = vec![0u8; plan.plain_len() as usize];
    plan.render(0, plan.plain_len(), sparse, &mut out);
    out
}

#[test]
fn lazy_render_matches_whole_file_decrypt_tail_moov() {
    // 黄金等价：尾部 moov（平台真实布局）
    let (_plain, cipher, key) = assemble(true);
    let reference = decrypt_mp4_buffer(&cipher, &key).expect("整文件解密应成功");

    let (region, moov_abs) = moov_region_of(&cipher);
    let plan =
        StreamingPlan::build(&region, moov_abs, cipher.len() as u64, &key).expect("计划应构建成功");
    let sparse = SparseBuffer::new(cipher.len() as u64);
    sparse.write(0, &cipher);

    let out = render_all(&plan, &sparse);
    assert_eq!(out, reference, "惰性渲染必须与整文件解密逐字节一致");
    // 不与 plain 直接比前缀：fixture 的音频样本故意压在视频样本上，
    // reference（整文件解密）对重叠字节就是双重异或的——那正是要锁住的行为
}

#[test]
fn lazy_render_matches_whole_file_decrypt_head_moov() {
    // 头部 moov：moov 变短后 mdat 前移，检验平移映射
    let (plain, cipher, key) = assemble(false);
    let reference = decrypt_mp4_buffer(&cipher, &key).expect("整文件解密应成功");

    let (region, moov_abs) = moov_region_of(&cipher);
    let plan =
        StreamingPlan::build(&region, moov_abs, cipher.len() as u64, &key).expect("计划应构建成功");
    let sparse = SparseBuffer::new(cipher.len() as u64);
    sparse.write(0, &cipher);

    let out = render_all(&plan, &sparse);
    assert_eq!(out, reference, "头部 moov 布局也要逐字节一致");
    let _ = plain;
}

#[test]
fn windowed_render_matches_whole_file_decrypt() {
    // 按 7 字节一窗渲染（比 16 字节块还小，专门打首块对齐），
    // 拼起来必须仍与整文件解密一致——协议层就是按任意 Range 取的
    let (_plain, cipher, key) = assemble(true);
    let reference = decrypt_mp4_buffer(&cipher, &key).unwrap();

    let (region, moov_abs) = moov_region_of(&cipher);
    let plan = StreamingPlan::build(&region, moov_abs, cipher.len() as u64, &key).unwrap();
    let sparse = SparseBuffer::new(cipher.len() as u64);
    sparse.write(0, &cipher);

    let mut out = Vec::with_capacity(plan.plain_len() as usize);
    let mut pos = 0u64;
    while pos < plan.plain_len() {
        let end = (pos + 7).min(plan.plain_len());
        let mut chunk = vec![0u8; (end - pos) as usize];
        plan.render(pos, end, &sparse, &mut chunk);
        out.extend_from_slice(&chunk);
        pos = end;
    }
    assert_eq!(out, reference, "任意窗口渲染拼接后必须与整文件解密一致");
}

#[test]
fn random_sized_ranges_also_match() {
    // 混合窗口大小（1/3/16/64/999 字节）逐段渲染，覆盖各种对齐情形
    let (_plain, cipher, key) = assemble(true);
    let reference = decrypt_mp4_buffer(&cipher, &key).unwrap();

    let (region, moov_abs) = moov_region_of(&cipher);
    let plan = StreamingPlan::build(&region, moov_abs, cipher.len() as u64, &key).unwrap();
    let sparse = SparseBuffer::new(cipher.len() as u64);
    sparse.write(0, &cipher);

    let sizes = [1u64, 3, 16, 64, 999];
    let mut out = Vec::with_capacity(plan.plain_len() as usize);
    let mut pos = 0u64;
    let mut i = 0usize;
    while pos < plan.plain_len() {
        let end = (pos + sizes[i % sizes.len()]).min(plan.plain_len());
        let mut chunk = vec![0u8; (end - pos) as usize];
        plan.render(pos, end, &sparse, &mut chunk);
        out.extend_from_slice(&chunk);
        pos = end;
        i += 1;
    }
    assert_eq!(out, reference);
}

#[test]
fn cipher_ranges_needed_split_around_moov() {
    // 一个横跨 moov 的明文区间：需要的密文应分成前后两段，中段（moov）不需要
    let (_plain, cipher, key) = assemble(false); // 头部 moov，才有「后段」
    let (region, moov_abs) = moov_region_of(&cipher);
    let plan = StreamingPlan::build(&region, moov_abs, cipher.len() as u64, &key).unwrap();

    let old_len = region.len() as i64;
    // i64：head 布局下 plain_len < cipher_len，u64 直接减会下溢
    let new_len = plan.plain_len() as i64 - cipher.len() as i64 + old_len;
    let moov_plain_end = moov_abs as i64 + new_len;
    let start = moov_abs.saturating_sub(4);
    let end = (moov_plain_end as u64 + 8).min(plan.plain_len());
    let ranges = plan.cipher_ranges_needed(start, end);

    assert_eq!(ranges.len(), 2, "moov 前后各一段: {ranges:?}");
    assert!(ranges[0].1 <= moov_abs, "第一段不得伸进旧 moov: {ranges:?}");
    // 尾段映射回密文时应从旧 moov 之后开始
    assert!(ranges[1].0 >= moov_abs + old_len as u64, "{ranges:?}");
}

#[test]
fn plain_len_shrinks_with_the_rebuilt_moov() {
    // 重建的 moov 摘掉了 sinf/senc 等保护 box，明文应比密文短，
    // 差值正是新旧 moov 的长度差
    let (_plain, cipher, key) = assemble(true);
    let (region, moov_abs) = moov_region_of(&cipher);
    let plan = StreamingPlan::build(&region, moov_abs, cipher.len() as u64, &key).unwrap();

    let old_len = region.len() as u64;
    let new_len = plan.plain_len() - moov_abs; // 尾部 moov：明文在 moov 前与密文等长
    assert!(new_len < old_len, "重建应摘掉保护 box 变短");
    assert_eq!(
        cipher.len() as u64 - plan.plain_len(),
        old_len - new_len,
        "总长差应等于新旧 moov 的长度差"
    );
}
