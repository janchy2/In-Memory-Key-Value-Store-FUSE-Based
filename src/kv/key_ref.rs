use std::{
    cell::RefCell,
    cmp::Ordering,
    hash::{Hash, Hasher},
    rc::Rc,
};

use crate::storage::string_table::StringTable;

#[derive(Debug)]
pub struct KeyRef {
    table: Rc<RefCell<StringTable>>,
    idx: usize,
    parent: Option<usize>,
}

impl KeyRef {
    pub fn new(table: Rc<RefCell<StringTable>>, idx: usize, parent: Option<usize>) -> Self {
        Self { table, idx, parent }
    }

    pub fn get_idx(&self) -> usize {
        self.idx
    }

    pub fn get_boundary_for_children_search(parent: usize) -> Self {
        let table = Rc::new(RefCell::new(StringTable::new(1)));
        table
            .borrow_mut()
            .append(&[])
            .expect("Append should not fail");
        KeyRef {
            table: table,
            idx: 0,
            parent: Some(parent),
        }
    }
}

impl PartialEq for KeyRef {
    fn eq(&self, other: &Self) -> bool {
        self.table.borrow().get(self.idx) == other.table.borrow().get(other.idx)
            && self.parent == other.parent
    }
}

impl Eq for KeyRef {}

impl Hash for KeyRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.table
            .borrow()
            .get(self.idx)
            .expect("The string index should be valid")
            .hash(state);
        self.parent.hash(state);
    }
}

impl Ord for KeyRef {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.parent.cmp(&other.parent) {
            Ordering::Equal => self
                .table
                .borrow()
                .get(self.idx)
                .expect("The string index should be valid")
                .cmp(
                    other
                        .table
                        .borrow()
                        .get(other.idx)
                        .expect("The string index should be valid"),
                ),
            ord => ord,
        }
    }
}

impl PartialOrd for KeyRef {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        cmp::Ordering,
        hash::{DefaultHasher, Hash, Hasher},
        rc::Rc,
    };

    use super::*;

    #[test]
    fn string_refs_with_different_indices_equal_same_parent() {
        let table = Rc::new(RefCell::new(StringTable::new(5)));

        let a1 = table.borrow_mut().append(b"a").unwrap();
        let a2 = table.borrow_mut().append(b"a").unwrap();

        let sr1 = KeyRef::new(Rc::clone(&table), a1, None);
        let sr2 = KeyRef::new(Rc::clone(&table), a2, None);

        assert_eq!(sr1, sr2);
    }

    #[test]
    fn string_refs_from_different_tables_equal_same_parent() {
        let t1 = Rc::new(RefCell::new(StringTable::new(10)));
        let t2 = Rc::new(RefCell::new(StringTable::new(10)));

        let a = t1.borrow_mut().append(b"hello").unwrap();
        let b = t2.borrow_mut().append(b"hello").unwrap();

        let sr1 = KeyRef::new(Rc::clone(&t1), a, None);
        let sr2 = KeyRef::new(Rc::clone(&t2), b, None);

        assert_eq!(sr1, sr2);
    }

    #[test]
    fn same_string_different_parent_not_equal() {
        let table = Rc::new(RefCell::new(StringTable::new(10)));

        let a1 = table.borrow_mut().append(b"a").unwrap();
        let a2 = table.borrow_mut().append(b"a").unwrap();

        let sr1 = KeyRef::new(Rc::clone(&table), a1, None);
        let sr2 = KeyRef::new(Rc::clone(&table), a2, Some(1));

        assert_ne!(sr1, sr2);
    }

    #[test]
    fn string_refs_different_strings_not_equal() {
        let table = Rc::new(RefCell::new(StringTable::new(10)));

        let a = table.borrow_mut().append(b"a").unwrap();
        let b = table.borrow_mut().append(b"b").unwrap();

        let sr_a = KeyRef::new(Rc::clone(&table), a, None);
        let sr_b = KeyRef::new(Rc::clone(&table), b, None);

        assert_ne!(sr_a, sr_b);
    }

    #[test]
    fn equal_string_and_parent_have_same_hash() {
        let table = Rc::new(RefCell::new(StringTable::new(10)));

        let a1 = table.borrow_mut().append(b"abc").unwrap();
        let a2 = table.borrow_mut().append(b"abc").unwrap();

        let sr1 = KeyRef::new(Rc::clone(&table), a1, Some(1));
        let sr2 = KeyRef::new(Rc::clone(&table), a2, Some(1));

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();

        sr1.hash(&mut h1);
        sr2.hash(&mut h2);

        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn different_parent_changes_hash() {
        let table = Rc::new(RefCell::new(StringTable::new(10)));

        let a = table.borrow_mut().append(b"abc").unwrap();

        let sr1 = KeyRef::new(Rc::clone(&table), a, None);
        let sr2 = KeyRef::new(Rc::clone(&table), a, Some(1));

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();

        sr1.hash(&mut h1);
        sr2.hash(&mut h2);

        assert_ne!(h1.finish(), h2.finish());
    }

    #[test]
    fn none_parent_is_less_than_some() {
        let table = Rc::new(RefCell::new(StringTable::new(5)));

        let a = table.borrow_mut().append(b"a").unwrap();

        let r1 = KeyRef::new(Rc::clone(&table), a, None);
        let r2 = KeyRef::new(Rc::clone(&table), a, Some(1));

        assert!(r1 < r2);
    }

    #[test]
    fn same_parent_compares_by_string() {
        let table = Rc::new(RefCell::new(StringTable::new(5)));

        let a = table.borrow_mut().append(b"a").unwrap();
        let b = table.borrow_mut().append(b"b").unwrap();

        let r1 = KeyRef::new(Rc::clone(&table), a, Some(1));
        let r2 = KeyRef::new(Rc::clone(&table), b, Some(1));

        assert!(r1 < r2);
    }

    #[test]
    fn equal_strings_and_parent_compare_equal() {
        let table = Rc::new(RefCell::new(StringTable::new(5)));

        let a1 = table.borrow_mut().append(b"a").unwrap();
        let a2 = table.borrow_mut().append(b"a").unwrap();

        let r1 = KeyRef::new(Rc::clone(&table), a1, Some(1));
        let r2 = KeyRef::new(Rc::clone(&table), a2, Some(1));

        assert_eq!(r1.cmp(&r2), Ordering::Equal);
    }
}
