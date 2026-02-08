use core::panic;
use std::{
    ffi::OsStr,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyWrite, Request, TimeOrNow, mount2,
};

use crate::{
    config::KvConfig,
    fuse::{
        helpers::{
            create_file_attr, get_child_ino_and_file_type, get_entry_for_ino, osstr_to_name,
            parse_ttl, try_get_value_from_hook,
        },
        inode::{form_ino, ino_to_idx, ino_to_parent_idx},
    },
    kv::store::{Entry, InsertResult, KVStore, RemoveResult},
};

pub enum HandleResult<T> {
    Ok(T),
    Error(i32),
}

pub struct ReadDirEntry {
    pub ino: u64,
    pub file_type: FileType,
    pub name: String,
}

pub struct LookupResult {
    pub attr: FileAttr,
}

pub struct CreateResult {
    pub attr: FileAttr,
}

const TTL: Duration = Duration::from_secs(1);
const ROOT_INDEX: usize = 1;

pub struct KVFS {
    kv_store: Arc<RwLock<KVStore>>,
    hooks_path: String,
}

impl KVFS {
    fn new(config: &KvConfig) -> Self {
        let kv_store = Arc::new(RwLock::new(KVStore::new(
            config.key_capacity,
            config.value_capacity,
            config.max_capacity,
        )));
        // The filesystem requires the root key to have a fixed index (ROOT_INDEX) to
        // support deterministic inode to key mapping. To achieve this, we first insert
        // a dummy key at index 0, and then insert the root key at index ROOT_INDEX (1)
        let mut guard = kv_store.write().unwrap();
        let reserved_empty = guard.register_reserved_key_value(0, 0, "", None);
        let reserved_root = guard.register_reserved_key_value(ROOT_INDEX, ROOT_INDEX, "", None);
        if !(reserved_empty && reserved_root) {
            panic!("Registering reserved keys failed");
        }
        drop(guard);
        let hooks_path = config.hooks_path.clone();
        Self {
            kv_store,
            hooks_path,
        }
    }

    #[cfg(test)]
    pub fn new_for_test(config: &KvConfig) -> Self {
        Self::new(config)
    }

    pub fn mount(config: &KvConfig) -> Result<(), std::io::Error> {
        let fs = KVFS::new(config);

        let options = [
            MountOption::FSName("kvfs".to_string()),
            MountOption::AutoUnmount,
            MountOption::AllowRoot,
        ];

        mount2(fs, &config.mountpoint, &options)
    }

    pub fn handle_getattr(&self, ino: u64) -> HandleResult<FileAttr> {
        let guard = self.kv_store.read().unwrap();
        match create_file_attr(ino, &guard) {
            Ok(attr) => HandleResult::Ok(attr),
            Err(_) => HandleResult::Error(libc::ENOENT),
        }
    }

    pub fn handle_lookup(&self, parent: u64, name: &str) -> HandleResult<LookupResult> {
        let parent_idx = ino_to_idx(parent);
        {
            let guard = self.kv_store.read().unwrap();
            if let Some(idx) = guard.get_idx_for_key_str(parent_idx, name) {
                let ino = form_ino(parent_idx, idx);
                let attr = match create_file_attr(ino, &guard) {
                    Ok(ok_attr) => ok_attr,
                    Err(_) => panic!("Key not found, but it should exist"),
                };
                return HandleResult::Ok(LookupResult { attr });
            }
        }

        // When a looked up key does not exist, an executable file with the same name is looked for in the given hooks directory.
        // If it exists, the value it produces is saved in the key-value store for the given key.
        let value = match try_get_value_from_hook(name, &self.hooks_path) {
            Some(v) => v,
            None => return HandleResult::Error(libc::ENOENT),
        };

        let parent_parent_idx = ino_to_parent_idx(parent);
        let mut guard = self.kv_store.write().unwrap();
        let idx = {
            match guard.insert_key(parent_parent_idx, parent_idx, name) {
                InsertResult::AlreadyExists(idx) => idx,
                InsertResult::Inserted(idx) => {
                    if !guard.insert_value(parent_idx, idx, value.as_bytes()) {
                        panic!("Failed to insert hook value");
                    }
                    idx
                }
            }
        };

        let ino = form_ino(parent_idx, idx);
        let attr = match create_file_attr(ino, &guard) {
            Ok(ok_attr) => ok_attr,
            Err(_) => panic!("Key not found, but it should exist"),
        };
        HandleResult::Ok(LookupResult { attr })
    }

