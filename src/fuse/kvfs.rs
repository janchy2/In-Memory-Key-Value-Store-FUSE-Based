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

    pub fn mount(config: &KvConfig) -> Result<(), std::io::Error> {
        let fs = KVFS::new(config);

        let options = [
            MountOption::FSName("kvfs".to_string()),
            MountOption::AutoUnmount,
        ];

        mount2(fs, &config.mountpoint, &options)
    }
}

impl Filesystem for KVFS {
    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let guard = self.kv_store.read().unwrap();

        let attr = {
            match create_file_attr(ino, &guard) {
                Ok(ok_attr) => ok_attr,
                Err(_) => {
                    reply.error(libc::ENOENT);
                    return;
                }
            }
        };
        reply.attr(&TTL, &attr);
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

        let parent_idx = ino_to_idx(parent);
        {
            let guard = self.kv_store.read().unwrap();
            if let Some(idx) = guard.get_idx_for_key_str(parent_idx, key) {
                let ino = form_ino(parent_idx, idx);
                let attr = match create_file_attr(ino, &guard) {
                    Ok(ok_attr) => ok_attr,
                    Err(_) => panic!("Key not found, but it should exist"),
                };
                reply.entry(&TTL, &attr, 0);
                return;
            }
        }

        // When a looked up key does not exist, an executable file with the same name is looked for in the given hooks directory.
        // If it exists, the value it produces is saved in the key-value store for the given key.
        let value = match try_get_value_from_hook(key, &self.hooks_path) {
            Some(v) => v,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        let parent_parent_idx = ino_to_parent_idx(parent);
        let mut guard = self.kv_store.write().unwrap();
        let idx = {
            match guard.insert_key(parent_parent_idx, parent_idx, key) {
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
        reply.entry(&TTL, &attr, 0);
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let guard = self.kv_store.read().unwrap();

        let entry = get_entry_for_ino(ino, &guard);
        match entry {
            Entry::NotFound => {
                reply.error(libc::ENOENT);
                return;
            }
            Entry::NoValue => {}
            Entry::Value(_) => {
                reply.error(libc::ENOTDIR);
                return;
            }
        }

        let idx = ino_to_idx(ino);
        let parent_idx = ino_to_parent_idx(ino);

        let mut current_offset = 1;
        if current_offset > offset {
            if reply.add(ino, current_offset, FileType::Directory, ".") {
                return;
            }
        }
        current_offset += 1;
        if current_offset > offset {
            let parent_parent_idx = guard.get_parent_parent_idx(parent_idx, idx).unwrap();
            let parent_ino = form_ino(parent_parent_idx, parent_idx);
            if reply.add(parent_ino, current_offset, FileType::Directory, "..") {
                return;
            }
        }
        current_offset += 1;

        let children = guard.get_children_keys_idx_and_names(idx);

        for (child_idx, child_name) in children {
            if current_offset <= offset {
                current_offset += 1;
                continue;
            }

            let (ino, kind) = get_child_ino_and_file_type(idx, child_idx, &guard);

            if reply.add(ino, current_offset, kind, child_name) {
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

        let mut guard = self.kv_store.write().unwrap();

        let entry = get_entry_for_ino(parent, &guard);
        match entry {
            Entry::NotFound => {
                reply.error(libc::ENOENT);
                return;
            }
            Entry::NoValue => {}
            Entry::Value(_) => {
                reply.error(libc::ENOTDIR);
                return;
            }
        }

        let parent_idx = ino_to_idx(parent);
        let parent_parent_idx = ino_to_parent_idx(parent);
        let idx = match guard.insert_key(parent_parent_idx, parent_idx, key_str) {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => return reply.error(libc::EEXIST),
        };

        let ino = form_ino(parent_idx, idx);
        // This filesystem does not track mutable metadata such as permissions, ownership, or timestamps,
        // so those types of values are mocked.
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
        reply.entry(&TTL, &attr, 0);
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

        let mut guard = self.kv_store.write().unwrap();

        let entry = get_entry_for_ino(parent, &guard);
        match entry {
            Entry::NotFound => {
                reply.error(libc::ENOENT);
                return;
            }
            Entry::NoValue => {}
            Entry::Value(_) => {
                reply.error(libc::ENOTDIR);
                return;
            }
        }

        let parent_idx = ino_to_idx(parent);
        let parent_parent_idx = ino_to_parent_idx(parent);
        let idx = match guard.insert_key(parent_parent_idx, parent_idx, key_str) {
            InsertResult::Inserted(idx) => idx,
            InsertResult::AlreadyExists(_) => return reply.error(libc::EEXIST),
        };

        if !guard.insert_value(parent_idx, idx, &[]) {
            panic!("Setting empty string value for existing key failed")
        }

        let ino = form_ino(parent_idx, idx);
        // This filesystem does not track mutable metadata such as permissions, ownership, or timestamps,
        // so those types of values are mocked.
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
        reply.entry(&TTL, &attr, 0);
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let key_str = match osstr_to_name(name) {
            Ok(s) => s,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        let mut guard = self.kv_store.write().unwrap();

        let parent_idx = ino_to_idx(parent);
        let entry = guard.get_value_for_key_str(parent_idx, key_str);
        match entry {
            Entry::NotFound => {
                reply.error(libc::ENOENT);
                return;
            }
            Entry::NoValue => {}
            Entry::Value(_) => {
                reply.error(libc::ENOTDIR);
                return;
            }
        }

        match guard.remove_key_str(parent_idx, key_str) {
            RemoveResult::NotFound => panic!("Key not found, but it should exist"),
            RemoveResult::HasChildren => reply.error(libc::ENOTEMPTY),
            RemoveResult::Removed => reply.ok(),
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

        let mut guard = self.kv_store.write().unwrap();

        let parent_idx = ino_to_idx(parent);
        let entry = guard.get_value_for_key_str(parent_idx, key_str);
        match entry {
            Entry::NotFound => {
                reply.error(libc::ENOENT);
                return;
            }
            Entry::NoValue => {
                reply.error(libc::EISDIR);
                return;
            }
            Entry::Value(_) => {}
        }

        match guard.remove_key_str(parent_idx, key_str) {
            RemoveResult::NotFound => panic!("Key not found, but it should exist"),
            RemoveResult::HasChildren => panic!("Key is a file, but it has children"),
            RemoveResult::Removed => reply.ok(),
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
        // The filesystem enforces atomic value replacement
        if offset != 0 {
            reply.error(libc::EINVAL);
            return;
        }

        let mut guard = self.kv_store.write().unwrap();

        let entry = get_entry_for_ino(ino, &guard);
        match entry {
            Entry::NotFound => {
                reply.error(libc::ENOENT);
                return;
            }
            Entry::NoValue => {
                reply.error(libc::EISDIR);
                return;
            }
            Entry::Value(_) => {}
        }

        let parent = ino_to_parent_idx(ino);
        let idx = ino_to_idx(ino);
        if !guard.insert_value(parent, idx, data) {
            panic!("Key not found, but it should exist");
        }
        reply.written(data.len() as u32);
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
        let guard = self.kv_store.read().unwrap();

        let entry = get_entry_for_ino(ino, &guard);
        let value = match entry {
            Entry::NotFound => {
                reply.error(libc::ENOENT);
                return;
            }
            Entry::NoValue => {
                reply.error(libc::EISDIR);
                return;
            }
            Entry::Value(value) => value,
        };

        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }

        let offset = offset as usize;
        let data = value.as_bytes();

        if offset >= data.len() {
            reply.data(&[]);
            return;
        }

        let end = std::cmp::min(offset + size as usize, data.len());
        reply.data(&data[offset..end]);
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
        if name != OsStr::new("user.ttl") {
            reply.error(libc::ENOTSUP);
            return;
        }

        let mut guard = self.kv_store.write().unwrap();

        let entry = get_entry_for_ino(ino, &guard);
        match entry {
            Entry::NotFound => {
                reply.error(libc::ENOENT);
                return;
            }
            Entry::NoValue => {
                reply.error(libc::EISDIR);
                return;
            }
            Entry::Value(_) => {}
        }

        let expires_at = match parse_ttl(value) {
            Ok(v) => v,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        let idx = ino_to_idx(ino);
        let parent = ino_to_parent_idx(ino);

        if !guard.set_expiration(parent, idx, expires_at) {
            panic!("Key not found, but it should exist")
        }
        reply.ok();
    }

    fn removexattr(&mut self, _req: &Request<'_>, ino: u64, name: &OsStr, reply: ReplyEmpty) {
        self.setxattr(_req, ino, name, b"0", 0, 0, reply);
    }
}
