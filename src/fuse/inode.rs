const IDX_BITS: u64 = 32;
const MASK: u64 = (1 << IDX_BITS) - 1;

#[inline]
pub fn ino_to_idx(ino: u64) -> u32 {
    (ino & MASK) as u32
}

#[inline]
pub fn ino_to_parent_idx(ino: u64) -> u32 {
    ((ino >> IDX_BITS) & MASK) as u32
}

#[inline]
pub fn form_ino(parent_idx: u32, idx: u32) -> u64 {
    ((parent_idx as u64) << IDX_BITS) | idx as u64
}
