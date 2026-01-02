#[derive(Debug, PartialEq)]
pub enum Error {
    OutOfMemory,
    IndexOutOfBounds
}

#[derive(Debug)]
pub struct StringTable {
    data: Box<[u8]>,
    next: usize,
    capacity: usize,
}

impl StringTable {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: vec![0; capacity].into_boxed_slice(), 
            next: 0,
            capacity: capacity,
        }
    }

    pub fn append(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        let len = bytes.len();
        let needed = len + 1;

        if self.next + needed > self.capacity {
            // TODO: call eviction method here instead of returning an error
            return Err(Error::OutOfMemory);
        }
        
        let offset = self.next;
        self.data[offset..offset + len].copy_from_slice(bytes);
        self.data[offset + len] = 0;

        self.next = offset + needed;

        Ok(offset)
    }

    pub fn get(&self, idx: usize) -> Result<&[u8], Error> {
        if idx >= self.next {
            return Err(Error::IndexOutOfBounds);
        }

        let data = &self.data[..];

        let mut end = idx;
        while end < self.next && data[end] != 0 {
            end += 1;
        }

        Ok(&data[idx..end])
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_stores_string_and_null_terminates() {
        let mut table = StringTable::new(5);
        let idx = table.append(b"abc").unwrap();

        assert_eq!(&table.data[idx..idx + 3], b"abc");
        assert_eq!(table.data[idx + 3], 0);
    }

    #[test]
    fn append_multiple_strings_are_contiguous() {
        let mut table = StringTable::new(5);
        let a = table.append(b"a").unwrap();
        let b = table.append(b"bb").unwrap();

        assert_eq!(a, 0);
        assert_eq!(b, 2);
    }

    #[test]
    fn append_capacity_exceeded() {
        let mut table = StringTable::new(2);
        let a = table.append(b"a").unwrap();
        let b = table.append(b"bb");

        assert_eq!(a, 0);
        assert_eq!(b, Err(Error::OutOfMemory));
    }

    #[test]
    fn get_string_correct_index() {
        let mut table = StringTable::new(10);
        let a = table.append(b"a").unwrap();
        let b = table.append(b"bbbb").unwrap();
        let c = table.append(b"cc").unwrap();
        let a_str = table.get(a).unwrap();
        let b_str = table.get(b).unwrap();
        let c_str = table.get(c).unwrap();

        assert_eq!(a_str, b"a");
        assert_eq!(b_str, b"bbbb");
        assert_eq!(c_str, b"cc");
    }

    #[test]
    fn get_string_out_of_bounds_index() {
        let table = StringTable::new(5);
        let result = table.get(5);

        assert_eq!(result, Err(Error::IndexOutOfBounds));
    }

    #[test]
    fn duplicate_strings_are_independent() {
        let mut table = StringTable::new(10);

        let a1 = table.append(b"a").unwrap();
        let a2 = table.append(b"a").unwrap();

        assert_ne!(a1, a2);
        assert_eq!(table.get(a1).unwrap(), b"a");
        assert_eq!(table.get(a2).unwrap(), b"a");
    }
}
