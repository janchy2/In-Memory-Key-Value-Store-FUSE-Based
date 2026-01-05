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

    pub fn insert_value(&mut self, parent: u32, idx: u32, value: &str) -> bool {
        let key = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        let value_idx = self
            .value_table
            .borrow_mut()
            .append(value.as_bytes())
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

    pub fn get_value_for_key(&self, parent: u32, idx: u32) -> Entry {
        let temp_key = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        self.get_value_str(&temp_key)
    }

    pub fn get_parent_parent_idx(&self, parent: u32, idx: u32) -> Option<u32> {
        let temp_key = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        self.key_value_map
            .get_key_value(&temp_key)
            .map(|(k, _)| k.get_parent_parent_idx())
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
        KeyRef::new(temp_table, temp_idx, parent, 0)
    }

    /*
    pub fn remove_key(&mut self, key: &str) {
        let parent_str = key.rsplit_once('/').map(|(before, _)| before).unwrap_or("");

        let parent_idx;
        if parent_str == "" {
            parent_idx = None
        } else {
            let (key_idx, _) = match self.lookup_key_value_for_key(parent_str) {
                Some(key_value) => key_value,
                None => return,
            };
            parent_idx = Some(key_idx);
        }

        let (key_idx, _) = match self.lookup_key_value_for_key(key) {
            Some(key_value) => key_value,
            None => return,
        };

        let root_key_ref = KeyRef::new(self.key_table.clone(), key_idx, parent_idx);

        let mut queue = VecDeque::new();
        let mut to_remove = Vec::new();

        queue.push_back(root_key_ref);

        while let Some(current) = queue.pop_front() {
            let current_idx = current.get_idx();
            let start = KeyRef::get_boundary_for_children_search(current_idx);
            let end = KeyRef::get_boundary_for_children_search(current_idx + 1);

            for (key_ref, _) in self.key_value_map.range(start..end) {
                queue.push_back(KeyRef::new(
                    self.key_table.clone(),
                    key_ref.get_idx(),
                    Some(current_idx),
                ));
            }
            to_remove.push(current);
        }
        for key_ref in to_remove {
            self.key_value_map.remove(&key_ref);
        }
    }
    */
}

/*
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_key_simple() {
        let mut store = KVStore::new();

        assert!(store.insert_key("a"));
        assert!(store.insert_key("a/b"));
        assert!(store.insert_key("a/b/c"));

        assert!(!store.insert_key("a/b"));
    }

    #[test]
    fn insert_key_value_and_get_value() {
        let mut store = KVStore::new();

        assert!(store.insert_key_value("a/b/c", "value1"));
        assert_eq!(store.get_value("a/b/c"), Some("value1".to_string()));

        assert_eq!(store.get_value("a/b"), None);
    }

    #[test]
    fn cannot_insert_below_value_key() {
        let mut store = KVStore::new();

        assert!(store.insert_key_value("a", "root"));

        assert!(!store.insert_key("a/b"));
        assert!(!store.insert_key_value("a/b", "child"));
    }

    #[test]
    fn get_children_keys_simple() {
        let mut store = KVStore::new();

        store.insert_key("a/b");
        store.insert_key("a/c");
        store.insert_key("a/d");

        let children = store.get_children_keys("a").unwrap();

        assert_eq!(
            children,
            vec!["b".to_string(), "c".to_string(), "d".to_string()]
        );
    }

    #[test]
    fn get_children_keys_nested() {
        let mut store = KVStore::new();

        store.insert_key("a/b/c");
        store.insert_key("a/b/d");
        store.insert_key("a/b/e");

        let children = store.get_children_keys("a/b").unwrap();

        assert_eq!(
            children,
            vec!["c".to_string(), "d".to_string(), "e".to_string()]
        );
    }

    #[test]
    fn remove_leaf_key() {
        let mut store = KVStore::new();

        store.insert_key("a/b/c");
        store.insert_key("a/b/d");

        store.remove_key("a/b/c");

        assert_eq!(store.get_value("a/b/c"), None);
        assert!(
            store
                .get_children_keys("a/b")
                .unwrap()
                .contains(&"d".to_string())
        );
    }

    #[test]
    fn remove_subtree() {
        let mut store = KVStore::new();

        store.insert_key_value("a/b/c", "v1");
        store.insert_key_value("a/b/d", "v2");
        store.insert_key("a/b/e/f");

        store.remove_key("a/b");

        assert_eq!(store.get_children_keys("a"), Some(vec![]));
        assert_eq!(store.get_value("a/b/c"), None);
        assert_eq!(store.get_value("a/b/d"), None);
    }

    #[test]
    fn remove_root_level_key() {
        let mut store = KVStore::new();

        store.insert_key("a/b");
        store.insert_key("c/d");

        store.remove_key("a");

        assert_eq!(store.get_children_keys("a"), None);
        assert!(store.get_children_keys("c").is_some());
    }

    #[test]
    fn remove_nonexistent_key_does_nothing() {
        let mut store = KVStore::new();

        store.insert_key("a/b");

        store.remove_key("x/y");

        assert!(store.get_children_keys("a").is_some());
    }
}
*/
