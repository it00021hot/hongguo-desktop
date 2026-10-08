//! TTEncrypt V5 的魔改哈希与分组加密（Python 参考实现的机械翻译）。
//! 函数体保持与参考实现逐句对应（不优化不改写），因此允许机械
//! 翻译固有的风格告警。
#![allow(
    clippy::assign_op_pattern,
    clippy::same_item_push,
    clippy::range_plus_one,
    unused_assignments,
    unused_mut,
    clippy::needless_range_loop,
    non_snake_case,
    clippy::manual_repeat_n,
    clippy::uninlined_format_args,
    unused_parens,
    clippy::needless_borrows_for_generic_args,
    clippy::double_parens,
    dead_code
)]
//!
//! - TtHashCore::calculate：SHA-512 骨架的哈希变体（128 字节块、
//!   64 字节输出），K/IV 被平台魔改，用本文件的表；
//! - hex_cf8 / hex_0a2：自定义分组加密的轮密钥扩展与 CBC 形态加密
//!   （含 10 张非标准 S-box/T-table）。
//!
//! 函数体是 gen_tt_hash.py 逐句自动转换（不要手改，重生成即可），
//! 行为对拍由 calculate 向量测试 + Python golden 测试兜底。

/// 魔改 K 表（160 个 u32 = 80 个 u64）
const K_TABLE: [u32; 160] = [
    3609767458, 1116352408, 602891725, 1899447441, 3964484399, 3049323471, 2173295548, 3921009573,
    4081628472, 961987163, 3053834265, 1508970993, 2937671579, 2453635748, 3664609560, 2870763221,
    2734883394, 3624381080, 1164996542, 310598401, 1323610764, 607225278, 3590304994, 1426881987,
    4068182383, 1925078388, 991336113, 2162078206, 633803317, 2614888103, 3479774868, 3248222580,
    2666613458, 3835390401, 944711139, 4022224774, 2341262773, 264347078, 2007800933, 604807628,
    1495990901, 770255983, 1856431235, 1249150122, 3175218132, 1555081692, 2198950837, 1996064986,
    3999719339, 2554220882, 766784016, 2821834349, 2566594879, 2952996808, 3203337956, 3210313671,
    1034457026, 3336571891, 2466948901, 3584528711, 3758326383, 113926993, 168717936, 338241895,
    1188179964, 666307205, 1546045734, 773529912, 1522805485, 1294757372, 2643833823, 1396182291,
    2343527390, 1695183700, 1014477480, 1986661051, 1206759142, 2177026350, 344077627, 2456956037,
    1290863460, 2730485921, 3158454273, 2820302411, 3505952657, 3259730800, 106217008, 3345764771,
    3606008344, 3516065817, 1432725776, 3600352804, 1467031594, 4094571909, 851169720, 275423344,
    3100823752, 430227734, 1363258195, 506948616, 3750685593, 659060556, 3785050280, 883997877,
    3318307427, 958139571, 3812723403, 1322822218, 2003034995, 1537002063, 3602036899, 1747873779,
    1575990012, 1955562222, 1125592928, 2024104815, 2716904306, 2227730452, 442776044, 2361852424,
    593698344, 2428436474, 3733110249, 2756734187, 2999351573, 3204031479, 3815920427, 3329325298,
    3928383900, 3391569614, 566280711, 3515267271, 3454069534, 3940187606, 4000239992, 4118630271,
    1914138554, 116418474, 2731055270, 174292421, 3203993006, 289380356, 320620315, 460393269,
    587496836, 685471733, 1086792851, 852142971, 365543100, 1017036298, 2618297676, 1126000580,
    3409855158, 1288033470, 4234509866, 1501505948, 987167468, 1607167915, 1246189591, 1816402316,
];

/// 初始状态（16 个 u32 = 8 个 u64；hex_c52 输出时高低字交换）
const TT_IV: [u32; 16] = [
    4089235720, 1779033703, 2227873595, 3144134277, 4271175723, 1013904242, 1595750129, 2773480762,
    2917565137, 1359893119, 725511199, 2600822924, 4215389547, 528734635, 327033209, 1541459225,
];

/// TT-Encrypt V5 密钥派生盐（与版本一起固定）
pub const TT_ORD_LIST: [u8; 64] = [
    77, 212, 194, 230, 184, 49, 98, 9, 14, 82, 179, 199, 166, 115, 59, 164, 28, 178, 70, 43, 130,
    154, 181, 138, 25, 107, 57, 219, 87, 23, 117, 36, 244, 155, 175, 127, 8, 232, 214, 141, 38,
    167, 46, 55, 193, 169, 90, 47, 31, 5, 165, 24, 146, 174, 242, 148, 151, 50, 182, 42, 56, 170,
    221, 88,
];

/// 带 CF 状态的核心（模拟 ARM 进位寄存器）
pub struct TtHashCore {
    cf: u8,
    block_bit_pos: u64,
}

impl Default for TtHashCore {
    fn default() -> Self {
        Self::new()
    }
}

impl TtHashCore {
    pub fn new() -> Self {
        Self {
            cf: 0,
            block_bit_pos: 0,
        }
    }

    #[inline]
    fn chk(&self, x: u32) -> u32 {
        x
    }

    #[inline]
    fn lsrs(&mut self, x: u32, k: u32) -> u32 {
        self.cf = ((x >> (k - 1)) & 1) as u8;
        x >> k
    }

    #[inline]
    fn lsls(&mut self, x: u32, k: u32) -> u32 {
        self.cf = ((x >> (32 - k)) & 1) as u8;
        x << k
    }

    #[inline]
    fn adds(&mut self, a: u32, b: u32) -> u32 {
        let (v, carry) = a.overflowing_add(b);
        self.cf = carry as u8;
        v
    }

