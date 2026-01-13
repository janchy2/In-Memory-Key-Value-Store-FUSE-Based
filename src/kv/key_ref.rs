use std::{
    cell::RefCell,
    cmp::Ordering,
    hash::{Hash, Hasher},
    rc::Rc,
    time::SystemTime,
};

use super::string_table::StringTable;

#[derive(Debug)]
pub struct KeyRef {
    table: Rc<RefCell<StringTable>>,
    idx: usize,
    parent: usize,
    parent_parent_idx: usize, // Only used for creating parent KeyRef, it is not part of identity
    expires_at: Option<SystemTime>,
}

impl KeyRef {
    pub fn new(
        table: Rc<RefCell<StringTable>>,
        idx: usize,
        parent: usize,
        parent_parent_idx: usize,
    ) -> Self {
        let expires_at = None;
        Self {
            table,
            idx,
            parent,
            parent_parent_idx,
            expires_at,
        }
    }

    pub fn get_idx(&self) -> usize {
        self.idx
    }

    pub fn get_parent_idx(&self) -> usize {
        self.parent
    }

    pub fn get_parent_parent_idx(&self) -> usize {
        self.parent_parent_idx
    }

    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            None => false,
            Some(t) => SystemTime::now() >= t,
        }
    }

    pub fn set_expiration(&mut self, expires_at: Option<SystemTime>) {
        self.expires_at = expires_at;
    }

    pub fn get_boundary_for_children_search(parent: usize) -> Self {
        let table = Rc::new(RefCell::new(StringTable::new(1, 1)));
        table.borrow_mut().append(&[]);
        // parent_parent_idx is irrelevant here
        KeyRef::new(table, 0, parent, 0)
    }
}

