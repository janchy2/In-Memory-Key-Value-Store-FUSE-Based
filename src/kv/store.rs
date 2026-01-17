use std::{cell::RefCell, collections::BTreeMap, rc::Rc, time::SystemTime};

use crate::kv::string_table::AppendResult;

use super::key_ref::KeyRef;
use super::string_table::StringTable;

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

pub enum InsertResult {
    Inserted(usize),
    AlreadyExists(usize),
}

pub struct KVStore {
    key_table: Rc<RefCell<StringTable>>,
    value_table: Rc<RefCell<StringTable>>,
    key_value_map: BTreeMap<KeyRef, Option<usize>>,
    // Reserved keys and their values are reinserted after key_value_map is cleared on eviction.
    // They are not guaranteed to have the same index when reinserted unless they are the first keys inserted
    // after KVStore creation.
    reserved_keys: Vec<KeyRef>,
}

impl KVStore {
    pub fn new(key_capacity: usize, value_capacity: usize, max_capacity: usize) -> Self {
        let key_table = Rc::new(RefCell::new(StringTable::new(key_capacity, max_capacity)));
        let value_table = Rc::new(RefCell::new(StringTable::new(value_capacity, max_capacity)));
        let key_value_map = BTreeMap::new();
        let reserved_keys: Vec<KeyRef> = Vec::new();

        KVStore {
            key_table,
            value_table,
            key_value_map,
            reserved_keys,
        }
    }

    pub fn register_reserved_key_value(
        &mut self,
        parent_parent_idx: usize,
        parent: usize,
        key: &str,
        value: Option<&str>,
    ) -> bool {
        let idx = match self.insert_key(parent_parent_idx, parent, key) {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => {
                return false;
            }
        };

        if let Some(value) = value {
            if !self.insert_value(parent, idx, value.as_bytes()) {
                return false;
            }
        }

        let key_ref = KeyRef::new(self.key_table.clone(), idx, parent, parent_parent_idx);
        self.reserved_keys.push(key_ref);
        true
    }

    pub fn get_idx_for_key_str(&self, parent: usize, key: &str) -> Option<usize> {
        let temp_key = self.form_key_ref_from_str(parent, key);
        if let Some(key_ref) = self.get_key_in_map(&temp_key) {
            return Some(key_ref.get_idx());
        }
        None
    }

    pub fn insert_key(
        &mut self,
        parent_parent_idx: usize,
        parent: usize,
        key: &str,
    ) -> InsertResult {
        let temp_key = self.form_key_ref_from_str(parent, key);
        if let Some(key_ref) = self.get_key_in_map(&temp_key) {
            return InsertResult::AlreadyExists(key_ref.get_idx());
        }
        let bytes = key.as_bytes();
        let append_result = self.key_table.borrow_mut().append(bytes);
        let idx = match append_result {
            AppendResult::Ok(idx) => idx,
            AppendResult::CapacityExceeded => {
                self.evict_all();
                Self::expect_append_ok(self.key_table.borrow_mut().append(bytes))
            }
        };
        let key_ref = KeyRef::new(self.key_table.clone(), idx, parent, parent_parent_idx);
        let second_key_ref = key_ref.clone();
        // This means that the key exists already, but is expired
        if let Some(_) = self.key_value_map.insert(key_ref, None) {
            self.key_value_map.remove(&temp_key);
            self.key_value_map.insert(second_key_ref, None);
        }
        InsertResult::Inserted(idx)
    }

    pub fn insert_value(&mut self, parent: usize, idx: usize, value: &[u8]) -> bool {
        let key = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        if let None = self.get_key_in_map(&key) {
            return false;
        }
        let append_result = self.value_table.borrow_mut().append(value);
        let value_idx = match append_result {
            AppendResult::Ok(idx) => idx,
            AppendResult::CapacityExceeded => {
                self.evict_all();
                Self::expect_append_ok(self.key_table.borrow_mut().append(value))
            }
        };
        if let Some(v) = self.key_value_map.get_mut(&key) {
            *v = Some(value_idx);
            true
        } else {
            false
        }
    }

    pub fn get_children_keys_idx_and_names(&self, idx: usize) -> Vec<(usize, String)> {
        let start = KeyRef::get_boundary_for_children_search(idx);
        let end = KeyRef::get_boundary_for_children_search(idx + 1);

        let mut result = Vec::new();

        for (key_ref, _) in self.key_value_map.range(start..end) {
            if key_ref.is_expired() {
                continue;
            }
            let child_idx = key_ref.get_idx();
            if child_idx == idx {
                // This can happen for root
                continue;
            }
            result.push((child_idx, self.get_key_str(key_ref)));
        }

        result
    }

