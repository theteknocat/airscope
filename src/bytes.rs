//! Fixed-size numbers out of byte buffers (netlink, HCI and btsnoop messages). Each reads at byte offset `o`
//! and panics past the end, exactly like indexing, so callers check lengths first.

/// Native-endian `u16` (netlink).
pub fn ne16(b: &[u8], o: usize) -> u16 {
    u16::from_ne_bytes([b[o], b[o + 1]])
}

/// Native-endian `u32` (netlink).
pub fn ne32(b: &[u8], o: usize) -> u32 {
    u32::from_ne_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Little-endian `u16` (HCI and btsnoop).
pub fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_at_offset() {
        let b = [0xff, 0x34, 0x12, 0x78, 0x56];
        assert_eq!(le16(&b, 1), 0x1234);
        assert_eq!(ne16(&b, 3), u16::from_ne_bytes([0x78, 0x56]));
        assert_eq!(ne32(&b, 1), u32::from_ne_bytes([0x34, 0x12, 0x78, 0x56]));
    }

    #[test]
    #[should_panic]
    fn past_the_end_panics() {
        le16(&[1, 2, 3], 2);
    }
}