    #[inline]
    fn adc(&mut self, a: u32, b: u32) -> u32 {
        let (v, c1) = a.overflowing_add(b);
        let (v, c2) = v.overflowing_add(self.cf as u32);
        self.cf = (c1 || c2) as u8;
        v
    }

    #[inline]
    fn adcs(&mut self, a: u32, b: u32) -> u32 {
        let (mut v, c1) = a.overflowing_add(b);
        let c2;
        (v, c2) = v.overflowing_add(self.cf as u32);
        self.cf = (c1 || c2) as u8;
        v
    }

    #[inline]
    fn eors(&self, a: u32, b: u32) -> u32 {
        a ^ b
    }

    #[inline]
    fn ands(&self, a: u32, b: u32) -> u32 {
        a & b
    }

    #[inline]
    fn orrs(&self, a: u32, b: u32) -> u32 {
        a | b
    }

    #[inline]
    fn rrx(&mut self, x: u32) -> u32 {
        (x >> 1) | ((self.cf as u32) << 31)
    }

    /// 参考实现的 UBFX 原样翻译（位域提取语义与标准不同，照抄才能对拍）
    fn ubfx(&self, num: u32, lsb: u32, width: u32) -> u32 {
        let tmp = format!("{:032b}", num);
        let start = 32usize.saturating_sub(lsb as usize + width as usize);
        let width = width as usize;
        let slice = &tmp[start..(start + width).min(tmp.len())];
        u32::from_str_radix(slice, 2).unwrap_or(0)
    }

    #[inline]
    fn utfx(&self, num: u32) -> u32 {
        num & 0xff
    }

