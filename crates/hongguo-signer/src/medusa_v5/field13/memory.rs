use super::{Field13, Field13Error, Result};

const BSS: u64 = 0x3ec9f0;
const BSS_END: u64 = 0x467560;
const SCRATCH: u64 = 0x700000;
const SCRATCH_END: u64 = 0x900000;

pub(super) struct Memory<'a> {
    source: &'a Field13,
    bss: Vec<u8>,
    scratch: Vec<u8>,
    heap: u64,
}

impl<'a> Memory<'a> {
    pub(super) fn new(source: &'a Field13) -> Self {
        let mut bss = vec![0; (BSS_END - BSS) as usize];
        let initialized = source.image.len() - BSS as usize;
        let first = BSS as usize - 0x8000;
        bss[..initialized].copy_from_slice(&source.image[first..first + initialized]);
        Self {
            source,
            bss,
            scratch: vec![0; (SCRATCH_END - SCRATCH) as usize],
            heap: 0x750000,
        }
    }

    pub(super) fn read(&self, address: u64, size: usize) -> u64 {
        if size == 8
            && let Some(value) = self.source.relocations.get(&address)
        {
            return *value;
        }
        let mut bytes = [0; 8];
        self.copy_to(address, &mut bytes[..size]);
        u64::from_le_bytes(bytes)
    }

    pub(super) fn copy_to(&self, address: u64, out: &mut [u8]) {
        for (offset, value) in out.iter_mut().enumerate() {
            *value = self.byte(address.wrapping_add(offset as u64));
        }
    }

    fn byte(&self, address: u64) -> u8 {
        match address {
            BSS..BSS_END => self.bss[(address - BSS) as usize],
            SCRATCH..SCRATCH_END => self.scratch[(address - SCRATCH) as usize],
            _ => {
                let file_offset = if address >= 0x3c4ef0 {
                    address - 0x4000
                } else {
                    address
                };
                self.source
                    .image
                    .get(file_offset as usize)
                    .copied()
                    .unwrap_or(0)
            }
        }
    }

    pub(super) fn write(&mut self, address: u64, value: u64, size: usize) -> Result<()> {
        let dest = self.writable(address, size as u64)?;
        dest.copy_from_slice(&value.to_le_bytes()[..size]);
        Ok(())
    }

    fn writable(&mut self, address: u64, size: u64) -> Result<&mut [u8]> {
        let (segment, base) = match address {
            BSS..BSS_END => (&mut self.bss, BSS),
            SCRATCH..SCRATCH_END => (&mut self.scratch, SCRATCH),
            _ => return Err(Field13Error::InvalidMemory),
        };
        let start = (address - base) as usize;
        let end = (address - base)
            .checked_add(size)
            .ok_or(Field13Error::InvalidMemory)?;
        segment
            .get_mut(start..end as usize)
            .ok_or(Field13Error::InvalidMemory)
    }

    pub(super) fn alloc(&mut self, size: u64) -> Result<u64> {
        let address = self.heap;
        self.heap = self
            .heap
            .checked_add(size.div_ceil(64) * 64)
            .filter(|end| *end < 0x870000)
            .ok_or(Field13Error::InvalidMemory)?;
        Ok(address)
    }

    pub(super) fn string(&mut self, address: u64, bytes: &[u8]) -> Result<()> {
        let data = self.alloc(bytes.len() as u64 + 1)?;
        self.write(address, 0x3c8288, 8)?;
        self.write(address + 8, bytes.len() as u64 + 1, 4)?;
        self.write(address + 12, bytes.len() as u64, 4)?;
        self.write(address + 16, data, 8)?;
        self.writable(data, bytes.len() as u64)?
            .copy_from_slice(bytes);
        self.write(data + bytes.len() as u64, 0, 1)
    }

    pub(super) fn copy(&mut self, dest: u64, source: u64, size: u64) -> Result<()> {
        if size > 0x4000 {
            return Err(Field13Error::InvalidMemory);
        }
        let mut buffer = vec![0; size as usize];
        self.copy_to(source, &mut buffer);
        self.writable(dest, size)?.copy_from_slice(&buffer);
        Ok(())
    }

    pub(super) fn fill(&mut self, dest: u64, byte: u8, size: u64) -> Result<()> {
        self.writable(dest, size)?.fill(byte);
        Ok(())
    }

    pub(super) fn decode_literal(&mut self, source: u64, dest: u64, mask: u64) -> Result<()> {
        for index in 0..300 {
            let key = self.byte(mask + index);
            if key == 0 {
                return Ok(());
            }
            self.write(dest + index, u64::from(self.byte(source + index) ^ key), 1)?;
        }
        Err(Field13Error::InvalidMemory)
    }
}
