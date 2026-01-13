const IDX_BITS: u64 = 32;
const MASK: u64 = (1 << IDX_BITS) - 1;

#[inline]
pub fn ino_to_idx(ino: u64) -> usize {
    (ino & MASK) as usize
}

#[inline]
pub fn ino_to_parent_idx(ino: u64) -> usize {
    ((ino >> IDX_BITS) & MASK) as usize
}

#[inline]
pub fn form_ino(parent_idx: usize, idx: usize) -> u64 {
    ((parent_idx as u64) << IDX_BITS) | idx as u64
}
