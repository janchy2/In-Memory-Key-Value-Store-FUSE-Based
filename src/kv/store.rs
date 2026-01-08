use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use super::key_ref::KeyRef;
use crate::storage::string_table::StringTable;

const KEY_TABLE_SIZE: usize = 1024;
const VALUE_TABLE_SIZE: usize = 1024;
// TODO: Parametrize this

pub enum Entry {
    NotFound,
    NoValue,
    Value(String),
}

pub enum RemoveResult {
    NotFound,
    HasChildren,
    Removed,
}

pub struct KVStore {
    key_table: Rc<RefCell<StringTable>>,
    value_table: Rc<RefCell<StringTable>>,
    key_value_map: BTreeMap<KeyRef, Option<u32>>,
}

impl KVStore {
    pub fn new() -> Self {
        let key_table = Rc::new(RefCell::new(StringTable::new(KEY_TABLE_SIZE)));
        let value_table = Rc::new(RefCell::new(StringTable::new(VALUE_TABLE_SIZE)));
        let mut key_value_map = BTreeMap::new();

        let root_idx = key_table
            .borrow_mut()
            .append(&[])
            .expect("Append should not fail here");
        let root_key = KeyRef::new(key_table.clone(), root_idx, root_idx, root_idx);
        key_value_map.insert(root_key, None);
        KVStore {
            key_table,
            value_table,
            key_value_map,
        }
    }

    pub fn get_idx_for_key_str(&self, parent: u32, key: &str) -> Option<u32> {
        let temp_key = self.form_key_ref_from_str(parent, key);
        self.key_value_map
            .get_key_value(&temp_key)
            .map(|(k, _)| k.get_idx())
    }

    pub fn insert_key(&mut self, parent_parent_idx: u32, parent: u32, key: &str) -> Option<u32> {
        let temp_key = self.form_key_ref_from_str(parent, key);
        if self.key_value_map.contains_key(&temp_key) {
            return None;
        }
        let bytes = key.as_bytes();
        let idx = self
            .key_table
            .borrow_mut()
            .append(bytes)
            .expect("Append should not fail");
        let key_ref = KeyRef::new(self.key_table.clone(), idx, parent, parent_parent_idx);
        self.key_value_map.insert(key_ref, None);
        Some(idx)
    }

    pub fn insert_value(&mut self, parent: u32, idx: u32, value: &[u8]) -> bool {
        let key = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        let value_idx = self
            .value_table
            .borrow_mut()
            .append(value)
            .expect("Append should not fail");
        if let Some(v) = self.key_value_map.get_mut(&key) {
            *v = Some(value_idx);
            true
        } else {
            false
        }
    }

    pub fn get_children_keys_idx_and_names(&self, idx: u32) -> Vec<(u32, String)> {
        let start = KeyRef::get_boundary_for_children_search(idx);
        let end = KeyRef::get_boundary_for_children_search(idx + 1);

        let mut result = Vec::new();

        for (key_ref, _) in self.key_value_map.range(start..end) {
            let child_idx = key_ref.get_idx();
            if child_idx == idx {
                // This can happen for root
                continue;
            }
            result.push((child_idx, self.get_key_str(key_ref)));
        }

        result
    }

    pub fn get_value_for_key_idx(&self, parent: u32, idx: u32) -> Entry {
        // parent_parent_idx is irrelevant here, so it is set to 0
        let temp_key = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        self.get_value_str(&temp_key)
    }

    pub fn get_value_for_key_str(&self, parent: u32, key: &str) -> Entry {
        let temp_key = self.form_key_ref_from_str(parent, key);
        self.get_value_str(&temp_key)
    }

    pub fn get_parent_parent_idx(&self, parent: u32, idx: u32) -> Option<u32> {
        // parent_parent_idx is irrelevant here, so it is set to 0
        let temp_key = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        self.key_value_map
            .get_key_value(&temp_key)
            .map(|(k, _)| k.get_parent_parent_idx())
    }

    pub fn remove_key(&mut self, parent: u32, key: &str) -> RemoveResult {
        let idx = match self.get_idx_for_key_str(parent, key) {
            Some(idx) => idx,
            None => return RemoveResult::NotFound,
        };
        let children = self.get_children_keys_idx_and_names(idx);
        if children.len() > 0 {
            return RemoveResult::HasChildren;
        }
        // parent_parent_idx is irrelevant here, so it is set to 0
        let key_ref = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        match self.key_value_map.remove(&key_ref) {
            Some(_) => RemoveResult::Removed,
            None => panic!("Key not found, but it should exist"),
        }
    }

