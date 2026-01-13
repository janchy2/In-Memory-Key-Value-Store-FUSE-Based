pub enum AppendResult {
    Ok(usize),
    CapacityExceeded,
}

#[derive(Debug)]
pub struct StringTable {
    data: Vec<u8>,
    max_capacity: usize,
}

impl StringTable {
    pub fn new(capacity: usize, max_capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            max_capacity,
        }
    }

    pub fn append(&mut self, bytes: &[u8]) -> AppendResult {
        let idx = self.data.len();
        let needed = bytes.len() + 1;
        let capacity = self.data.capacity();
        if idx + needed > capacity {
            if capacity * 2 > self.max_capacity {
                return AppendResult::CapacityExceeded;
            } else {
                if let Err(_) = self.data.try_reserve_exact(capacity) {
                    return AppendResult::CapacityExceeded;
                }
            }
        }

        let iter = bytes.iter().copied().chain(std::iter::once(0));
        for byte in iter {
            self.data.push(byte);
        }

        AppendResult::Ok(idx)
    }

    pub fn get(&self, idx: usize) -> Option<&[u8]> {
        let mut end = idx;
        while let Some(byte) = self.data.get(end) {
            if *byte == 0 {
                break;
            }
            end += 1;
        }

        self.data.get(idx..end)
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unwrap_idx(res: AppendResult) -> usize {
        match res {
            AppendResult::Ok(idx) => idx,
            AppendResult::CapacityExceeded => panic!("Capacity exceeded"),
        }
    }

    #[test]
    fn append_stores_string_and_null_terminates() {
        let mut table = StringTable::new(5, 5);
        let idx = unwrap_idx(table.append(b"abc")) as usize;

        assert_eq!(&table.data[idx..idx + 3], b"abc");
        assert_eq!(table.data[idx + 3], 0);
    }

    #[test]
    fn append_multiple_strings_are_contiguous() {
        let mut table = StringTable::new(10, 10);
        let a = unwrap_idx(table.append(b"a"));
        let b = unwrap_idx(table.append(b"bb"));

        assert_eq!(a, 0);
        assert_eq!(b, 2);
    }

    #[test]
    fn get_string_correct_index() {
        let mut table = StringTable::new(15, 15);
        let a = unwrap_idx(table.append(b"a"));
        let b = unwrap_idx(table.append(b"bbbb"));
        let c = unwrap_idx(table.append(b"cc"));

        let a_str = table.get(a).unwrap();
        let b_str = table.get(b).unwrap();
        let c_str = table.get(c).unwrap();

        assert_eq!(a_str, b"a");
        assert_eq!(b_str, b"bbbb");
        assert_eq!(c_str, b"cc");
    }

    #[test]
    fn get_string_out_of_bounds_index() {
        let table = StringTable::new(5, 5);
        let result = table.get(1);

        assert_eq!(result, None);
    }

    #[test]
    fn duplicate_strings_are_independent() {
        let mut table = StringTable::new(10, 10);

        let a1 = unwrap_idx(table.append(b"a"));
        let a2 = unwrap_idx(table.append(b"a"));

        assert_ne!(a1, a2);
        assert_eq!(table.get(a1).unwrap(), b"a");
        assert_eq!(table.get(a2).unwrap(), b"a");
    }

    #[test]
    fn more_capacity_reserved_when_not_enough_space() {
        let mut table = StringTable::new(3, 10);

        let r1 = table.append(b"a");
        let r2 = table.append(b"bb");

        assert!(matches!(r1, AppendResult::Ok(_)));
        assert!(matches!(r2, AppendResult::Ok(_)));
    }

    #[test]
    fn capacity_exceeded_when_expanding_exceeds_max() {
        let mut table = StringTable::new(3, 5);

        let r1 = table.append(b"a");
        let r2 = table.append(b"bb");

        assert!(matches!(r1, AppendResult::Ok(_)));
        assert!(matches!(r2, AppendResult::CapacityExceeded));
    }
}
