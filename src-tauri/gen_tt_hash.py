"""把 TTEncrypt V5 参考实现（Python）机械翻译成 Rust（tt_hash.rs）。

一次性工具：hex_27e/hex_30a（魔改哈希的调度与压缩）、hex_cf8/hex_0a2
（自定义分组加密的轮密钥扩展与 CBC 加密）逐句自动转换；行为对拍由
Rust 测试兜底（calculate 向量 + Python golden）。重生成后手工步骤：
无（pub 化已内置）。
"""
import io
import re
import tempfile

SRC = tempfile.gettempdir() + '/ttencrypt.py'
OUT = 'src/signer/tt_hash.rs'
TAB = chr(9)
NL = chr(10)

src = open(SRC, encoding='utf-8').read()


def get_fn(name):
    m = re.search(r'    def %s\(self[^)]*\):\n(.*?)(?=\n    def |\n\Z)' % name, src, re.S)
    assert m, name
    return m.group(1)


def preprocess(body):
    # `if X: break else: <块>` 折叠成 `if not(X): <块>`
    lines = body.splitlines(keepends=True)
    out = []
    i = 0
    while i < len(lines):
        ln = lines[i]
        m = re.match(r'(\s*)if (.+):\n', ln)
        if m and i + 2 < len(lines):
            brk = re.match(r'\s*break\s*\n', lines[i + 1])
            els = re.match(r'\s*else:\s*\n', lines[i + 2])
            if brk and els:
                out.append(m.group(1) + 'if not (' + m.group(2) + '):\n')
                i += 3
                continue
        out.append(ln)
        i += 1
    return ''.join(out)


def convert_expr(e):
    e = e.strip()
    for op in ['ADCS', 'ADC', 'ADDS', 'LSRS', 'LSLS', 'EORS', 'ANDS', 'ORRS', 'RRX']:
        e = re.sub(r'self\.%s\(' % op, 'self.%s(' % op.lower(), e)
    e = re.sub(r'self\.check\(', 'self.chk(', e)
    e = re.sub(r'self\.UBFX\(', 'self.ubfx(', e)
    e = re.sub(r'self\.UTFX\(', 'self.utfx(', e)
    e = re.sub(r'self\.dump_list\(', 'self.dump_list(', e)
    e = re.sub(r'self\.hex_list\(', 'Self::hex_list(&', e)
    for i in range(10):
        e = e.replace('self.dword_%d[' % i, 'DWORD_%d[(' % i)
    e = re.sub(r'DWORD_(\d+)\[\(([^)]+)\)\]', r'DWORD_\1[(\2) as usize]', e)
    e = re.sub(r'\bint\(', '(', e)
    e = re.sub(r'\bLR\b', 'lr', e)
    e = re.sub(r'\blist_55C\b', 'list_55c', e)
    e = re.sub(r'\bR(\d+)\b', lambda m: 'r' + m.group(1), e)
    e = e.replace('self.rodata[', 'K_TABLE[')
    e = re.sub(r'\(self\.parseLong\(self\.toHex\((\w+)\) \+ "000000", 10, 16\)\)',
               r'((\1) << 24)', e)
    e = re.sub(r'\(self\.parseLong\(self\.toHex\((\w+)\) \+ "0000", 10, 16\)\)',
               r'((\1) << 16)', e)
    e = re.sub(r'\(self\.parseLong\(self\.toHex\((\w+)\) \+ "00", 10, 16\)\)',
               r'((\1) << 8)', e)
    return e


def convert_fn(body_py):
    body_py = preprocess(body_py)
    out = []
    for raw in body_py.splitlines():
        if not raw.strip():
            continue
        py_ind = (len(raw) - len(raw.lstrip())) // 4
        rust_ind = TAB * (1 + py_ind)
        code = raw.strip()
        m = re.match(r'for (\w+) in range\(([^:]+)\):$', code)
        if m:
            out.append(rust_ind + 'for %s in 0..(%s) {' % (m.group(1), convert_expr(m.group(2))))
            continue
        m = re.match(r'if (.+):$', code)
        if m:
            out.append(rust_ind + 'if !(%s) {' % convert_expr(m.group(1)))
            continue
        if code == 'break':
            continue
        st = convert_expr(code)
        st = st.replace('result = result + list_740;',
                        'result.extend_from_slice(&list_740);')
        if '=' in st and not st.startswith('//'):
            st += ';'
        out.append(rust_ind + st)
    fixed = []
    prev_ind = 1
    for ln in out:
        cur = len(ln) - len(ln.lstrip(TAB))
        while cur < prev_ind:
            fixed.append(TAB * (prev_ind - 1) + '}')
            prev_ind -= 1
        fixed.append(ln)
        prev_ind = cur
    while prev_ind > 1:
        fixed.append(TAB * (prev_ind - 1) + '}')
        prev_ind -= 1
    return NL.join(fixed)