    fn get_key_str(&self, key_ref: &KeyRef) -> String {
        let key_table_ref = self.key_table.borrow();
        let bytes = key_table_ref
            .get(key_ref.get_idx())
            .expect("Get should not fail here");
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn get_value_str(&self, key_ref: &KeyRef) -> Entry {
        match self.key_value_map.get(key_ref).as_deref() {
            Some(Some(value_idx)) => {
                let value_table_ref = self.value_table.borrow();
                let bytes = value_table_ref
                    .get(*value_idx)
                    .expect("Get should not fail here");

                Entry::Value(String::from_utf8(bytes.to_vec()).unwrap())
            }
            Some(None) => Entry::NoValue,
            None => Entry::NotFound,
        }
    }

    fn form_key_ref_from_str(&self, parent: u32, key: &str) -> KeyRef {
        let bytes = key.as_bytes();
        let temp_table = Rc::new(RefCell::new(StringTable::new(bytes.len() + 1)));
        let temp_idx = temp_table
            .borrow_mut()
            .append(bytes)
            .expect("Append to temp table should not fail");
        // parent_parent_idx is irrelevant here, so it is set to 0
        KeyRef::new(temp_table, temp_idx, parent, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_store_contains_root_directory() {
        let store = KVStore::new();

        let root_children = store.get_children_keys_idx_and_names(1);
        assert!(root_children.is_empty());

        match store.get_value_for_key_idx(1, 1) {
            Entry::NoValue => {}
            _ => panic!("Root must be a directory"),
        }
    }

    #[test]
    fn insert_key_and_lookup_by_string() {
        let mut store = KVStore::new();

        let idx = store.insert_key(1, 1, "a").expect("Insert should succeed");

        assert_eq!(store.get_idx_for_key_str(1, "a"), Some(idx));
    }

    #[test]
    fn duplicate_key_under_same_parent_is_rejected() {
        let mut store = KVStore::new();

        assert!(store.insert_key(1, 1, "a").is_some());
        assert!(store.insert_key(1, 1, "a").is_none());
    }

    #[test]
    fn same_key_name_under_different_parents_is_allowed() {
        let mut store = KVStore::new();

        let a = store.insert_key(1, 1, "a").unwrap();
        let b = store.insert_key(1, 1, "b").unwrap();

        let a1 = store.insert_key(1, a, "x").unwrap();
        let b1 = store.insert_key(1, b, "x").unwrap();

        assert_ne!(a1, b1);
    }

    #[test]
    fn insert_value_and_retrieve_it() {
        let mut store = KVStore::new();

        let idx = store.insert_key(1, 1, "file").unwrap();
        let bytes = "hello".as_bytes();
        assert!(store.insert_value(1, idx, bytes));

        match store.get_value_for_key_idx(1, idx) {
            Entry::Value(v) => assert_eq!(v, "hello"),
            _ => panic!("Expected value"),
        }
    }

    #[test]
    fn get_children_of_key() {
        let mut store = KVStore::new();

        let idx = store.insert_key(1, 1, "key").unwrap();
        store.insert_key(0, idx, "a");
        store.insert_key(0, idx, "b");
        store.insert_key(0, idx, "c");

        let children = store.get_children_keys_idx_and_names(idx);
        let names: Vec<_> = children.into_iter().map(|(_, n)| n).collect();

        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn parent_parent_idx_is_correct() {
        let mut store = KVStore::new();

        let a = store.insert_key(1, 1, "a").unwrap();
        let b = store.insert_key(1, a, "b").unwrap();

        let ppi = store.get_parent_parent_idx(a, b).unwrap();
        assert_eq!(ppi, 1);
    }

    #[test]
    fn remove_leaf_key() {
        let mut store = KVStore::new();

        let a = store.insert_key(1, 1, "a").unwrap();
        let b = store.insert_key(1, a, "b").unwrap();

        assert!(matches!(store.remove_key(a, "b"), RemoveResult::Removed));

        assert!(matches!(store.get_value_for_key_idx(a, b), Entry::NotFound));
    }

    #[test]
    fn remove_key_with_children_fails() {
        let mut store = KVStore::new();

        let a = store.insert_key(1, 1, "a").unwrap();
        store.insert_key(1, a, "b");

        assert!(matches!(
            store.remove_key(1, "a"),
            RemoveResult::HasChildren
        ));
    }

    #[test]
    fn remove_nonexistent_key() {
        let mut store = KVStore::new();

        assert!(matches!(
            store.remove_key(1, "missing"),
            RemoveResult::NotFound
        ));
    }
}