    pub fn handle_readdir(&self, ino: u64, offset: i64) -> HandleResult<Vec<ReadDirEntry>> {
        let guard = self.kv_store.read().unwrap();

        let entry = get_entry_for_ino(ino, &guard);
        match entry {
            Entry::NotFound => return HandleResult::Error(libc::ENOENT),
            Entry::NoValue => {}
            Entry::Value(_) => return HandleResult::Error(libc::ENOTDIR),
        }

        let idx = ino_to_idx(ino);
        let parent_idx = ino_to_parent_idx(ino);

        let mut entries = Vec::new();
        let mut current_offset = 1;

        if current_offset > offset {
            entries.push(ReadDirEntry {
                ino,
                file_type: FileType::Directory,
                name: ".".to_string(),
            });
        }
        current_offset += 1;

        if current_offset > offset {
            let parent_parent_idx = guard.get_parent_parent_idx(parent_idx, idx).unwrap();
            let parent_ino = form_ino(parent_parent_idx, parent_idx);
            entries.push(ReadDirEntry {
                ino: parent_ino,
                file_type: FileType::Directory,
                name: "..".to_string(),
            });
        }
        current_offset += 1;

        let children = guard.get_children_keys_idx_and_names(idx);

        for (child_idx, child_name) in children {
            if current_offset <= offset {
                current_offset += 1;
                continue;
            }

            let (child_ino, kind) = get_child_ino_and_file_type(idx, child_idx, &guard);
            entries.push(ReadDirEntry {
                ino: child_ino,
                file_type: kind,
                name: child_name.to_string(),
            });
            current_offset += 1;
        }

        HandleResult::Ok(entries)
    }