def extract_array(name):
    txt = re.search(name + r' = \[(.*?)\]', src, re.S).group(1)
    nums = [n.strip() for n in txt.replace(NL, ' ').split(',') if n.strip()]
    body = NL.join(', '.join(nums[i:i + 8]) + ',' for i in range(0, len(nums), 8))
    return body, len(nums)


def decls_for(body, skip):
    targets = set()
    for ln in body.splitlines():
        mm = re.match(r'\s*([A-Za-z_][A-Za-z0-9_]*) = ', ln)
        if mm and mm.group(1) not in skip:
            targets.add(mm.group(1))
    return NL.join('let mut %s: u32 = 0;' % t for t in sorted(targets))


def postprocess_rs(txt):
    txt = re.sub(r'\s*result = \[\];\n', NL, txt)
    txt = re.sub(r'\s*l55cl = len\(list_55c\);\n', NL, txt)
    txt = re.sub(r'\s*lens = len\(content\);\n', NL, txt)
    txt = re.sub(r'\s*end = lens // 16;\n', NL, txt)
    txt = re.sub(r'\s*end = lens / 16;\n', NL, txt)
    txt = re.sub(r'\s*l388l = len\(out_words\);\n', NL, txt)
    txt = re.sub(r'\s*list_378 = list_378;\n', NL, txt)
    txt = re.sub(r'\s*out_words = \[\];\n', NL, txt)
    txt = re.sub(r'\s*list_478 = \[\];\n', NL, txt)
    txt = re.sub(r'(list_478\.append\(r\d\))\n', r'\1;' + NL, txt)
    txt = re.sub(r'list_478\.append\((r\d+)\)', r'list_478.push(\1)', txt)
    txt = re.sub(r'\bstate\[([a-z0-9_]+)\]', r'state[(\1) as usize]', txt)
    txt = re.sub(r'out_words\[l388l', r'out_words[l388l_usize', txt)
    txt = txt.replace('let r5 = out_words[l388l_usize', 'r5 = out_words[l388l_usize')
    txt = txt.replace('list_378 = [r3, r2, r4, r5];', 'list_378 = vec![r3, r2, r4, r5];')
    txt = txt.replace('out_words = out_words + list_378;',
                      'out_words.extend_from_slice(&list_378);')
    txt = txt.replace('list_740 = Self::hex_list(&[r0, r1, r12, r2]);',
                      'list_740 = Self::hex_list(&[r0, r1, r12, r2]);')
    txt = txt.replace('if !(not (', 'if !((')
    # 裸形 DWORD 残留兜底（形如 DWORD_0[(r6] 缺 ) as usize]）
    txt = re.sub(r'DWORD_(\d+)\[\(([^)\n]+)\](?!\))',
                 r'DWORD_\1[(\2) as usize]', txt)
    txt = txt.replace('result  # WORKED', 'result')
    txt = re.sub(r'for (\w+) in 0\.\.\(([^)]+)\) \{', r'for \1 in 0..\2 {', txt)
    # list_638 多行字面量：转换器加错分号与闭合
    txt = txt.replace('let list_638 = [;', 'let list_638 = [')
    txt = re.sub(r'\}' + NL + r'(\s*)\]', '];', txt, count=1)
    return txt


