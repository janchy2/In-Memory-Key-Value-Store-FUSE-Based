use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

use super::key_ref::KeyRef;
use crate::storage::string_table::StringTable;

const KEY_TABLE_SIZE: usize = 1024;
const VALUE_TABLE_SIZE: usize = 1024;
// TODO: Parametrize this

pub struct KVStore {
    key_table: Rc<RefCell<StringTable>>,
    value_table: Rc<RefCell<StringTable>>,
    key_value_map: BTreeMap<KeyRef, Option<usize>>,
}

impl KVStore {
    pub fn new() -> Self {
        let key_table = Rc::new(RefCell::new(StringTable::new(KEY_TABLE_SIZE)));
        let value_table = Rc::new(RefCell::new(StringTable::new(VALUE_TABLE_SIZE)));
        let key_value_map = BTreeMap::new();
        KVStore {
            key_table,
            value_table,
            key_value_map,
        }
    }

    fn lookup_key_value_for_key_part(
        &self,
        str_bytes: &[u8],
        parent_idx: Option<usize>,
    ) -> Option<(usize, Option<usize>)> {
        let temp_table = Rc::new(RefCell::new(StringTable::new(str_bytes.len() + 1)));
        let temp_idx = temp_table
            .borrow_mut()
            .append(str_bytes)
            .expect("Append to temp table should not fail");
        let temp_key = KeyRef::new(temp_table, temp_idx, parent_idx);

        self.key_value_map
            .get_key_value(&temp_key)
            .map(|(k, v)| (k.get_idx(), *v))
    }

    fn lookup_key_value_for_key(&self, key: &str) -> Option<(usize, Option<usize>)> {
        let mut parent = None;
        let mut value = None;

        for part in key.split('/') {
            let bytes = part.as_bytes();

            let (key_idx, value_idx) = self.lookup_key_value_for_key_part(bytes, parent)?;
            parent = Some(key_idx);
            value = value_idx;
        }

        Some((parent.unwrap(), value))
    }

    pub fn insert_key(&mut self, key: &str) -> bool {
        let mut parent = None;
        let mut inserted = false;

        for part in key.split('/') {
            let bytes = part.as_bytes();

            match self.lookup_key_value_for_key_part(bytes, parent) {
                Some((existing_key, value)) => {
                    // Cannot insert below a key that already has a value
                    if value.is_some() {
                        return false;
                    }
                    parent = Some(existing_key);
                }
                None => {
                    let idx = self
                        .key_table
                        .borrow_mut()
                        .append(bytes)
                        .expect("Append should not fail");

                    self.key_value_map
                        .insert(KeyRef::new(self.key_table.clone(), idx, parent), None);

                    parent = Some(idx);
                    inserted = true;
                }
            }
        }

        inserted
    }

    pub fn insert_key_value(&mut self, key: &str, value: &str) -> bool {
        let mut parent = None;
        let mut iter = key.split('/').peekable();

        while let Some(part) = iter.next() {
            let bytes = part.as_bytes();
            let is_last = iter.peek().is_none();

            match self.lookup_key_value_for_key_part(bytes, parent) {
                Some((existing_key, existing_value)) => {
                    // Parent keys must not already have values
                    if !is_last && existing_value.is_some() {
                        return false;
                    }

                    parent = Some(existing_key);

                    // Assign value at leaf
                    if is_last {
                        let value_idx = self
                            .value_table
                            .borrow_mut()
                            .append(value.as_bytes())
                            .expect("Append should not fail");

                        let key = KeyRef::new(self.key_table.clone(), existing_key, parent);
                        if let Some(v) = self.key_value_map.get_mut(&key) {
                            *v = Some(value_idx);
                        }
                    }
                }

                None => {
                    let idx = self
                        .key_table
                        .borrow_mut()
                        .append(bytes)
                        .expect("Append should not fail");

                    let value_idx = if is_last {
                        Some(
                            self.value_table
                                .borrow_mut()
                                .append(value.as_bytes())
                                .expect("Append should not fail"),
                        )
                    } else {
                        None
                    };

                    self.key_value_map
                        .insert(KeyRef::new(self.key_table.clone(), idx, parent), value_idx);

                    parent = Some(idx);
                }
            }
        }

        true
    }

    pub fn get_children_keys(&self, parent_key: &str) -> Option<Vec<String>> {
        let (parent_idx, _) = self.lookup_key_value_for_key(parent_key)?;

        let start = KeyRef::get_boundary_for_children_search(parent_idx);
        let end = KeyRef::get_boundary_for_children_search(parent_idx + 1);

        let mut result = Vec::new();

        for (key_ref, _) in self.key_value_map.range(start..end) {
            let key_table_ref = self.key_table.borrow();
            let bytes = key_table_ref
                .get(key_ref.get_idx())
                .expect("Get should not fail here");
            let s = String::from_utf8(bytes.to_vec()).expect("Keys must be valid UTF-8");
            result.push(s);
        }

        Some(result)
    }

    pub fn get_value(&self, key: &str) -> Option<String> {
        let (_, value) = self.lookup_key_value_for_key(key)?;

        let value_idx = value?;
        let value_table_ref = self.value_table.borrow();
        let bytes = value_table_ref
            .get(value_idx)
            .expect("Get should not fail here");
        Some(String::from_utf8(bytes.to_vec()).expect("Keys must be valid UTF-8"))
    }

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
}

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
