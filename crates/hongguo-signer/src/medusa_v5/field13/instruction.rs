use super::{CALLBACK, Field13Error, RETURN, Result, Vm};

#[derive(Clone, Copy)]
struct Instruction(u32);

impl Instruction {
    fn bits(self, offset: u32, width: u32) -> u32 {
        (self.0 >> offset) & ((1 << width) - 1)
    }

    fn bit(self, offset: u32) -> u32 {
        self.bits(offset, 1)
    }

    fn reg(self, offset: u32) -> usize {
        self.bits(offset, 5) as usize
    }

    fn reg_bit(self, offset: u32, high_bit: u32) -> usize {
        (self.bits(offset, 4) | self.bit(high_bit) << 4) as usize
    }

    fn immediate(self, low: u32, middle: u32, high: u32, sign: u32) -> u32 {
        self.bits(low, 5)
            | self.bits(middle, 5) << 5
            | self.bits(high, 5) << 10
            | self.bit(sign) << 15
    }

    fn address(self, register: u64, immediate: u32) -> u64 {
        register.wrapping_add_signed(i64::from(immediate as i16))
    }
}

fn signed_word(value: u64) -> u64 {
    (value as i32 as i64) as u64
}

impl Vm<'_> {
    pub(super) fn step(&mut self) -> Result<()> {
        let op = Instruction(self.mem.read(self.pc, 4) as u32);
        let next = match op.bits(0, 6) {
            4 => {
                self.bits(op)?;
                self.pc + 4
            }
            13 | 20 | 29 | 43 | 45 => self.branch(op),
            17 => self.alu(op)?,
            1 | 16 | 19 | 22 | 24 | 26 | 28 | 31 | 40 | 47 | 49 | 61 | 62 | 63 => {
                self.transfer(op)?;
                self.pc + 4
            }
            6 | 15 | 33 | 48 | 52 | 53 | 55 | 59 => {
                self.immediate(op)?;
                self.pc + 4
            }
            _ => return Err(self.unsupported(op)),
        };
        self.pc = next;
        self.regs[0] = 0;
        Ok(())
    }

    fn unsupported(&self, op: Instruction) -> Field13Error {
        Field13Error::UnhandledInstruction {
            location: self.pc,
            word: op.0,
        }
    }

    fn bits(&mut self, op: Instruction) -> Result<()> {
        let [a, b, c, d] = [12, 17, 22, 27].map(|offset| op.reg(offset));
        let regs = &mut self.regs;
        regs[d] = match op.bits(6, 6) {
            11 => (regs[c] >> a) & ((1 << (b + 1)) - 1),
            22 => {
                let value = regs[a] as u32;
                signed_word(u64::from(if c == 31 {
                    value
                } else {
                    (value >> b) & ((1 << (c + 1)) - 1)
                }))
            }
            34 => match a {
                2 => signed_word(u64::from((regs[b] as u32).swap_bytes().rotate_left(16))),
                16 => regs[c] as i8 as i64 as u64,
                24 => regs[d] as i16 as i64 as u64,
                _ => return Err(self.unsupported(op)),
            },
            _ => return Err(self.unsupported(op)),
        };
        Ok(())
    }

    fn branch(&self, op: Instruction) -> u64 {
        let regs = &self.regs;
        let next = self.pc + 4;
        let (take, displacement) = match op.bits(0, 6) {
            13 => {
                let target = op.immediate(11, 6, 16, 26) & 0x7fff
                    | op.bits(26, 5) << 15
                    | op.bits(21, 5) << 20
                    | op.bit(31) << 25;
                return self.entry + u64::from(target) * 4;
            }
            20 => (
                regs[op.reg_bit(12, 31)] != regs[op.reg(7)],
                op.immediate(26, 21, 16, 6),
            ),
            29 => (
                regs[op.reg_bit(7, 31)] as i64 <= 0,
                op.immediate(11, 26, 21, 16),
            ),
            43 => (
                regs[op.reg_bit(7, 31)] as i64 > 0,
                op.bits(11, 10) | op.bits(26, 5) << 10 | op.bit(21) << 15,
            ),
            45 => (
                regs[op.reg_bit(22, 31)] == regs[op.reg_bit(7, 21)],
                op.immediate(16, 26, 11, 6),
            ),
            _ => unreachable!(),
        };
        if take {
            next.wrapping_add_signed(i64::from(displacement as i16) * 4)
        } else {
            next
        }
    }

    fn alu(&mut self, op: Instruction) -> Result<u64> {
        let [a, b, c, d] = [12, 17, 22, 27].map(|offset| op.reg(offset));
        let regs = &mut self.regs;
        let next = self.pc + 4;
        let sub = op.bits(6, 6);
        match sub {
            0 => regs[a] = (regs[b] as i32 >> d) as i64 as u64,
            3 => regs[b] = signed_word(regs[d] << c),
            7 if d & 1 != 0 => {
                regs[(d & 30) | (a & 1)] =
                    signed_word(u64::from((regs[b] as u32).rotate_right(c as u32)))
            }
            7 => regs[b] = u64::from(regs[a] as u32 >> ((d & 30) | (c & 1))),
            10 => regs[a] = regs[c] & regs[d],
            12 => regs[c] = regs[d] << (32 + a),
            13 => {}
            14 => regs[b] = regs[a].wrapping_add(regs[c]),
            16 => regs[a] = regs[b].wrapping_sub(regs[d]),
            18 if a & 1 != 0 => {
                let source = regs[(a & 30) | (b & 1)] as u32;
                regs[d] = signed_word(u64::from(source.rotate_right(regs[c] as u32 & 31)));
            }
            18 => regs[c] = u64::from(regs[(a & 30) | (d & 1)] as u32 >> (regs[b] & 31)),
            23 => regs[a] = regs[b] << c,
            29 => regs[d] = regs[a] ^ regs[b],
            30 | 50 => {
                if sub == 50 {
                    regs[b] = next;
                }
                let callback_args = regs[5];
                match regs[c] {
                    CALLBACK => self.callback(callback_args)?,
                    RETURN => self.stopped = true,
                    target => return Ok(target),
                }
            }
            32 => {
                if regs[b] != 0 {
                    regs[d] = regs[c];
                }
            }
            33 if d & 1 != 0 => regs[a] = regs[c].rotate_right(b as u32),
            33 => regs[a] = regs[(d & 30) | (c & 1)] >> b,
            34 => regs[a] = signed_word(regs[d] << (regs[c] & 31)),
            38 if d & 1 != 0 => regs[(d & 30) | (b & 1)] = regs[c].rotate_right(32 + a as u32),
            38 => regs[a] = regs[c] >> (32 + b),
            43 => regs[a] = u64::from((regs[c] as i64) < (regs[d] as i64)),
            44 => regs[a] = regs[c] | regs[d],
            45 if a & 1 == 0 => regs[d] = regs[(a & 30) | (b & 1)] >> (regs[c] & 63),
            46 => regs[c] = signed_word(regs[b].wrapping_sub(regs[a])),
            48 => regs[c] = !(regs[a] | regs[d]),
            51 => regs[a] = signed_word(regs[c].wrapping_add(regs[d])),
            52 => regs[b] = self.hi,
            54 => regs[b] = u64::from(regs[a] < regs[d]),
            57 => self.hi = (((regs[a] as u32 as u64 * regs[c] as u32 as u64) as i64) >> 32) as u64,
            58 => regs[c] = regs[b] << (regs[a] & 63),
            _ => return Err(self.unsupported(op)),
        }
        Ok(next)
    }

    fn immediate(&mut self, op: Instruction) -> Result<()> {
        let regs = &mut self.regs;
        let i = op.0;
        match op.bits(0, 6) {
            6 => {
                let immediate = op.bits(6, 5) | (i >> 16) & 0x7fe0 | (i >> 1) & 0x8000;
                regs[op.reg_bit(17, 11)] = regs[op.reg_bit(12, 31)] ^ u64::from(immediate);
            }
            15 => {
                let immediate = op.bits(6, 10) | op.bits(26, 5) << 10 | op.bit(16) << 15;
                regs[op.reg(17)] = op.address(regs[op.reg_bit(22, 31)], immediate);
            }
            33 => {
                let immediate = op.immediate(11, 6, 26, 21) as i16 as i64 as u64;
                regs[op.reg_bit(22, 16)] = u64::from(regs[op.reg_bit(17, 31)] < immediate);
            }
            48 => {
                let immediate = op.bits(16, 5) | (i >> 1) & 0x7fe0 | (i >> 6) & 0x8000;
                regs[op.reg(22)] = regs[op.reg(27)] | u64::from(immediate);
            }
            52 => {
                regs[op.reg_bit(22, 11)] =
                    (i64::from(op.immediate(16, 26, 6, 21) as i16) << 16) as u64
            }
            53 => {
                regs[op.reg_bit(22, 11)] =
                    regs[op.reg_bit(12, 31)] & u64::from(op.immediate(16, 26, 6, 21))
            }
            55 => {
                let immediate = ((i >> 6) & 0x7c1f | (i >> 16) & 0x3e0 | (i >> 11) & 0x8000) as i16;
                regs[op.reg_bit(27, 11)] =
                    u64::from((regs[op.reg_bit(12, 31)] as i64) < i64::from(immediate));
            }
            59 => {
                let immediate = op.immediate(21, 11, 26, 6);
                regs[op.reg_bit(7, 16)] =
                    op.address(signed_word(regs[op.reg_bit(17, 31)]), immediate);
            }
            _ => return Err(self.unsupported(op)),
        }
        Ok(())
    }

    fn transfer(&mut self, op: Instruction) -> Result<()> {
        let r = &mut self.regs;
        let q = r[op.reg_bit(22, 31)];
        let t = op.reg_bit(27, 21);
        match op.bits(0, 6) {
            1 => {
                let address = op.address(r[op.reg_bit(12, 31)], op.immediate(21, 6, 26, 16));
                r[op.reg_bit(17, 11)] = signed_word(self.mem.read(address, 4));
            }
            16 => {
                let immediate = op.bits(16, 5) | (op.0 >> 1) & 0x7fe0 | (op.0 >> 11) & 0x8000;
                self.mem.write(op.address(q, immediate), r[t], 1)?;
            }
            22 => {
                let address = op.address(r[op.reg_bit(17, 31)], op.immediate(11, 21, 6, 26));
                self.mem.write(address, r[op.reg_bit(27, 16)], 4)?;
            }
            24 => r[t] = self.mem.read(op.address(q, op.immediate(6, 16, 11, 26)), 8),
            26 => self
                .mem
                .write(op.address(q, op.immediate(16, 11, 6, 26)), r[t], 8)?,
            31 => {
                let immediate = op.bits(16, 15) | op.bit(11) << 15;
                let address = op.address(r[op.reg_bit(7, 31)], immediate);
                r[op.reg_bit(12, 6)] = self.mem.read(address, 2);
            }
            40 => {
                let immediate = op.bits(16, 5) | op.bits(26, 5) << 5 | op.bits(6, 6) << 10;
                r[op.reg_bit(12, 21)] = self.mem.read(op.address(q, immediate), 1);
            }
            61 => {
                r[t] =
                    self.mem.read(op.address(q, op.immediate(16, 11, 6, 26)), 1) as i8 as i64 as u64
            }
            _ => return self.unaligned(op),
        }
        Ok(())
    }

    fn unaligned(&mut self, op: Instruction) -> Result<()> {
        let r = &mut self.regs;
        match op.bits(0, 6) {
            19 => {
                let immediate = op.bits(16, 15) | op.bit(11) << 15;
                let address = op.address(r[op.reg_bit(7, 31)], immediate);
                let shift = (address & 7) * 8;
                let existing = self.mem.read(address & !7, 8);
                let value = (existing & ((1 << shift) - 1)) | (r[op.reg_bit(12, 6)] << shift);
                self.mem.write(address & !7, value, 8)?;
            }
            28 => {
                let address = op.address(r[op.reg_bit(17, 31)], op.immediate(26, 11, 21, 6));
                let dest = op.reg_bit(7, 16);
                let shift = (address & 7) * 8;
                let value = self.mem.read(address & !7, 8);
                r[dest] = (value << (56 - shift)) | (r[dest] & (0x00ff_ffff_ffff_ffff >> shift));
            }
            47 => {
                let address = op.address(r[op.reg_bit(12, 31)], op.immediate(16, 6, 26, 21));
                let dest = op.reg_bit(22, 11);
                let shift = (address & 3) * 8;
                let value = self.mem.read(address & !3, 4);
                r[dest] = signed_word((value << (24 - shift)) | (r[dest] & (0xff_ffff >> shift)));
            }
            49 => {
                let immediate = op.bits(16, 5) | op.bits(26, 5) << 5 | op.bits(6, 6) << 10;
                let address = op.address(r[op.reg_bit(22, 31)], immediate);
                let dest = op.reg_bit(12, 21);
                let shift = (address & 7) * 8;
                let keep = u64::MAX.checked_shl((64 - shift) as u32).unwrap_or(0);
                r[dest] = (self.mem.read(address & !7, 8) >> shift) | (r[dest] & keep);
            }
            62 => {
                let immediate = op.bits(16, 10) | op.bits(6, 6) << 10;
                let address = op.address(r[op.reg(27)], immediate);
                let shift = (address & 7) * 8;
                let keep = 0xffff_ffff_ffff_ff00_u64 << shift;
                let existing = self.mem.read(address & !7, 8);
                let value = (existing & keep) | (r[op.reg_bit(12, 26)] >> (56 - shift));
                self.mem.write(address & !7, value, 8)?;
            }
            63 => {
                let address = op.address(r[op.reg(27)], op.immediate(6, 16, 11, 21));
                let dest = op.reg(22);
                let shift = (address & 3) * 8;
                let keep = u32::MAX.checked_shl((32 - shift) as u32).unwrap_or(0);
                r[dest] = signed_word(
                    (self.mem.read(address & !3, 4) >> shift) | (r[dest] & u64::from(keep)),
                );
            }
            _ => return Err(self.unsupported(op)),
        }
        Ok(())
    }
}