def build():
    r27 = postprocess_rs(convert_fn(preprocess(get_fn('hex_27E'))))
    r27 = re.sub(r'\bparam_list\b', 'p', r27)
    r27 = 'let mut p = p;' + NL + r27
    r27 = r27.replace('p = p + [r3, r0];', '{ p.push(r3); p.push(r0); }')
    r27 = r27.replace('return p  # WORKED', 'p').replace('return p;', 'p')
    # 收尾悬挂：保留最后 `p` 表达式
    lines27 = r27.strip(NL).splitlines()
    out27 = []
    seen_p = False
    for ln in lines27:
        if seen_p:
            continue
        if ln.strip() == 'p':
            seen_p = True
            out27.append(TAB * 2 + 'p')
            continue
        out27.append(ln)
    r27 = NL.join(out27)
    r27 = decls_for(r27, {'p'}) + NL + r27

    r30 = postprocess_rs(convert_fn(preprocess(get_fn('hex_30A'))))
    r30 = re.sub(r'\bparam_list\b', 'state', r30)
    r30 = re.sub(r'\blist_3B8\b', 'sched', r30)
    r30 = 'let mut state = p0;' + NL + r30
    r30 = re.sub(r'list_638 = \[;', 'let list_638 = [', r30)
    r30 = r30.replace('return state;', 'state').replace('return state', 'state')
    lines30 = r30.strip(NL).splitlines()
    out30 = []
    seen_state = False
    for ln in lines30:
        if seen_state:
            continue
        if ln.strip() == 'state':
            seen_state = True
            out30.append(TAB * 2 + 'state')
            continue
        out30.append(ln)
    r30 = NL.join(out30)
    r30 = decls_for(r30, {'state'}) + NL + r30

    rcf8 = postprocess_rs(convert_fn(preprocess(get_fn('hex_CF8'))))
    rcf8 = re.sub(r'\bparam_list\b', 'list_378', rcf8)
    rcf8 = re.sub(r'\blist_388\b', 'out_words', rcf8)
    rcf8 = re.sub(r'list_468 = list_378 \+ out_words;',
                  'list_468.extend_from_slice(&list_378);'
                  + NL + '\t\tlist_468.extend_from_slice(&out_words);', rcf8)
    rcf8 = re.sub(r'return list_468;?', 'list_468', rcf8)
    rcf8 = ('let mut list_378 = list_378.to_vec();' + NL
            + 'let original_key = list_378.clone();' + NL
            + 'let mut list_468: Vec<u32> = Vec::new();' + NL
            + 'let mut list_478: Vec<u32> = Vec::new();' + NL
            + 'let mut out_words: Vec<u32> = Vec::new();' + NL) + rcf8
    rcf8 = decls_for(rcf8, {'list_378', 'list_468', 'list_478', 'out_words',
                            'original_key', 'l388l_usize'}) + NL + rcf8

    r0a2 = postprocess_rs(convert_fn(preprocess(get_fn('hex_0A2'))))
    r0a2 = ('let mut result: Vec<u8> = Vec::new();' + NL
            + 'let l55cl = list_55c.len();' + NL
            + 'let lens = content.len();' + NL
            + 'let end = lens / 16;' + NL) + r0a2
    r0a2 = r0a2.replace('tmp_list = self.dump_list(list_740);',
                        'let tmp_list = self.dump_list(&list_740);')
    r0a2 = re.sub(r'return result;?', 'result', r0a2)
    # 收尾悬挂：保留最后 `result` 表达式
    lines_a2 = r0a2.strip(NL).splitlines()
    out_a2 = []
    seen_res = False
    for ln in lines_a2:
        if seen_res:
            continue
        if ln.strip() == 'result':
            seen_res = True
            out_a2.append(TAB * 2 + 'result')
            continue
        out_a2.append(ln)
    r0a2 = NL.join(out_a2)
    r0a2 = decls_for(r0a2, {'result', 'l55cl', 'lens', 'end', 'tmp_list',
                            'list_740', 'content', 'list_55c'}) + NL + r0a2

    k_body, k_n = extract_array('rodata')
    iv_body, iv_n = extract_array('LIST_6B0')
    ord_body, ord_n = extract_array('ord_list')
    tables = []
    for i in range(10):
        b, n = extract_array('dword_%d' % i)
        tables.append('/// 参考实现 dword_%d（固件逆向的自定义表，非标准 AES）'
                      % i + NL + 'const DWORD_%d: [u32; %d] = [' % (i, n)
                      + NL + b + NL + '];')
    tables = NL.join(tables)

    L = []
    L.append('//! TTEncrypt V5 的魔改哈希与分组加密（Python 参考实现的机械翻译）。')
    L.append('!')
    L.append('//! 函数体保持与参考实现逐句对应（不优化不改写），因此允许机械')
    L.append('//! 翻译固有的风格告警。')
    L.append('#![allow(clippy::assign_op_pattern, clippy::unnecessary_parens,')
    L.append('        clippy::same_item_push, clippy::range_plus_one, unused_assignments)]')
    L.append('//!')
    L.append('//! - TtHashCore::calculate：SHA-512 骨架的哈希变体（128 字节块、')
    L.append('//!   64 字节输出），K/IV 被平台魔改，用本文件的表；')
    L.append('//! - hex_cf8 / hex_0a2：自定义分组加密的轮密钥扩展与 CBC 形态加密')
    L.append('//!   （含 10 张非标准 S-box/T-table）。')
    L.append('//!')
    L.append('//! 函数体是 gen_tt_hash.py 逐句自动转换（不要手改，重生成即可），')
    L.append('//! 行为对拍由 calculate 向量测试 + Python golden 测试兜底。')
    L.append('')
    L.append('/// 魔改 K 表（160 个 u32 = 80 个 u64）')
    L.append('const K_TABLE: [u32; %d] = [' % k_n)
    L.append(k_body)
    L.append('];')
    L.append('')
    L.append('/// 初始状态（16 个 u32 = 8 个 u64；hex_c52 输出时高低字交换）')
    L.append('const TT_IV: [u32; %d] = [' % iv_n)
    L.append(iv_body)
    L.append('];')
    L.append('')
    L.append('/// TT-Encrypt V5 密钥派生盐（与版本一起固定）')
    L.append('pub const TT_ORD_LIST: [u8; %d] = [' % ord_n)
    L.append(ord_body)
    L.append('];')
    L.append('')
    L.append(tables)
    L.append('''
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
        Self { cf: 0, block_bit_pos: 0 }
    }

    #[inline]
    fn chk(&self, x: u32) -> u32 { x }

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
    fn eors(&self, a: u32, b: u32) -> u32 { a ^ b }

    #[inline]
    fn ands(&self, a: u32, b: u32) -> u32 { a & b }

    #[inline]
    fn orrs(&self, a: u32, b: u32) -> u32 { a | b }

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
    fn utfx(&self, num: u32) -> u32 { num & 0xff }

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
        content
            .chunks_exact(4)
            .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
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
''' + r27 + '''
    }

    /// 压缩函数（hex_30A 原样翻译）
    fn hex_30a(&mut self, p0: [u32; 16], sched: &[u32]) -> [u32; 16] {
''' + r30 + '''
    }

    /// 参考实现 hex_CF8 原样翻译：轮密钥扩展（非标准 AES schedule）
    pub fn hex_cf8(&mut self, list_378: &[u32]) -> Vec<u32> {
''' + rcf8 + '''
    }

    /// 参考实现 hex_0A2 原样翻译：自定义分组加密（CBC 形态的链式结构）
    pub fn hex_0a2(&mut self, mut content: Vec<u8>, mut list_740: Vec<u8>,
                   list_55c: &[u32]) -> Vec<u8> {
''' + r0a2 + '''
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
            (0, "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"),
            (1, "e45bf5817ddf94aa2f7a407071f0eedc6beb98f768b4cd33d1176d44d1563a45a5d7212290eb7670c6786b13591aedac86478993895e8b24e612014abaa6ba04"),
            (19, "ae31ede5ae0b51a4748d925137e39bebcb0d83df818ea9b308eeb1d9fcb913598aa6f54b55ee9239651010e3fb1db506d04f9c556ba535ffed41c41553dd392a"),
            (32, "c3902ef600c188f0a9b0d32d5e78edf886d61887e698a81aab084c8f86dbfe6f5c4ba5a226e2b0313a837747c1a09a56b1ec9f52479b6f9959ac1b0d0c3d3465"),
            (39, "08673ac2515372c2be09b1b82bdf334a875e536ad3d5749c7ed03cf8d819e1df89787441d69ea7a579d65345c5b0fd3466ec8efa5f32a3215d8054d917c30958"),
            (100, "364662a2cfeefa210252f45394a87e9ab0d268fe0c448b7e60d69888c8506824fa142beff9a337045417b0bcc5d2e1cecd65223d4d1078cd12d52a2f57b92ed9"),
            (127, "548c0b30880e6b0f3bfd75510c12e3ac1b1a0579c37868b18d793358fecaac2400c67021b109ccaa87e711cab67bb51762a20ac0f01c10c0b9680834033ff9ea"),
            (128, "99b16f17aa0b969a5b8f08f367719d516e330ccd2660b6f0688ec031dbc783de50a1cd185a2568dba75070a2403d17d4741d163578515dfd2ff756ddfe4d47b1"),
            (200, "cca3c0276046ef9f2897bdfc3ec330f77f4959914b1462bd581b232ddb3e9aa98acf5f5a2b21c7f49d2e43721daa61a2b5cee6af6052dfeb766e66ddb0d1719c"),
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
''')
    text = NL.join(L)
    io.open(OUT, 'w', encoding='utf-8', newline=NL).write(text)
    print('tt_hash.rs 重新生成完毕')


if __name__ == '__main__':
    build()