    pub fn get_value_for_key_idx(&self, parent: usize, idx: usize) -> Entry {
        // parent_parent_idx is irrelevant here, so it is set to 0
        let temp_key = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        self.get_value_str(&temp_key)
    }

    pub fn get_value_for_key_str(&self, parent: usize, key: &str) -> Entry {
        let temp_key = self.form_key_ref_from_str(parent, key);
        self.get_value_str(&temp_key)
    }

    pub fn get_parent_parent_idx(&self, parent: usize, idx: usize) -> Option<usize> {
        // parent_parent_idx is irrelevant here, so it is set to 0
        let temp_key = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        self.get_key_in_map(&temp_key)
            .map(|k| k.get_parent_parent_idx())
    }

    pub fn remove_key_str(&mut self, parent: usize, key: &str) -> RemoveResult {
        let idx = match self.get_idx_for_key_str(parent, key) {
            Some(idx) => idx,
            None => return RemoveResult::NotFound,
        };
        self.remove_key(parent, idx)
    }

    pub fn set_expiration(
        &mut self,
        parent: usize,
        idx: usize,
        expires_at: Option<SystemTime>,
    ) -> bool {
        let parent_parent_idx = match self.get_parent_parent_idx(parent, idx) {
            Some(idx) => idx,
            None => return false,
        };

        let mut key_ref = KeyRef::new(self.key_table.clone(), idx, parent, parent_parent_idx);
        let value = match self.key_value_map.get(&key_ref) {
            Some(value) => value.clone(),
            None => panic!("Key not found, but it should exist"),
        };

        match self.remove_key(parent, idx) {
            RemoveResult::Removed => {}
            RemoveResult::HasChildren => panic!("Key not found, but it should exist"),
            RemoveResult::NotFound => panic!("Key not found, but it should exist"),
        };
        key_ref.set_expiration(expires_at);
        self.key_value_map.insert(key_ref, value);
        true
    }

    fn evict_all(&mut self) {
        let to_reinsert = self.get_keys_and_values_to_reinsert();
        self.key_value_map.clear();
        self.reserved_keys.clear();
        self.key_table.borrow_mut().clear();
        self.value_table.borrow_mut().clear();
        self.reinsert_reserved_keys_and_values(to_reinsert);
    }

    fn get_keys_and_values_to_reinsert(&mut self) -> Vec<(KeyRef, String, Option<String>)> {
        let mut result = Vec::new();
        for key_ref in &self.reserved_keys {
            let key = self.get_key_str(&key_ref);
            let value = match self.get_value_str(&key_ref) {
                Entry::NoValue => None,
                Entry::NotFound => panic!("Reserved key not found"),
                Entry::Value(value) => Some(value),
            };

            result.push((key_ref.clone(), key, value));
        }

        result
    }

    fn reinsert_reserved_keys_and_values(
        &mut self,
        to_reinsert: Vec<(KeyRef, String, Option<String>)>,
    ) {
        for (key_ref, key, value) in to_reinsert {
            let value_ref = value.as_deref();
            if !self.register_reserved_key_value(
                key_ref.get_parent_parent_idx(),
                key_ref.get_parent_idx(),
                &key,
                value_ref,
            ) {
                panic!("Reinserting reserved keys and values failed");
            }
        }
    }

    fn expect_append_ok(res: AppendResult) -> usize {
        match res {
            AppendResult::Ok(idx) => idx,
            AppendResult::CapacityExceeded => {
                panic!("Capacity exceeded when it should not")
            }
        }
    }

    fn get_key_in_map(&self, key_ref: &KeyRef) -> Option<&KeyRef> {
        match self.key_value_map.get_key_value(key_ref) {
            Some((key, _)) => {
                if key.is_expired() {
                    return None;
                }
                Some(key)
            }
            None => None,
        }
    }

