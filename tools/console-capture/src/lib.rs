#![no_std]

pub const PAGE: usize = 4096;

/// Only ordinary loaded-image virtual addresses; never physical aliases or MMIO.
pub fn image_range(base: u32, size: u32) -> bool {
    base >= 0x8000_0000
        && base % PAGE as u32 == 0
        && size > 0
        && size <= 0x0400_0000
        && base.checked_add(size).is_some_and(|end| end <= 0xa000_0000)
}

pub fn readable(protection: u32) -> bool {
    protection & 0x701 == 0 && protection & 0xee != 0
}

/// Bound a single page-sized read, including loader records in ordinary low virtual memory.
pub fn read_range(address: u32, length: usize) -> bool {
    address >= 0x10000
        && length > 0
        && length <= PAGE
        && address
            .checked_add(length as u32)
            .is_some_and(|end| end <= 0xa0000000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_module_and_bounds() {
        assert!(image_range(0x80040000, 0x200000));
        assert!(image_range(0x91850000, 0x3c800));
        for (base, size) in [
            (0, 4096),
            (0xa0000000, 4096),
            (0x80000001, 4096),
            (0x80040000, 0),
            (0x80040000, 0x4000001),
            (0x9ffff000, 8192),
            (0xfffff000, 8192),
        ] {
            assert!(!image_range(base, size));
        }
    }

    #[test]
    fn skip_inaccessible_guard_and_device_pages() {
        for p in [2, 4, 8, 0x20, 0x40, 0x80, 0x20000002] {
            assert!(readable(p));
        }
        for p in [0, 1, 0x10, 0x102, 0x202, 0x404, 0x101] {
            assert!(!readable(p));
        }
    }

    #[test]
    fn real_loader_handles_are_not_image_bases() {
        for handle in [0x801a5d60, 0x3a0bff48, 0x3a0d3ee8, 0x3a0d4f28] {
            assert!(read_range(handle, 0x60));
        }
        assert!(!image_range(0x3a0bff48, 0x60));
        for (addr, size) in [
            (0, 0x60),
            (0xfff0, 0x60),
            (0x80040000, 0),
            (0x80040000, PAGE + 1),
            (0x9ffffff0, 0x60),
            (0xa0000000, 0x60),
            (0xfffffff0, 0x60),
        ] {
            assert!(!read_range(addr, size));
        }
    }
}
