use std::hash::{Hash, Hasher};

use super::string_table::StringTable;

#[derive(Debug)]
pub struct StrRef<'a> {
    table: &'a StringTable,
    idx: usize,
}

impl<'a> StrRef<'a> {
    pub fn new(table: &'a StringTable, idx: usize) -> Self {
        Self { table, idx }
    }
}

impl<'a> PartialEq for StrRef<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.table.get(self.idx) == other.table.get(other.idx)
    }
}

impl<'a> Eq for StrRef<'a> {}

impl<'a> Hash for StrRef<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) { 
        self.table.get(self.idx).expect("The string index should be valid").hash(state);
    }
}


#[cfg(test)]
mod tests {
    use std::{collections::HashMap, hash::DefaultHasher};

    use super::*;

    #[test]
    fn string_refs_with_different_indices_equal() {
        let mut table = StringTable::new(5);

        let a1 = table.append(b"a").unwrap();
        let a2 = table.append(b"a").unwrap();

        let sr1 = StrRef::new(&table, a1);
        let sr2 = StrRef::new(&table, a2);

        assert_eq!(sr1, sr2);
    }

    #[test]
    fn string_refs_from_different_tables_equal() {
        let mut t1 = StringTable::new(10);
        let mut t2 = StringTable::new(10);

        let a = t1.append(b"hello").unwrap();
        let b = t2.append(b"hello").unwrap();

        let sr1 = StrRef::new(&t1, a);
        let sr2 = StrRef::new(&t2, b);

        assert_eq!(sr1, sr2);
    }

    #[test]
    fn string_refs_different_strings_not_equal() {
        let mut table = StringTable::new(10);

        let a = table.append(b"a").unwrap();
        let b = table.append(b"b").unwrap();

        let sr_a = StrRef::new(&table, a);
        let sr_b = StrRef::new(&table, b);

        assert_ne!(sr_a, sr_b);
    }

    #[test]
    fn string_refs_same_indices_not_equal() {
        let mut t1 = StringTable::new(10);
        let mut t2 = StringTable::new(10);

        let a = t1.append(b"hello").unwrap();
        let b = t2.append(b"hi").unwrap();

        let sr1 = StrRef::new(&t1, a);
        let sr2 = StrRef::new(&t2, b);

        assert_ne!(sr1, sr2);
    }

    #[test]
    fn equal_string_refs_have_same_hash() {
        let mut table = StringTable::new(10);

        let a1 = table.append(b"abc").unwrap();
        let a2 = table.append(b"abc").unwrap();

        let sr1 = StrRef::new(&table, a1);
        let sr2 = StrRef::new(&table, a2);

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();

        sr1.hash(&mut h1);
        sr2.hash(&mut h2);

        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn strref_works_as_hashmap_key() {
        let mut table = StringTable::new(20);

        let a1 = table.append(b"key").unwrap();
        let a2 = table.append(b"key").unwrap();

        let sr1 = StrRef::new(&table, a1);
        let sr2 = StrRef::new(&table, a2);

        let mut map = HashMap::new();
        map.insert(sr1, 22);

        assert_eq!(map.get(&sr2), Some(&22));
    }

    #[test]
    #[should_panic]
    fn hashing_invalid_index_panics() {
        let table = StringTable::new(5);
        let sr = StrRef::new(&table, 999);

        let mut h = std::collections::hash_map::DefaultHasher::new();
        sr.hash(&mut h);
    }

}