    fn remove_key(&mut self, parent: usize, idx: usize) -> RemoveResult {
        let children = self.get_children_keys_idx_and_names(idx);
        if children.len() > 0 {
            return RemoveResult::HasChildren;
        }
        // parent_parent_idx is irrelevant here, so it is set to 0
        let key_ref = KeyRef::new(self.key_table.clone(), idx, parent, 0);
        match self.key_value_map.remove(&key_ref) {
            Some(_) => RemoveResult::Removed,
            None => return RemoveResult::NotFound,
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
        match self.key_value_map.get_key_value(key_ref) {
            Some((key, value_idx)) => {
                if key.is_expired() {
                    return Entry::NotFound;
                }
                match value_idx {
                    Some(value_idx) => {
                        let value_table_ref = self.value_table.borrow();
                        let bytes = value_table_ref
                            .get(*value_idx)
                            .expect("Get should not fail here");

                        Entry::Value(String::from_utf8(bytes.to_vec()).unwrap())
                    }
                    None => Entry::NoValue,
                }
            }
            None => Entry::NotFound,
        }
    }

    fn form_key_ref_from_str(&self, parent: usize, key: &str) -> KeyRef {
        let bytes = key.as_bytes();
        let capacity = bytes.len() + 1;
        let temp_table = Rc::new(RefCell::new(StringTable::new(capacity, capacity)));
        let temp_idx = Self::expect_append_ok(temp_table.borrow_mut().append(bytes));
        // parent_parent_idx is irrelevant here, so it is set to 0
        KeyRef::new(temp_table, temp_idx, parent, 0)
    }
}

#[cfg(test)]
mod tests {
use std::{thread::sleep, time::Duration};

    use super::*;

    #[test]
    fn insert_key_and_lookup_by_string() {
        let mut store = KVStore::new(10, 10, 10);

        let idx = match store.insert_key(0, 0, "a") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };

        assert_eq!(store.get_idx_for_key_str(0, "a"), Some(idx));
    }

    #[test]
    fn duplicate_key_under_same_parent_is_rejected() {
        let mut store = KVStore::new(10, 10, 10);

        let idx = match store.insert_key(0, 0, "a") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };

        match store.insert_key(0, 0, "a") {
            InsertResult::Inserted(_) => panic!("Expected AlreadyExists"),
            InsertResult::AlreadyExists(existing_idx) => {
                assert_eq!(existing_idx, idx);
            }
        }
    }

    #[test]
    fn same_key_name_under_different_parents_is_allowed() {
        let mut store = KVStore::new(10, 10, 10);

        let a: usize = match store.insert_key(0, 0, "a") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        let b: usize = match store.insert_key(0, 0, "b") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };

        let a1: usize = match store.insert_key(0, a, "x") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        let b1: usize = match store.insert_key(0, b, "x") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };

        assert_ne!(a1, b1);
    }

    #[test]
    fn insert_value_and_retrieve_it() {
        let mut store = KVStore::new(10, 10, 10);

        let idx: usize = match store.insert_key(0, 0, "file") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        assert!(store.insert_value(0, idx, b"hello"));

        match store.get_value_for_key_idx(0, idx) {
            Entry::Value(v) => assert_eq!(v, "hello"),
            _ => panic!("Expected value"),
        }
    }

    #[test]
    fn get_children_of_key() {
        let mut store = KVStore::new(10, 10, 10);

        let idx: usize = match store.insert_key(0, 0, "key") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        store.insert_key(0, idx, "a");
        store.insert_key(0, idx, "b");
        store.insert_key(0, idx, "c");

        let children = store.get_children_keys_idx_and_names(idx);
        let names: Vec<_> = children.into_iter().map(|(_, n)| n).collect();

        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn parent_parent_idx_is_correct() {
        let mut store = KVStore::new(10, 10, 10);

        let a: usize = match store.insert_key(0, 0, "a") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        let b: usize = match store.insert_key(0, a, "b") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };

        let ppi = store.get_parent_parent_idx(a, b).unwrap();
        assert_eq!(ppi, 0);
    }

    #[test]
    fn remove_leaf_key() {
        let mut store = KVStore::new(10, 10, 10);

        let a: usize = match store.insert_key(0, 0, "a") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        let b: usize = match store.insert_key(0, a, "b") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };

        assert!(matches!(
            store.remove_key_str(a, "b"),
            RemoveResult::Removed
        ));

        assert!(matches!(store.get_value_for_key_idx(a, b), Entry::NotFound));
    }

    #[test]
    fn remove_key_with_children_fails() {
        let mut store = KVStore::new(10, 10, 10);

        let a: usize = match store.insert_key(0, 0, "a") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        store.insert_key(0, a, "b");

        assert!(matches!(
            store.remove_key_str(0, "a"),
            RemoveResult::HasChildren
        ));
    }

    #[test]
    fn remove_nonexistent_key() {
        let mut store = KVStore::new(10, 10, 10);

        assert!(matches!(
            store.remove_key_str(0, "missing"),
            RemoveResult::NotFound
        ));
    }

    #[test]
    fn expired_key_is_not_found_by_lookup() {
        let mut store = KVStore::new(10, 10, 10);

        let idx: usize = match store.insert_key(0, 0, "temp") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        store.insert_value(0, idx, b"value");

        let expired_at = SystemTime::now() - Duration::from_secs(1);
        assert!(store.set_expiration(0, idx, Some(expired_at)));

        assert_eq!(store.get_idx_for_key_str(0, "temp"), None);
    }

    #[test]
    fn expired_key_returns_not_found_on_get_value() {
        let mut store = KVStore::new(10, 10, 10);

        let idx: usize = match store.insert_key(0, 0, "temp") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        store.insert_value(1, idx, b"value");

        let expired_at = SystemTime::now() - Duration::from_secs(1);
        store.set_expiration(0, idx, Some(expired_at));

        match store.get_value_for_key_idx(0, idx) {
            Entry::NotFound => {}
            _ => panic!("Expired key must behave as NotFound"),
        }
    }

    #[test]
    fn expired_key_is_not_listed_as_child() {
        let mut store = KVStore::new(10, 10, 10);

        let idx: usize = match store.insert_key(0, 0, "child") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };

        let expired_at = SystemTime::now() - Duration::from_secs(1);
        store.set_expiration(0, idx, Some(expired_at));

        let children = store.get_children_keys_idx_and_names(0);
        assert!(children.is_empty());
    }

    #[test]
    fn ttl_is_preserved_when_value_is_overwritten() {
        let mut store = KVStore::new(10, 10, 10);

        let idx: usize = match store.insert_key(0, 0, "file") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        store.insert_value(0, idx, b"old");

        let expires_at = SystemTime::now() + Duration::from_secs(3);
        store.set_expiration(0, idx, Some(expires_at));

        store.insert_value(0, idx, b"new");

        assert_eq!(store.get_idx_for_key_str(0, "file"), Some(idx));

        match store.get_value_for_key_idx(0, idx) {
            Entry::Value(v) => assert_eq!(v, "new"),
            _ => panic!("Expected updated value"),
        }

        sleep(Duration::from_secs(3));

        assert_eq!(store.get_idx_for_key_str(0, "file"), None);
    }

    #[test]
    fn non_expired_key_is_visible() {
        let mut store = KVStore::new(10, 10, 10);

        let idx: usize = match store.insert_key(0, 0, "alive") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };
        store.insert_value(0, idx, b"ok");

        let expires_at = SystemTime::now() + Duration::from_secs(60);
        store.set_expiration(0, idx, Some(expires_at));

        assert_eq!(store.get_idx_for_key_str(0, "alive"), Some(idx));

        match store.get_value_for_key_idx(0, idx) {
            Entry::Value(v) => assert_eq!(v, "ok"),
            _ => panic!("Expected value for non-expired key"),
        }
    }

    #[test]
    fn can_reinsert_key_with_same_name_after_expiration() {
        let mut store = KVStore::new(10, 10, 10);
        let idx1: usize = match store.insert_key(0, 0, "temp") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Expected Inserted"),
        };

        let expires_at = SystemTime::now() + Duration::from_millis(50);
        assert!(store.set_expiration(0, idx1, Some(expires_at)));

        sleep(Duration::from_millis(80));
        assert_eq!(store.get_idx_for_key_str(0, "temp"), None);

        let idx2: usize = match store.insert_key(0, 0, "temp") {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => panic!("Reinserting expired key should succeed"),
        };
        assert_eq!(store.get_idx_for_key_str(0, "temp"), Some(idx2));
    }

    #[test]
    fn reserved_keys_survive_eviction() {
        let mut store = KVStore::new(10, 10, 20);

        assert!(store.register_reserved_key_value(0, 0, "rkey1", Some("val1")));
        assert!(store.register_reserved_key_value(0, 0, "rkey2", None));

        store.evict_all();

        let idx1 = store.get_idx_for_key_str(0, "rkey1").unwrap();
        let idx2 = store.get_idx_for_key_str(0, "rkey2").unwrap();

        match store.get_value_for_key_idx(0, idx1) {
            Entry::Value(v) => assert_eq!(v, "val1"),
            _ => panic!("Expected value"),
        }

        match store.get_value_for_key_idx(0, idx2) {
            Entry::NoValue => {}
            _ => panic!("Expected no value"),
        }
    }
}