    pub fn handle_mkdir(&self, parent: u64, name: &str) -> HandleResult<CreateResult> {
        let mut guard = self.kv_store.write().unwrap();

        let entry = get_entry_for_ino(parent, &guard);
        match entry {
            Entry::NotFound => return HandleResult::Error(libc::ENOENT),
            Entry::NoValue => {}
            Entry::Value(_) => return HandleResult::Error(libc::ENOTDIR),
        }

        let parent_idx = ino_to_idx(parent);
        let parent_parent_idx = ino_to_parent_idx(parent);
        let idx = match guard.insert_key(parent_parent_idx, parent_idx, name) {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => return HandleResult::Error(libc::EEXIST),
        };

        let ino = form_ino(parent_idx, idx);
        let attr = FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: FileType::Directory,
            perm: 0o755,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 512,
        };
        HandleResult::Ok(CreateResult { attr })
    }

    pub fn handle_mknod(&self, parent: u64, name: &str) -> HandleResult<CreateResult> {
        let mut guard = self.kv_store.write().unwrap();

        let entry = get_entry_for_ino(parent, &guard);
        match entry {
            Entry::NotFound => return HandleResult::Error(libc::ENOENT),
            Entry::NoValue => {}
            Entry::Value(_) => return HandleResult::Error(libc::ENOTDIR),
        }

        let parent_idx = ino_to_idx(parent);
        let parent_parent_idx = ino_to_parent_idx(parent);
        let idx = match guard.insert_key(parent_parent_idx, parent_idx, name) {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => return HandleResult::Error(libc::EEXIST),
        };

        if !guard.insert_value(parent_idx, idx, &[]) {
            panic!("Setting empty string value for existing key failed")
        }

        let ino = form_ino(parent_idx, idx);
        let attr = FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: FileType::RegularFile,
            perm: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 512,
        };
        HandleResult::Ok(CreateResult { attr })
    }

    pub fn handle_rmdir(&self, parent: u64, name: &str) -> HandleResult<()> {
        let mut guard = self.kv_store.write().unwrap();

        let parent_idx = ino_to_idx(parent);
        let entry = guard.get_value_for_key_str(parent_idx, name);
        match entry {
            Entry::NotFound => return HandleResult::Error(libc::ENOENT),
            Entry::NoValue => {}
            Entry::Value(_) => return HandleResult::Error(libc::ENOTDIR),
        }

        match guard.remove_key_str(parent_idx, name) {
            RemoveResult::NotFound => panic!("Key not found, but it should exist"),
            RemoveResult::HasChildren => HandleResult::Error(libc::ENOTEMPTY),
            RemoveResult::Removed => HandleResult::Ok(()),
        }
    }

    pub fn handle_unlink(&self, parent: u64, name: &str) -> HandleResult<()> {
        let mut guard = self.kv_store.write().unwrap();

        let parent_idx = ino_to_idx(parent);
        let entry = guard.get_value_for_key_str(parent_idx, name);
        match entry {
            Entry::NotFound => return HandleResult::Error(libc::ENOENT),
            Entry::NoValue => return HandleResult::Error(libc::EISDIR),
            Entry::Value(_) => {}
        }

        match guard.remove_key_str(parent_idx, name) {
            RemoveResult::NotFound => panic!("Key not found, but it should exist"),
            RemoveResult::HasChildren => panic!("Key is a file, but it has children"),
            RemoveResult::Removed => HandleResult::Ok(()),
        }
    }

    pub fn handle_write(&self, ino: u64, offset: i64, data: &[u8]) -> HandleResult<u32> {
        // The filesystem enforces atomic value replacement
        if offset != 0 {
            return HandleResult::Error(libc::EINVAL);
        }

        let mut guard = self.kv_store.write().unwrap();

        let entry = get_entry_for_ino(ino, &guard);
        match entry {
            Entry::NotFound => return HandleResult::Error(libc::ENOENT),
            Entry::NoValue => return HandleResult::Error(libc::EISDIR),
            Entry::Value(_) => {}
        }

        let parent = ino_to_parent_idx(ino);
        let idx = ino_to_idx(ino);
        if !guard.insert_value(parent, idx, data) {
            panic!("Key not found, but it should exist");
        }
        HandleResult::Ok(data.len() as u32)
    }

    pub fn handle_read(&self, ino: u64, offset: i64, size: u32) -> HandleResult<Vec<u8>> {
        let guard = self.kv_store.read().unwrap();

        let entry = get_entry_for_ino(ino, &guard);
        let value = match entry {
            Entry::NotFound => return HandleResult::Error(libc::ENOENT),
            Entry::NoValue => return HandleResult::Error(libc::EISDIR),
            Entry::Value(value) => value,
        };

        if offset < 0 {
            return HandleResult::Error(libc::EINVAL);
        }

        let offset = offset as usize;
        let data = value.as_bytes();

        if offset >= data.len() {
            return HandleResult::Ok(vec![]);
        }

        let end = std::cmp::min(offset + size as usize, data.len());
        HandleResult::Ok(data[offset..end].to_vec())
    }

    pub fn handle_setxattr(&self, ino: u64, name: &str, value: &[u8]) -> HandleResult<()> {
        if name != "user.ttl" {
            return HandleResult::Error(libc::ENOTSUP);
        }

        let mut guard = self.kv_store.write().unwrap();

        let entry = get_entry_for_ino(ino, &guard);
        match entry {
            Entry::NotFound => return HandleResult::Error(libc::ENOENT),
            Entry::NoValue => return HandleResult::Error(libc::EISDIR),
            Entry::Value(_) => {}
        }

        let expires_at = match parse_ttl(value) {
            Ok(v) => v,
            Err(e) => return HandleResult::Error(e),
        };

        let idx = ino_to_idx(ino);
        let parent = ino_to_parent_idx(ino);

        if !guard.set_expiration(parent, idx, expires_at) {
            panic!("Key not found, but it should exist")
        }
        HandleResult::Ok(())
    }

    pub fn handle_removexattr(&self, ino: u64, name: &str) -> HandleResult<()> {
        self.handle_setxattr(ino, name, b"0")
    }
}