impl Clone for KeyRef {
    fn clone(&self) -> Self {
        let table = self.table.clone();
        let idx = self.idx;
        let parent = self.parent;
        let parent_parent_idx = self.parent_parent_idx;
        let expires_at = None;
        Self {
            table,
            idx,
            parent,
            parent_parent_idx,
            expires_at,
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

    use crate::kv::string_table::AppendResult;

    use super::*;

    fn unwrap_idx(res: AppendResult) -> usize {
        match res {
            AppendResult::Ok(idx) => idx,
            AppendResult::CapacityExceeded => panic!("Capacity exceeded"),
        }
    }

    #[test]
    fn string_refs_with_different_indices_equal_same_parent() {
        let table = Rc::new(RefCell::new(StringTable::new(5, 5)));

        let a1 = unwrap_idx(table.borrow_mut().append(b"a"));
        let a2 = unwrap_idx(table.borrow_mut().append(b"a"));

        let sr1 = KeyRef::new(Rc::clone(&table), a1, 0, 0);
        let sr2 = KeyRef::new(Rc::clone(&table), a2, 0, 0);

        assert_eq!(sr1, sr2);
    }

    #[test]
    fn string_refs_from_different_tables_equal_same_parent() {
        let t1 = Rc::new(RefCell::new(StringTable::new(10, 10)));
        let t2 = Rc::new(RefCell::new(StringTable::new(10, 10)));

        let a = unwrap_idx(t1.borrow_mut().append(b"hello"));
        let b = unwrap_idx(t2.borrow_mut().append(b"hello"));

        let sr1 = KeyRef::new(Rc::clone(&t1), a, 0, 0);
        let sr2 = KeyRef::new(Rc::clone(&t2), b, 0, 0);

        assert_eq!(sr1, sr2);
    }

    #[test]
    fn same_string_different_parent_not_equal() {
        let table = Rc::new(RefCell::new(StringTable::new(10, 10)));

        let a1 = unwrap_idx(table.borrow_mut().append(b"a"));
        let a2 = unwrap_idx(table.borrow_mut().append(b"a"));

        let sr1 = KeyRef::new(Rc::clone(&table), a1, 0, 0);
        let sr2 = KeyRef::new(Rc::clone(&table), a2, 1, 0);

        assert_ne!(sr1, sr2);
    }

    #[test]
    fn string_refs_different_strings_not_equal() {
        let table = Rc::new(RefCell::new(StringTable::new(10, 10)));

        let a = unwrap_idx(table.borrow_mut().append(b"a"));
        let b = unwrap_idx(table.borrow_mut().append(b"b"));

        let sr_a = KeyRef::new(Rc::clone(&table), a, 0, 0);
        let sr_b = KeyRef::new(Rc::clone(&table), b, 0, 0);

        assert_ne!(sr_a, sr_b);
    }

    #[test]
    fn equal_string_and_parent_have_same_hash() {
        let table = Rc::new(RefCell::new(StringTable::new(10, 10)));

        let a1 = unwrap_idx(table.borrow_mut().append(b"abc"));
        let a2 = unwrap_idx(table.borrow_mut().append(b"abc"));

        let sr1 = KeyRef::new(Rc::clone(&table), a1, 1, 0);
        let sr2 = KeyRef::new(Rc::clone(&table), a2, 1, 0);

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();

        sr1.hash(&mut h1);
        sr2.hash(&mut h2);

        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn different_parent_changes_hash() {
        let table = Rc::new(RefCell::new(StringTable::new(10, 10)));

        let a = unwrap_idx(table.borrow_mut().append(b"abc"));

        let sr1 = KeyRef::new(Rc::clone(&table), a, 0, 0);
        let sr2 = KeyRef::new(Rc::clone(&table), a, 1, 0);

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();

        sr1.hash(&mut h1);
        sr2.hash(&mut h2);

        assert_ne!(h1.finish(), h2.finish());
    }

    #[test]
    fn same_parent_compares_by_string() {
        let table = Rc::new(RefCell::new(StringTable::new(5, 5)));

        let a = unwrap_idx(table.borrow_mut().append(b"a"));
        let b = unwrap_idx(table.borrow_mut().append(b"b"));

        let r1 = KeyRef::new(Rc::clone(&table), a, 1, 0);
        let r2 = KeyRef::new(Rc::clone(&table), b, 1, 0);

        assert!(r1 < r2);
    }

    #[test]
    fn same_string_compares_by_parent() {
        let table = Rc::new(RefCell::new(StringTable::new(5, 5)));

        let a = unwrap_idx(table.borrow_mut().append(b"a"));
        let b = unwrap_idx(table.borrow_mut().append(b"a"));

        let r1 = KeyRef::new(Rc::clone(&table), a, 2, 0);
        let r2 = KeyRef::new(Rc::clone(&table), b, 1, 0);

        assert!(r1 > r2);
    }

    #[test]
    fn equal_strings_and_parent_compare_equal() {
        let table = Rc::new(RefCell::new(StringTable::new(5, 5)));

        let a1 = unwrap_idx(table.borrow_mut().append(b"a"));
        let a2 = unwrap_idx(table.borrow_mut().append(b"a"));

        let r1 = KeyRef::new(Rc::clone(&table), a1, 1, 0);
        let r2 = KeyRef::new(Rc::clone(&table), a2, 1, 0);

        assert_eq!(r1.cmp(&r2), Ordering::Equal);
    }

    #[test]
    fn different_parent_parent_idx_does_not_affect_equality() {
        let table = Rc::new(RefCell::new(StringTable::new(10, 10)));

        let a1 = unwrap_idx(table.borrow_mut().append(b"a"));
        let a2 = unwrap_idx(table.borrow_mut().append(b"a"));

        let r1 = KeyRef::new(Rc::clone(&table), a1, 1, 42);
        let r2 = KeyRef::new(Rc::clone(&table), a2, 1, 999);

        assert_eq!(r1, r2);
    }

    #[test]
    fn parent_parent_idx_does_not_affect_ordering() {
        let table = Rc::new(RefCell::new(StringTable::new(10, 10)));

        let a = unwrap_idx(table.borrow_mut().append(b"a"));
        let b = unwrap_idx(table.borrow_mut().append(b"b"));

        let r1 = KeyRef::new(Rc::clone(&table), a, 1, 10);
        let r2 = KeyRef::new(Rc::clone(&table), b, 1, 999);

        assert!(r1 < r2);
    }

    #[test]
    fn parent_parent_idx_does_not_affect_hash() {
        let table = Rc::new(RefCell::new(StringTable::new(10, 10)));

        let a1 = unwrap_idx(table.borrow_mut().append(b"x"));
        let a2 = unwrap_idx(table.borrow_mut().append(b"x"));

        let r1 = KeyRef::new(Rc::clone(&table), a1, 5, 1);
        let r2 = KeyRef::new(Rc::clone(&table), a2, 5, 999);

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();

        r1.hash(&mut h1);
        r2.hash(&mut h2);

        assert_eq!(h1.finish(), h2.finish());
    }
}
