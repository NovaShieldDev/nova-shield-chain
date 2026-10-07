//! WASM lineer bellek yönetimi — Polkadot HeapAllocStrategy pattern

use crate::errors::{VmError, VmResult};

/// WASM lineer bellek — 64KiB page'ler halinde büyür
#[derive(Debug, Clone)]
pub struct LinearMemory {
    data: Vec<u8>,
    max_bytes: usize,
}

impl LinearMemory {
    /// Yeni bellek — başlangıç page sayısı ile
    pub fn new(initial_pages: usize, max_pages: usize) -> Self {
        let initial_bytes = initial_pages * 65_536; // 1 page = 64KiB
        let max_bytes = max_pages * 65_536;
        Self {
            data: vec![0u8; initial_bytes],
            max_bytes,
        }
    }

    /// Mevcut boyut (byte)
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Bellek büyüt (delta page)
    pub fn grow(&mut self, delta_pages: usize) -> VmResult<usize> {
        let current_pages = self.data.len() / 65_536;
        let new_bytes = self.data.len() + delta_pages * 65_536;
        if new_bytes > self.max_bytes {
            return Err(VmError::MemoryLimitExceeded { max_bytes: self.max_bytes });
        }
        self.data.resize(new_bytes, 0);
        Ok(current_pages)
    }

    /// Bellek okuma
    pub fn read(&self, offset: usize, len: usize) -> VmResult<&[u8]> {
        let end = offset.checked_add(len).ok_or(VmError::InvalidInput)?;
        if end > self.data.len() {
            return Err(VmError::MemoryLimitExceeded { max_bytes: self.data.len() });
        }
        Ok(&self.data[offset..end])
    }

    /// Bellek yazma
    pub fn write(&mut self, offset: usize, data: &[u8]) -> VmResult<()> {
        let end = offset.checked_add(data.len()).ok_or(VmError::InvalidInput)?;
        if end > self.data.len() {
            return Err(VmError::MemoryLimitExceeded { max_bytes: self.data.len() });
        }
        self.data[offset..end].copy_from_slice(data);
        Ok(())
    }

    /// 4-byte LE u32 okuma (WASM ABI)
    pub fn read_u32(&self, offset: usize) -> VmResult<u32> {
        let bytes = self.read(offset, 4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    /// 4-byte LE u32 yazma
    pub fn write_u32(&mut self, offset: usize, val: u32) -> VmResult<()> {
        self.write(offset, &val.to_le_bytes())
    }

    /// 8-byte LE u64 okuma
    pub fn read_u64(&self, offset: usize) -> VmResult<u64> {
        let bytes = self.read(offset, 8)?;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    /// Slice olarak tüm belleğe erişim
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Bellek sıfırlama (reset)
    pub fn clear(&mut self) {
        self.data.fill(0);
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_initial_size() {
        let mem = LinearMemory::new(1, 16);
        assert_eq!(mem.size(), 65_536);
    }

    #[test]
    fn test_memory_read_write() {
        let mut mem = LinearMemory::new(1, 16);
        mem.write(0, &[1, 2, 3, 4]).unwrap();
        let data = mem.read(0, 4).unwrap();
        assert_eq!(data, &[1, 2, 3, 4]);
    }

    #[test]
    fn test_memory_grow() {
        let mut mem = LinearMemory::new(1, 16);
        let prev = mem.grow(2).unwrap();
        assert_eq!(prev, 1); // önceki page sayısı
        assert_eq!(mem.size(), 3 * 65_536);
    }

    #[test]
    fn test_memory_grow_exceeds_max() {
        let mut mem = LinearMemory::new(1, 2);
        assert!(matches!(mem.grow(2), Err(VmError::MemoryLimitExceeded { .. })));
    }

    #[test]
    fn test_memory_out_of_bounds_read() {
        let mem = LinearMemory::new(1, 16);
        assert!(matches!(mem.read(65_534, 4), Err(VmError::MemoryLimitExceeded { .. })));
    }

    #[test]
    fn test_memory_u32_roundtrip() {
        let mut mem = LinearMemory::new(1, 16);
        mem.write_u32(100, 0xDEAD_BEEF).unwrap();
        assert_eq!(mem.read_u32(100).unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn test_memory_u64_read() {
        let mut mem = LinearMemory::new(1, 16);
        mem.write(0, &42u64.to_le_bytes()).unwrap();
        assert_eq!(mem.read_u64(0).unwrap(), 42);
    }

    #[test]
    fn test_memory_clear() {
        let mut mem = LinearMemory::new(1, 16);
        mem.write(0, &[1, 2, 3]).unwrap();
        mem.clear();
        assert_eq!(mem.read(0, 3).unwrap(), &[0, 0, 0]);
    }
}