impl Filesystem for KVFS {
    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        match self.handle_getattr(ino) {
            HandleResult::Ok(attr) => reply.attr(&TTL, &attr),
            HandleResult::Error(err) => reply.error(err),
        }
    }

    fn setattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        _size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        // Attribute updates are ignored, as this filesystem does not track mutable
        // metadata such as permissions, ownership, or timestamps. The function
        // simply returns the current attributes to support basic filesystem
        // operations like `touch`.
        self.getattr(_req, ino, _fh, reply);
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let key = match osstr_to_name(name) {
            Ok(k) => k,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        match self.handle_lookup(parent, key) {
            HandleResult::Ok(result) => reply.entry(&TTL, &result.attr, 0),
            HandleResult::Error(err) => reply.error(err),
        }
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let entries = match self.handle_readdir(ino, offset) {
            HandleResult::Ok(entries) => entries,
            HandleResult::Error(err) => {
                reply.error(err);
                return;
            }
        };

        let mut current_offset = 1;
        for entry in entries {
            if current_offset <= offset {
                current_offset += 1;
                continue;
            }
            if reply.add(entry.ino, current_offset, entry.file_type, entry.name) {
                return;
            }
            current_offset += 1;
        }

        reply.ok();
    }

    fn mkdir(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let key_str = match osstr_to_name(name) {
            Ok(s) => s,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        match self.handle_mkdir(parent, key_str) {
            HandleResult::Ok(result) => reply.entry(&TTL, &result.attr, 0),
            HandleResult::Error(err) => reply.error(err),
        }
    }

    fn mknod(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        let key_str = match osstr_to_name(name) {
            Ok(s) => s,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        match self.handle_mknod(parent, key_str) {
            HandleResult::Ok(result) => reply.entry(&TTL, &result.attr, 0),
            HandleResult::Error(err) => reply.error(err),
        }
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let key_str = match osstr_to_name(name) {
            Ok(s) => s,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        match self.handle_rmdir(parent, key_str) {
            HandleResult::Ok(()) => reply.ok(),
            HandleResult::Error(err) => reply.error(err),
        }
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let key_str = match osstr_to_name(name) {
            Ok(s) => s,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        match self.handle_unlink(parent, key_str) {
            HandleResult::Ok(()) => reply.ok(),
            HandleResult::Error(err) => reply.error(err),
        }
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        match self.handle_write(ino, offset, data) {
            HandleResult::Ok(written) => reply.written(written),
            HandleResult::Error(err) => reply.error(err),
        }
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        match self.handle_read(ino, offset, size) {
            HandleResult::Ok(data) => reply.data(&data),
            HandleResult::Error(err) => reply.error(err),
        }
    }

    fn setxattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        name: &OsStr,
        value: &[u8],
        _flags: i32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        let name_str = match name.to_str() {
            Some(s) => s,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        match self.handle_setxattr(ino, name_str, value) {
            HandleResult::Ok(()) => reply.ok(),
            HandleResult::Error(err) => reply.error(err),
        }
    }

    fn removexattr(&mut self, _req: &Request<'_>, ino: u64, name: &OsStr, reply: ReplyEmpty) {
        let name_str = match name.to_str() {
            Some(s) => s,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        match self.handle_removexattr(ino, name_str) {
            HandleResult::Ok(()) => reply.ok(),
            HandleResult::Error(err) => reply.error(err),
        }
    }
}