    #[inline]
    fn hex_list(content: &[u32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(content.len() * 4);
        for v in content {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out
    }

    #[inline]
    fn dump_list(&self, content: &[u8]) -> Vec<u32> {
        let mut out = Vec::with_capacity(content.len() / 4);
        for pair in content.as_chunks::<4>().0 {
            out.push(u32::from_be_bytes([pair[0], pair[1], pair[2], pair[3]]));
        }
        out
    }

    /// 魔改哈希主函数（参考实现 calculate 原样翻译）
    pub fn calculate(&mut self, content: &[u8]) -> Vec<u8> {
        // Python 里 hex_6A8 是 calculate 的局部变量（每次从 0 起算）
        self.block_bit_pos = 0;
        let length = content.len();
        let mut tmp = content.to_vec();
        let divisible = length % 0x80;
        let t = 0x80usize - divisible;

        if t > 0x11 {
            tmp.push(0x80);
            for _ in 0..(t - 0x11) {
                tmp.push(0);
            }
            for _ in 0..16 {
                tmp.push(0);
            }
        } else {
            tmp.push(128);
            for _ in 0..(128 - 16 + t + 1) {
                tmp.push(0);
            }
            for _ in 0..16 {
                tmp.push(0);
            }
        }

        let tmp_list_size = tmp.len();
        let blocks = tmp_list_size / 0x80;
        // 状态跨块链接（list_6B0 只初始化一次）
        let mut state = TT_IV;
        for i in 0..blocks {
            if (tmp_list_size / 128 - 1) == i {
                let ending = self.handle_ending(self.block_bit_pos, divisible as u64);
                for j in 0..8 {
                    let index = tmp_list_size - j - 1;
                    tmp[index] = ending[7 - j];
                }
            }
            let mut param_list = [0u32; 32];
            for j in 0..32 {
                let b0 = tmp[0x80 * i + 4 * j] as u32;
                let b1 = tmp[0x80 * i + 4 * j + 1] as u32;
                let b2 = tmp[0x80 * i + 4 * j + 2] as u32;
                let b3 = tmp[0x80 * i + 4 * j + 3] as u32;
                param_list[j] = (b0 << 24) | (b1 << 16) | (b2 << 8) | b3;
            }
            let sched = self.hex_27e(param_list.to_vec());
            state = self.hex_30a(state, &sched);
            self.block_bit_pos += 0x400;
        }
        self.hex_c52(&state)
    }

    /// 长度字段（handle_ending 原样）：num 已是比特数（每块 +0x400），
    /// 只把当前块的余数字节转比特后相加
    fn handle_ending(&mut self, num: u64, r0: u64) -> [u8; 8] {
        num.wrapping_add(r0 << 3).to_be_bytes()
    }

    /// 输出字节序（hex_C52）：高低字交换后大端展开
    fn hex_c52(&self, st: &[u32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(st.len() * 4);
        for i in 0..8 {
            out.extend_from_slice(&st[2 * i + 1].to_be_bytes());
            out.extend_from_slice(&st[2 * i].to_be_bytes());
        }
        out
    }

    /// 消息调度（hex_27E 原样翻译）
    fn hex_27e(&mut self, mut p: Vec<u32>) -> Vec<u32> {
        let mut lr: u32 = 0;
        let mut r0: u32 = 0;
        let mut r10: u32 = 0;
        let mut r3: u32 = 0;
        let mut r4: u32 = 0;
        let mut r5: u32 = 0;
        let mut r6: u32 = 0;
        let mut r8: u32 = 0;
        let mut r9: u32 = 0;
        let mut p = p;
        r6 = p[0];
        r8 = p[1];
        for i in 0..0x40 {
            r0 = p[2 * i + 0x1c];
            r5 = p[2 * i + 0x1d];
            r4 = self.lsrs(r0, 0x13);
            r3 = self.lsrs(r0, 0x1d);
            lr = r4 | self.chk(r5) << 13;
            r4 = self.lsls(r0, 3);
            r4 = r4 | self.chk(r5) >> 29;
            r3 = r3 | self.chk(r5) << 3;
            r4 = r4 ^ self.chk(r0) >> 6;
            lr = lr ^ r4;
            r4 = self.lsrs(r5, 6);
            r4 = r4 | self.chk(r0) << 26;
            r9 = r3 ^ r4;
            r4 = self.lsrs(r5, 0x13);
            r0 = r4 | self.chk(r0) << 13;
            r10 = p[2 * i + 0x12];
            r3 = p[2 * i + 0x13];
            r5 = p[2 * i + 0x2];
            r4 = p[2 * i + 0x3];
            r0 = r0 ^ r9;
            r3 = self.adds(r3, r8);
            r6 = self.adc(r6, r10);
            r8 = self.adds(r3, r0);
            lr = self.adc(lr, r6);
            r6 = self.lsrs(r4, 7);
            r3 = self.lsrs(r4, 8);
            r6 = r6 | self.chk(r5) << 25;
            r3 = r3 | self.chk(r5) << 24;
            r3 = (self.eors(r3, r6));
            r6 = self.lsrs(r5, 1);
            r0 = (self.rrx(r4));
            r0 = (self.eors(r0, r3));
            r3 = r6 | self.chk(r4) << 31;
            r6 = self.lsrs(r5, 8);
            r0 = (self.adds(r0, r8));
            r6 = r6 | self.chk(r4) << 24;
            r8 = r4;
            r6 = r6 ^ self.chk(r5) >> 7;
            r3 = r3 ^ r6;
            r6 = r5;
            r3 = self.adc(r3, lr);
            {
                p.push(r3);
                p.push(r0);
            }
        }
        p
    }

    /// 压缩函数（hex_30A 原样翻译）
    fn hex_30a(&mut self, p0: [u32; 16], sched: &[u32]) -> [u32; 16] {
        let mut lr: u32 = 0;
        let mut r0: u32 = 0;
        let mut r1: u32 = 0;
        let mut r10: u32 = 0;
        let mut r11: u32 = 0;
        let mut r12: u32 = 0;
        let mut r2: u32 = 0;
        let mut r3: u32 = 0;
        let mut r4: u32 = 0;
        let mut r5: u32 = 0;
        let mut r6: u32 = 0;
        let mut r8: u32 = 0;
        let mut r9: u32 = 0;
        let mut v_350: u32 = 0;
        let mut v_354: u32 = 0;
        let mut v_358: u32 = 0;
        let mut v_35C: u32 = 0;
        let mut v_360: u32 = 0;
        let mut v_364: u32 = 0;
        let mut v_36C: u32 = 0;
        let mut v_370: u32 = 0;
        let mut v_374: u32 = 0;
        let mut v_378: u32 = 0;
        let mut v_37C: u32 = 0;
        let mut v_380: u32 = 0;
        let mut v_384: u32 = 0;
        let mut v_388: u32 = 0;
        let mut v_38C: u32 = 0;
        let mut v_390: u32 = 0;
        let mut v_398: u32 = 0;
        let mut v_39C: u32 = 0;
        let mut v_3A0: u32 = 0;
        let mut v_3A4: u32 = 0;
        let mut v_3A8: u32 = 0;
        let mut v_3AC: u32 = 0;
        let mut state = p0;
        v_3A0 = state[7];
        v_3A4 = state[6];
        v_374 = state[5];
        v_378 = state[4];
        lr = state[0];
        r12 = state[1];
        v_39C = state[2];
        v_398 = state[3];
        v_3AC = state[11];
        v_3A8 = state[10];
        r9 = state[12];
        r10 = state[13];
        r5 = state[9];
        r8 = state[8];
        r4 = state[15];
        r6 = state[14];
        for index in 0..10 {
            v_384 = r5;
            r3 = K_TABLE[0x10 * index];
            r1 = K_TABLE[0x10 * index + 2];
            r2 = K_TABLE[0x10 * index + 1];
            r3 = self.adds(r3, r6);
            r6 = self.chk(r8) >> 14;
            v_390 = r1;
            r6 = r6 | self.chk(r5) << 18;
            r1 = K_TABLE[0x10 * index + 3];
            r0 = K_TABLE[0x10 * index + 4];
            v_36C = r0;
            r0 = self.adc(r2, r4);
            r2 = self.lsrs(r5, 0x12);
            r4 = self.lsrs(r5, 0xE);
            r2 = r2 | self.chk(r8) << 14;
            r4 = r4 | self.chk(r8) << 18;
            r2 = self.eors(r2, r4);
            r4 = self.lsls(r5, 0x17);
            r4 = r4 | self.chk(r8) >> 9;
            v_38C = r1;
            r2 = self.eors(r2, r4);
            r4 = self.chk(r8) >> 18;
            r4 = r4 | self.chk(r5) << 14;
            r6 = self.eors(r6, r4);
            r4 = self.lsrs(r5, 9);
            r4 = r4 | self.chk(r8) << 23;
            v_354 = r8;
            r6 = self.eors(r6, r4);
            r3 = self.adds(r3, r6);
            r0 = self.adcs(r0, r2);
            r2 = sched[0x10 * index + 1];
            r2 = self.adds(r2, r3);
            r3 = sched[0x10 * index + 3];
            r6 = sched[0x10 * index];
            v_358 = r10;
            r6 = self.adcs(r6, r0);
            r0 = v_3AC;
            v_360 = r3;
            r0 = r0 ^ r10;
            r3 = sched[0x10 * index + 2];
            r0 = self.ands(r0, r5);
            r1 = sched[0x10 * index + 5];
            r4 = r0 ^ r10;
            r0 = v_3A8;
            v_364 = r1;
            r0 = r0 ^ r9;
            r1 = v_374;
            r0 = r0 & r8;
            r8 = v_39C;
            r0 = r0 ^ r9;
            v_35C = r3;
            r10 = self.adds(r2, r0);
            r0 = v_398;
            r11 = self.adc(r6, r4);
            r3 = v_378;
            r2 = r0 | r12;
            r6 = r0 & r12;
            r2 = self.ands(r2, r1);
            r1 = r0;
            r2 = self.orrs(r2, r6);
            r6 = r8 | lr;
            r6 = self.ands(r6, r3);
            r3 = r8 & lr;
            r3 = self.orrs(r3, r6);
            r6 = self.chk(r12) << 30;
            r0 = self.chk(r12) >> 28;
            r6 = r6 | self.chk(lr) >> 2;
            r0 = r0 | self.chk(lr) << 4;
            r4 = self.chk(lr) >> 28;
            r0 = self.eors(r0, r6);
            r6 = self.chk(r12) << 25;
            r6 = r6 | self.chk(lr) >> 7;
            r4 = r4 | self.chk(r12) << 4;
            r0 = self.eors(r0, r6);
            r6 = self.chk(r12) >> 2;
            r6 = r6 | self.chk(lr) << 30;
            r3 = self.adds(r3, r10);
            r6 = r6 ^ r4;
            r4 = self.chk(r12) >> 7;
            r4 = r4 | self.chk(lr) << 25;
            r2 = self.adc(r2, r11);
            r6 = self.eors(r6, r4);
            v_37C = r12;
            r5 = self.adds(r3, r6);
            r6 = self.adc(r2, r0);
            r0 = r6 | r12;
            r2 = r6 & r12;
            r0 = self.ands(r0, r1);
            r3 = self.lsrs(r6, 0x1C);
            r0 = self.orrs(r0, r2);
            r2 = self.lsls(r6, 0x1E);
            r2 = r2 | self.chk(r5) >> 2;
            r3 = r3 | self.chk(r5) << 4;
            r2 = self.eors(r2, r3);
            r3 = self.lsls(r6, 0x19);
            r3 = r3 | self.chk(r5) >> 7;
            r4 = self.lsrs(r5, 0x1C);
            r3 = self.eors(r3, r2);
            r2 = self.lsrs(r6, 2);
            r2 = r2 | self.chk(r5) << 30;
            r4 = r4 | self.chk(r6) << 4;
            r2 = self.eors(r2, r4);
            r4 = self.lsrs(r6, 7);
            r4 = r4 | self.chk(r5) << 25;
            r12 = r6;
            r2 = self.eors(r2, r4);
            r4 = r5 | lr;
            r4 = r4 & r8;
            r6 = r5 & lr;
            r4 = self.orrs(r4, r6);
            v_388 = r5;
            r5 = self.adds(r2, r4);
            r0 = self.adcs(r0, r3);
            v_398 = r1;
            r4 = r9;
            v_350 = r0;
            r0 = v_3A4;
            r1 = v_3A0;
            v_380 = lr;
            lr = self.adds(r0, r10);
            r9 = self.adc(r1, r11);
            r0 = v_3AC;
            r6 = self.chk(lr) >> 14;
            r1 = v_384;
            r3 = self.chk(r9) >> 18;
            r2 = self.chk(r9) >> 14;
            r3 = r3 | self.chk(lr) << 14;
            r2 = r2 | self.chk(lr) << 18;
            r2 = self.eors(r2, r3);
            r3 = self.chk(r9) << 23;
            r3 = r3 | self.chk(lr) >> 9;
            r6 = r6 | self.chk(r9) << 18;
            r2 = self.eors(r2, r3);
            r3 = self.chk(lr) >> 18;
            r3 = r3 | self.chk(r9) << 14;
            v_39C = r8;
            r3 = self.eors(r3, r6);
            r6 = self.chk(r9) >> 9;
            r6 = r6 | self.chk(lr) << 23;
            r8 = v_354;
            r3 = self.eors(r3, r6);
            r6 = r0 ^ r1;
            r6 = r6 & r9;
            v_370 = r12;
            r6 = self.eors(r6, r0);
            r0 = v_3A8;
            r1 = r0 ^ r8;
            r1 = r1 & lr;
            r1 = self.eors(r1, r0);
            r0 = v_358;
            r1 = self.adds(r1, r4);
            r6 = self.adcs(r6, r0);
            r0 = v_390;
            r1 = self.adds(r1, r0);
            r0 = v_38C;
            r6 = self.adcs(r6, r0);
            r0 = v_360;
            r1 = self.adds(r1, r0);
            r0 = v_35C;
            r6 = self.adcs(r6, r0);
            r1 = self.adds(r1, r3);
            r3 = self.adc(r6, r2);
            r2 = v_350;
            r0 = self.adds(r5, r1);
            r5 = v_37C;
            r4 = self.adc(r2, r3);
            v_390 = r4;
            r2 = r4 | r12;
            r6 = r4 & r12;
            r2 = self.ands(r2, r5);
            r5 = self.lsrs(r4, 0x1C);
            r10 = r2 | r6;
            r2 = self.lsls(r4, 0x1E);
            r2 = r2 | self.chk(r0) >> 2;
            r5 = r5 | self.chk(r0) << 4;
            r2 = self.eors(r2, r5);
            r5 = self.lsls(r4, 0x19);
            r5 = r5 | self.chk(r0) >> 7;
            r6 = self.lsrs(r0, 0x1C);
            r12 = r2 ^ r5;
            r2 = self.lsrs(r4, 2);
            r2 = r2 | self.chk(r0) << 30;
            r6 = r6 | self.chk(r4) << 4;
            r2 = self.eors(r2, r6);
            r6 = self.lsrs(r4, 7);
            r4 = v_388;
            r6 = r6 | self.chk(r0) << 25;
            r5 = v_380;
            r2 = self.eors(r2, r6);
            r6 = r0 | r4;
            r4 = self.ands(r4, r0);
            r6 = self.ands(r6, r5);
            v_38C = r0;
            r4 = self.orrs(r4, r6);
            r6 = lr ^ r8;
            r0 = self.adds(r2, r4);
            v_3A4 = r0;
            r0 = self.adc(r12, r10);
            v_3A0 = r0;
            r0 = v_378;
            r10 = self.adds(r1, r0);
            r0 = v_374;
            r6 = r6 & r10;
            r1 = self.adc(r3, r0);
            r5 = self.chk(r10) >> 14;
            r0 = v_384;
            r6 = r6 ^ r8;
            r3 = self.lsrs(r1, 0x12);
            r4 = self.lsrs(r1, 0xE);
            r3 = r3 | self.chk(r10) << 14;
            r4 = r4 | self.chk(r10) << 18;
            r3 = self.eors(r3, r4);
            r4 = self.lsls(r1, 0x17);
            r4 = r4 | self.chk(r10) >> 9;
            r5 = r5 | self.chk(r1) << 18;
            r11 = r3 ^ r4;
            r3 = self.chk(r10) >> 18;
            r3 = r3 | self.chk(r1) << 14;
            v_378 = r1;
            r3 = self.eors(r3, r5);
            r5 = self.lsrs(r1, 9);
            r5 = r5 | self.chk(r10) << 23;
            r3 = self.eors(r3, r5);
            r5 = r9 ^ r0;
            r5 = self.ands(r5, r1);
            r1 = v_3A8;
            r5 = self.eors(r5, r0);
            r0 = v_36C;
            r4 = self.adds(r0, r1);
            r2 = K_TABLE[0x10 * index + 5];
            r0 = v_3AC;
            r2 = self.adcs(r2, r0);
            r0 = v_364;
            r4 = self.adds(r4, r0);
            r12 = sched[0x10 * index + 4];
            r0 = v_3A4;
            r2 = self.adc(r2, r12);
            r6 = self.adds(r6, r4);
            r2 = self.adcs(r2, r5);
            r3 = self.adds(r3, r6);
            r11 = self.adc(r11, r2);
            r1 = self.adds(r0, r3);
            r0 = v_3A0;
            r6 = v_390;
            r4 = self.chk(r1) >> 28;
            r0 = self.adc(r0, r11);
            r5 = v_370;
            r2 = r0 | r6;
            r6 = self.ands(r6, r0);
            r2 = self.ands(r2, r5);
            r5 = self.lsrs(r0, 0x1C);
            r12 = r2 | r6;
            r6 = self.lsls(r0, 0x1E);
            r6 = r6 | self.chk(r1) >> 2;
            r5 = r5 | self.chk(r1) << 4;
            r6 = self.eors(r6, r5);
            r5 = self.lsls(r0, 0x19);
            r5 = r5 | self.chk(r1) >> 7;
            r4 = r4 | self.chk(r0) << 4;
            r6 = self.eors(r6, r5);
            r5 = self.lsrs(r0, 2);
            r5 = r5 | self.chk(r1) << 30;
            v_3AC = r0;
            r5 = self.eors(r5, r4);
            r4 = self.lsrs(r0, 7);
            r0 = v_38C;
            r4 = r4 | self.chk(r1) << 25;
            r2 = v_388;
            r5 = self.eors(r5, r4);
            r4 = r1 | r0;
            v_3A8 = r1;
            r4 = self.ands(r4, r2);
            r2 = r1 & r0;
            r2 = self.orrs(r2, r4);
            r0 = self.adds(r5, r2);
            v_3A4 = r0;
            r0 = self.adc(r6, r12);
            v_3A0 = r0;
            r0 = v_39C;
            r2 = v_398;
            r0 = self.adds(r0, r3);
            v_39C = r0;
            r11 = self.adc(r11, r2);
            r4 = self.lsrs(r0, 0xE);
            r3 = self.chk(r11) >> 18;
            r6 = self.chk(r11) >> 14;
            r3 = r3 | self.chk(r0) << 14;
            r6 = r6 | self.chk(r0) << 18;
            r3 = self.eors(r3, r6);
            r6 = self.chk(r11) << 23;
            r6 = r6 | self.chk(r0) >> 9;
            r4 = r4 | self.chk(r11) << 18;
            r1 = self.eors(r3, r6);
            r6 = self.lsrs(r0, 0x12);
            r6 = r6 | self.chk(r11) << 14;
            r3 = r10 ^ lr;
            r6 = self.eors(r6, r4);
            r4 = self.chk(r11) >> 9;
            r3 = self.ands(r3, r0);
            r4 = r4 | self.chk(r0) << 23;
            r5 = r6 ^ r4;
            v_398 = r1;
            r3 = r3 ^ lr;
            r1 = v_378;
            r6 = K_TABLE[0x10 * index + 6];
            r12 = K_TABLE[0x10 * index + 7];
            r4 = r1 ^ r9;
            r0 = v_384;
            r6 = self.adds(r6, r8);
            r4 = r4 & r11;
            r12 = self.adc(r12, r0);
            r4 = r4 ^ r9;
            r8 = sched[0x10 * index + 7];
            r2 = sched[0x10 * index + 6];
            r6 = self.adds(r6, r8);
            r0 = v_398;
            r2 = self.adc(r2, r12);
            r3 = self.adds(r3, r6);
            r2 = self.adcs(r2, r4);
            r6 = self.adds(r3, r5);
            r12 = self.adc(r2, r0);
            r0 = v_3A4;
            r4 = v_390;
            r1 = self.adds(r0, r6);
            r0 = v_3A0;
            v_384 = r1;
            r5 = self.adc(r0, r12);
            r0 = v_3AC;
            r8 = self.chk(r1) >> 28;
            r2 = r5 | r0;
            r3 = r8 | self.chk(r5) << 4;
            r2 = self.ands(r2, r4);
            r4 = r5 & r0;
            r0 = r2 | r4;
            r4 = self.lsls(r5, 0x1E);
            r2 = self.lsrs(r5, 0x1C);
            r4 = r4 | self.chk(r1) >> 2;
            r2 = r2 | self.chk(r1) << 4;
            v_3A0 = r0;
            r2 = self.eors(r2, r4);
            r4 = self.lsls(r5, 0x19);
            r4 = r4 | self.chk(r1) >> 7;
            r0 = v_3A8;
            r2 = self.eors(r2, r4);
            r4 = self.lsrs(r5, 2);
            r4 = r4 | self.chk(r1) << 30;
            r8 = r5;
            r3 = self.eors(r3, r4);
            r4 = self.lsrs(r5, 7);
            r4 = r4 | self.chk(r1) << 25;
            r5 = v_38C;
            r3 = self.eors(r3, r4);
            r4 = r1 | r0;
            r4 = self.ands(r4, r5);
            r5 = r1 & r0;
            r4 = self.orrs(r4, r5);
            v_36C = r8;
            r0 = self.adds(r3, r4);
            v_3A4 = r0;
            r0 = v_3A0;
            r0 = self.adcs(r0, r2);
            v_3A0 = r0;
            r0 = v_380;
            r2 = v_37C;
            r0 = self.adds(r0, r6);
            r5 = self.adc(r12, r2);
            v_37C = r5;
            r4 = self.lsrs(r0, 0xE);
            v_380 = r0;
            r2 = self.lsrs(r5, 0x12);
            r3 = self.lsrs(r5, 0xE);
            r2 = r2 | self.chk(r0) << 14;
            r3 = r3 | self.chk(r0) << 18;
            r2 = self.eors(r2, r3);
            r3 = self.lsls(r5, 0x17);
            r3 = r3 | self.chk(r0) >> 9;
            r4 = r4 | self.chk(r5) << 18;
            r1 = r2 ^ r3;
            r3 = self.lsrs(r0, 0x12);
            r3 = r3 | self.chk(r5) << 14;
            v_398 = r1;
            r3 = self.eors(r3, r4);
            r4 = self.lsrs(r5, 9);
            r1 = v_378;
            r4 = r4 | self.chk(r0) << 23;
            r12 = r3 ^ r4;
            r3 = sched[0x10 * index + 9];
            r4 = r11 ^ r1;
            r4 = self.ands(r4, r5);
            r4 = self.eors(r4, r1);
            r1 = v_39C;
            r5 = r1 ^ r10;
            r5 = self.ands(r5, r0);
            r5 = r5 ^ r10;
            r2 = K_TABLE[0x10 * index + 8];
            r0 = self.adds(r2, lr);
            r2 = K_TABLE[0x10 * index + 9];
            r2 = self.adc(r2, r9);
            r0 = self.adds(r0, r3);
            r3 = sched[0x10 * index + 8];
            r2 = self.adcs(r2, r3);
            r0 = self.adds(r0, r5);
            r2 = self.adcs(r2, r4);
            r1 = self.adds(r0, r12);
            r0 = v_398;
            r3 = v_3AC;
            r4 = self.adc(r2, r0);
            r0 = v_3A4;
            r6 = self.adds(r0, r1);
            r0 = v_3A0;
            v_3A4 = r6;
            r0 = self.adcs(r0, r4);
            v_3A0 = r0;
            r2 = r0 | r8;
            r2 = self.ands(r2, r3);
            r3 = r0 & r8;
            lr = r2 | r3;
            r8 = r6;
            r3 = self.lsls(r0, 0x1E);
            r5 = self.lsrs(r0, 0x1C);
            r3 = r3 | self.chk(r8) >> 2;
            r5 = r5 | self.chk(r8) << 4;
            r3 = self.eors(r3, r5);
            r5 = self.lsls(r0, 0x19);
            r5 = r5 | self.chk(r8) >> 7;
            r2 = self.chk(r8) >> 28;
            r12 = r3 ^ r5;
            r5 = self.lsrs(r0, 2);
            r5 = r5 | self.chk(r8) << 30;
            r2 = r2 | self.chk(r0) << 4;
            r2 = self.eors(r2, r5);
            r5 = self.lsrs(r0, 7);
            r3 = v_384;
            r5 = r5 | self.chk(r8) << 25;
            r6 = v_3A8;
            r2 = self.eors(r2, r5);
            r5 = r8 | r3;
            r5 = self.ands(r5, r6);
            r6 = r8 & r3;
            r5 = self.orrs(r5, r6);
            r0 = self.adds(r2, r5);
            v_398 = r0;
            r2 = v_388;
            r12 = self.adc(r12, lr);
            r0 = v_370;
            r3 = self.adds(r1, r2);
            r1 = v_380;
            r8 = self.adc(r4, r0);
            r0 = r3;
            r2 = self.chk(r8) >> 18;
            r3 = self.chk(r8) >> 14;
            r2 = r2 | self.chk(r0) << 14;
            r3 = r3 | self.chk(r0) << 18;
            r2 = self.eors(r2, r3);
            r3 = self.chk(r8) << 23;
            r3 = r3 | self.chk(r0) >> 9;
            r4 = self.lsrs(r0, 0xE);
            lr = r2 ^ r3;
            r3 = self.lsrs(r0, 0x12);
            r3 = r3 | self.chk(r8) << 14;
            r4 = r4 | self.chk(r8) << 18;
            r3 = self.eors(r3, r4);
            r4 = self.chk(r8) >> 9;
            r4 = r4 | self.chk(r0) << 23;
            r2 = r0;
            r0 = v_37C;
            r3 = self.eors(r3, r4);
            v_388 = r2;
            r4 = r0 ^ r11;
            r0 = v_39C;
            r4 = r4 & r8;
            r5 = r1 ^ r0;
            r4 = r4 ^ r11;
            r5 = self.ands(r5, r2);
            r5 = self.eors(r5, r0);
            r6 = K_TABLE[0x10 * index + 10];
            r1 = self.adds(r6, r10);
            r6 = K_TABLE[0x10 * index + 11];
            r0 = v_378;
            r6 = self.adcs(r6, r0);
            r2 = sched[0x10 * index + 11];
            r1 = self.adds(r1, r2);
            r2 = sched[0x10 * index + 10];
            r0 = v_398;
            r2 = self.adcs(r2, r6);
            r1 = self.adds(r1, r5);
            r2 = self.adcs(r2, r4);
            r1 = self.adds(r1, r3);
            r4 = self.adc(r2, lr);
            r6 = v_3A0;
            r0 = self.adds(r0, r1);
            r9 = self.adc(r12, r4);
            r3 = v_36C;
            r2 = r9 | r6;
            r5 = self.chk(r9) >> 28;
            v_374 = r9;
            r2 = self.ands(r2, r3);
            r3 = r9 & r6;
            r10 = r2 | r3;
            r3 = self.chk(r9) << 30;
            r3 = r3 | self.chk(r0) >> 2;
            r5 = r5 | self.chk(r0) << 4;
            r3 = self.eors(r3, r5);
            r5 = self.chk(r9) << 25;
            r5 = r5 | self.chk(r0) >> 7;
            r6 = self.lsrs(r0, 0x1C);
            r12 = r3 ^ r5;
            r5 = self.chk(r9) >> 2;
            r5 = r5 | self.chk(r0) << 30;
            r6 = r6 | self.chk(r9) << 4;
            r5 = self.eors(r5, r6);
            r6 = self.chk(r9) >> 7;
            r3 = v_3A4;
            r6 = r6 | self.chk(r0) << 25;
            r2 = v_384;
            r5 = self.eors(r5, r6);
            r6 = r0 | r3;
            r6 = self.ands(r6, r2);
            r2 = r0 & r3;
            r2 = r2 | r6;
            r2 = self.adds(r2, r5);
            v_398 = r2;
            r2 = self.adc(r12, r10);
            v_378 = r2;
            r2 = v_38C;
            r12 = self.adds(r1, r2);
            r1 = v_390;
            lr = self.adc(r4, r1);
            r4 = self.chk(r12) >> 14;
            r1 = self.chk(lr) >> 18;
            r2 = self.chk(lr) >> 14;
            r1 = r1 | self.chk(r12) << 14;
            r2 = r2 | self.chk(r12) << 18;
            r1 = self.eors(r1, r2);
            r2 = self.chk(lr) << 23;
            r2 = r2 | self.chk(r12) >> 9;
            r4 = r4 | self.chk(lr) << 18;
            r1 = self.eors(r1, r2);
            r2 = self.chk(r12) >> 18;
            r2 = r2 | self.chk(lr) << 14;
            v_390 = r1;
            r2 = self.eors(r2, r4);
            r4 = self.chk(lr) >> 9;
            r1 = v_37C;
            r4 = r4 | self.chk(r12) << 23;
            r10 = r2 ^ r4;
            r2 = v_388;
            r4 = r8 ^ r1;
            r4 = r4 & lr;
            r4 = self.eors(r4, r1);
            r1 = v_380;
            r5 = r2 ^ r1;
            r2 = v_39C;
            r5 = r5 & r12;
            r5 = self.eors(r5, r1);
            r6 = K_TABLE[0x10 * index + 12];
            r3 = K_TABLE[0x10 * index + 13];
            r6 = self.adds(r6, r2);
            r3 = self.adc(r3, r11);
            r1 = sched[0x10 * index + 13];
            r1 = self.adds(r1, r6);
            r6 = sched[0x10 * index + 12];
            r3 = self.adcs(r3, r6);
            r1 = self.adds(r1, r5);
            r3 = self.adcs(r3, r4);
            r5 = self.adds(r1, r10);
            r1 = v_390;
            r2 = self.adc(r3, r1);
            r1 = v_398;
            r3 = v_3A0;
            r10 = self.adds(r1, r5);
            r1 = v_378;
            v_378 = r0;
            r11 = self.adc(r1, r2);
            r6 = self.chk(r10) >> 28;
            r1 = r11 | r9;
            v_398 = r11;
            r1 = self.ands(r1, r3);
            r3 = r11 & r9;
            r9 = r1 | r3;
            r3 = self.chk(r11) << 30;
            r4 = self.chk(r11) >> 28;
            r3 = r3 | self.chk(r10) >> 2;
            r4 = r4 | self.chk(r10) << 4;
            r6 = r6 | self.chk(r11) << 4;
            r3 = self.eors(r3, r4);
            r4 = self.chk(r11) << 25;
            r4 = r4 | self.chk(r10) >> 7;
            r1 = v_3A4;
            r3 = self.eors(r3, r4);
            r4 = self.chk(r11) >> 2;
            r4 = r4 | self.chk(r10) << 30;
            v_39C = r10;
            r4 = self.eors(r4, r6);
            r6 = self.chk(r11) >> 7;
            r6 = r6 | self.chk(r10) << 25;
            r4 = self.eors(r4, r6);
            r6 = r10 | r0;
            r6 = self.ands(r6, r1);
            r1 = r10 & r0;
            r1 = self.orrs(r1, r6);
            r10 = lr;
            r0 = self.adds(r4, r1);
            v_390 = r0;
            r0 = self.adc(r3, r9);
            v_38C = r0;
            r0 = v_3A8;
            r9 = r12;
            r4 = self.adds(r5, r0);
            r0 = v_3AC;
            v_3A8 = r4;
            r0 = self.adcs(r0, r2);
            r3 = self.lsrs(r4, 0xE);
            v_3AC = r0;
            r1 = self.lsrs(r0, 0x12);
            r2 = self.lsrs(r0, 0xE);
            r1 = r1 | self.chk(r4) << 14;
            r2 = r2 | self.chk(r4) << 18;
            r1 = self.eors(r1, r2);
            r2 = self.lsls(r0, 0x17);
            r2 = r2 | self.chk(r4) >> 9;
            r3 = r3 | self.chk(r0) << 18;
            r11 = r1 ^ r2;
            r2 = self.lsrs(r4, 0x12);
            r2 = r2 | self.chk(r0) << 14;
            r2 = self.eors(r2, r3);
            r3 = self.lsrs(r0, 9);
            r3 = r3 | self.chk(r4) << 23;
            r2 = self.eors(r2, r3);
            r3 = lr ^ r8;
            r3 = self.ands(r3, r0);
            r0 = v_388;
            lr = r3 ^ r8;
            r5 = r12 ^ r0;
            r5 = self.ands(r5, r4);
            r3 = r0;
            r5 = self.eors(r5, r0);
            r4 = K_TABLE[0x10 * index + 14];
            r6 = K_TABLE[0x10 * index + 15];
            r0 = v_380;
            r4 = self.adds(r4, r0);
            r0 = v_37C;
            r6 = self.adcs(r6, r0);
            r0 = sched[0x10 * index + 14];
            r1 = sched[0x10 * index + 15];
            r1 = self.adds(r1, r4);
            r0 = self.adcs(r0, r6);
            r1 = self.adds(r1, r5);
            r0 = self.adc(r0, lr);
            r1 = self.adds(r1, r2);
            r2 = v_390;
            r0 = self.adc(r0, r11);
            r4 = r8;
            lr = self.adds(r2, r1);
            r2 = v_38C;
            r6 = r3;
            r12 = self.adc(r2, r0);
            r2 = v_384;
            r8 = self.adds(r1, r2);
            r2 = v_36C;
            r5 = self.adc(r0, r2);
        }
        let list_638 = [
            self.chk(lr),
            self.chk(r12),
            self.chk(v_39C),
            self.chk(v_398),
            self.chk(v_378),
            self.chk(v_374),
            self.chk(v_3A4),
            self.chk(v_3A0),
            self.chk(r8),
            self.chk(r5),
            self.chk(v_3A8),
            self.chk(v_3AC),
            self.chk(r9),
            self.chk(r10),
            self.chk(r6),
            self.chk(r4),
        ];
        for i in 0..8 {
            r0 = state[2 * i];
            r1 = state[2 * i + 1];
            r0 = self.adds(r0, list_638[2 * i]);
            r1 = self.adcs(r1, list_638[2 * i + 1]);
            state[2 * i] = r0;
            state[2 * i + 1] = r1;
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Python 参考实现 calculate(range(32)) 的已知输出（对拍锚点）。
    #[test]
    fn calculate_matches_python_reference() {
        let input: Vec<u8> = (0..32).collect();
        let out = TtHashCore::new().calculate(&input);
        assert_eq!(
            out.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "3d94eea49c580aef816935762be049559d6d1440dede12e6a125f1841fff8e6fa9d71862a3e5746b571be3d187b0041046f52ebd850c7cbd5fde8ee38473b649"
        );
    }

    /// 各长度向量的 Python 参考对拍（单块/多块/边界）。
    #[test]
    fn calculate_vectors_from_python() {
        let cases: Vec<(usize, &str)> = vec![
            (
                0,
                "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
            ),
            (
                1,
                "e45bf5817ddf94aa2f7a407071f0eedc6beb98f768b4cd33d1176d44d1563a45a5d7212290eb7670c6786b13591aedac86478993895e8b24e612014abaa6ba04",
            ),
            (
                19,
                "ae31ede5ae0b51a4748d925137e39bebcb0d83df818ea9b308eeb1d9fcb913598aa6f54b55ee9239651010e3fb1db506d04f9c556ba535ffed41c41553dd392a",
            ),
            (
                32,
                "c3902ef600c188f0a9b0d32d5e78edf886d61887e698a81aab084c8f86dbfe6f5c4ba5a226e2b0313a837747c1a09a56b1ec9f52479b6f9959ac1b0d0c3d3465",
            ),
            (
                39,
                "08673ac2515372c2be09b1b82bdf334a875e536ad3d5749c7ed03cf8d819e1df89787441d69ea7a579d65345c5b0fd3466ec8efa5f32a3215d8054d917c30958",
            ),
            (
                100,
                "364662a2cfeefa210252f45394a87e9ab0d268fe0c448b7e60d69888c8506824fa142beff9a337045417b0bcc5d2e1cecd65223d4d1078cd12d52a2f57b92ed9",
            ),
            (
                127,
                "548c0b30880e6b0f3bfd75510c12e3ac1b1a0579c37868b18d793358fecaac2400c67021b109ccaa87e711cab67bb51762a20ac0f01c10c0b9680834033ff9ea",
            ),
            (
                128,
                "99b16f17aa0b969a5b8f08f367719d516e330ccd2660b6f0688ec031dbc783de50a1cd185a2568dba75070a2403d17d4741d163578515dfd2ff756ddfe4d47b1",
            ),
            (
                200,
                "cca3c0276046ef9f2897bdfc3ec330f77f4959914b1462bd581b232ddb3e9aa98acf5f5a2b21c7f49d2e43721daa61a2b5cee6af6052dfeb766e66ddb0d1719c",
            ),
        ];
        for (n, expect) in cases {
            let input: Vec<u8> = (0..n).map(|i| ((i * 7 + 3) % 256) as u8).collect();
            let out = TtHashCore::new().calculate(&input);
            let got: String = out.iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(got, expect, "calculate 长度 {n} 不一致");
        }
    }

    /// 924 字节（8 块）真实 gz 输入的对拍。
    #[test]
    fn calculate_multi_block_924() {
        let gz_hex = include_str!("../domain/api/testdata/py_gz.hex");
        let gz: Vec<u8> = (0..gz_hex.trim().len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&gz_hex.trim()[i..i + 2], 16).unwrap())
            .collect();
        let out = TtHashCore::new().calculate(&gz);
        assert_eq!(
            out.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "d6b7290f6f6a5c4d98ee5b8f9ea82c73749ce8680ea1b847c7e69c6c4708b1cea1b06b473bdb82ef2a35583bee4317b022eb09362dba7a80b6592006e4129cc1"
        );
    }
}
