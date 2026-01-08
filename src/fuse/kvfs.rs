use std::{
    ffi::OsStr,
    time::{Duration, SystemTime},
};

use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyWrite, Request, TimeOrNow, mount2,
};

use crate::{
    fuse::inode::{form_ino, ino_to_idx, ino_to_parent_idx},
    kv::store::{Entry, KVStore, RemoveResult},
};

const TTL: Duration = Duration::from_secs(1);

pub enum Error {
    KeyNotFound,
}

pub struct KVFS {
    kv_store: KVStore,
}

impl KVFS {
    fn new() -> Self {
        let kv_store = KVStore::new();
        Self { kv_store }
    }

    pub fn mount(mountpoint: &str) {
        let fs = KVFS::new();

        let options = vec![
            MountOption::FSName("kvfs".to_string()),
            MountOption::DefaultPermissions,
            MountOption::AutoUnmount,
        ];

        println!("Mounting filesystem to {mountpoint}");
        mount2(fs, mountpoint, &options).expect("Failed to mount filesystem");
    }
}

fn create_file_attr(ino: u64, kv_store: &KVStore) -> Result<FileAttr, Error> {
    let mut size = 0;
    let kind;
    let nlink;

    let entry = get_entry_for_ino(ino, kv_store);
    match entry {
        Entry::NotFound => {
            return Err(Error::KeyNotFound);
        }
        Entry::NoValue => {
            kind = FileType::Directory;
            let idx = ino_to_idx(ino);
            let children = kv_store.get_children_keys_idx_and_names(idx);
            nlink = 2 + children.len() as u32;
        }
        Entry::Value(value) => {
            kind = FileType::RegularFile;
            size = value.len() as u64;
            nlink = 1;
        }
    }

    // This filesystem does not track mutable metadata such as permissions, ownership, or timestamps,
    // so those types of values are mocked
    Ok(FileAttr {
        ino,
        size: size,
        blocks: (size + 511) / 512,
        atime: SystemTime::now(),
        mtime: SystemTime::now(),
        ctime: SystemTime::now(),
        crtime: SystemTime::now(),
        kind: kind,
        perm: 0o755,
        nlink: nlink,
        uid: unsafe { libc::getuid() },
        gid: unsafe { libc::getgid() },
        rdev: 0,
        flags: 0,
        blksize: 512,
    })
}

fn get_entry_for_ino(ino: u64, kv_store: &KVStore) -> Entry {
    let parent = ino_to_parent_idx(ino);
    let idx = ino_to_idx(ino);
    kv_store.get_value_for_key_idx(parent, idx)
}

fn osstr_to_name(name: &OsStr) -> Result<&str, libc::c_int> {
    name.to_str().ok_or(libc::ENOENT)
}

fn get_child_ino_and_file_type(
    parent_idx: u32,
    child_idx: u32,
    kv_store: &KVStore,
) -> (u64, FileType) {
    let ino = form_ino(parent_idx, child_idx);
    let entry = kv_store.get_value_for_key_idx(parent_idx, child_idx);
    match entry {
        Entry::NotFound => panic!("Child node key not found. This should never happen"),
        Entry::NoValue => (ino, FileType::Directory),
        Entry::Value(_) => (ino, FileType::RegularFile),
    }
}

impl Filesystem for KVFS {
    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        println!("getattr inode {ino}");
        let attr = match create_file_attr(ino, &self.kv_store) {
            Ok(ok_attr) => ok_attr,
            Err(_) => {
                println!("No entry");
                reply.error(libc::ENOENT);
                return;
            }
        };
        reply.attr(&TTL, &attr);
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        println!("lookup parent {parent}");
        let key_str = match osstr_to_name(name) {
            Ok(s) => s,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        let parent_idx = ino_to_idx(parent);
        let key_idx = match self.kv_store.get_idx_for_key_str(parent_idx, key_str) {
            Some(idx) => idx,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        let ino = form_ino(parent_idx, key_idx);
        let attr = match create_file_attr(ino, &self.kv_store) {
            Ok(ok_attr) => ok_attr,
            Err(_) => {
                reply.error(libc::ENOENT);
                return;
            }
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
        println!("readdir inode {ino}");
        let entry = get_entry_for_ino(ino, &self.kv_store);
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
            let parent_parent_idx = self
                .kv_store
                .get_parent_parent_idx(parent_idx, idx)
                .unwrap();
            let parent_ino = form_ino(parent_parent_idx, parent_idx);
            if reply.add(parent_ino, current_offset, FileType::Directory, "..") {
                return;
            }
        }
        current_offset += 1;

        let children = self.kv_store.get_children_keys_idx_and_names(idx);

        for (child_idx, child_name) in children {
            if current_offset <= offset {
                current_offset += 1;
                continue;
            }

            let (ino, kind) = get_child_ino_and_file_type(idx, child_idx, &self.kv_store);

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
        println!("mkdir parent {parent}");
        let entry = get_entry_for_ino(parent, &self.kv_store);
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

        let key_str = match osstr_to_name(name) {
            Ok(s) => s,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        let parent_idx = ino_to_idx(parent);
        let parent_parent_idx = ino_to_parent_idx(parent);
        let idx = match self
            .kv_store
            .insert_key(parent_parent_idx, parent_idx, key_str)
        {
            Some(idx) => idx,
            None => return reply.error(libc::EEXIST),
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
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
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
        println!("mknod parent {parent}");
        let entry = get_entry_for_ino(parent, &self.kv_store);
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

        let key_str = match osstr_to_name(name) {
            Ok(s) => s,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        let parent_idx = ino_to_idx(parent);
        let parent_parent_idx = ino_to_parent_idx(parent);
        let idx = match self
            .kv_store
            .insert_key(parent_parent_idx, parent_idx, key_str)
        {
            Some(idx) => idx,
            None => return reply.error(libc::EEXIST),
        };

        if !self.kv_store.insert_value(parent_idx, idx, &[]) {
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
            perm: 0o755,
            nlink: 1,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            flags: 0,
            blksize: 512,
        };
        reply.entry(&TTL, &attr, 0);
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
        println!("setattr inode {ino}");
        let attr = match create_file_attr(ino, &self.kv_store) {
            Ok(ok_attr) => ok_attr,
            Err(_) => {
                println!("No entry");
                reply.error(libc::ENOENT);
                return;
            }
        };
        reply.attr(&TTL, &attr);
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        println!("rmdir parent {parent}");
        let key_str = match osstr_to_name(name) {
            Ok(s) => s,
            Err(e) => {
                reply.error(e);
                return;
            }
        };
        let parent_idx = ino_to_idx(parent);
        let entry = self.kv_store.get_value_for_key_str(parent_idx, key_str);
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

        match self.kv_store.remove_key(parent_idx, key_str) {
            RemoveResult::NotFound => panic!("Key not found, but it should exist"),
            RemoveResult::HasChildren => reply.error(libc::ENOTEMPTY),
            RemoveResult::Removed => reply.ok(),
        }
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        println!("unlink parent {parent}");
        let key_str = match osstr_to_name(name) {
            Ok(s) => s,
            Err(e) => {
                reply.error(e);
                return;
            }
        };
        let parent_idx = ino_to_idx(parent);
        let entry = self.kv_store.get_value_for_key_str(parent_idx, key_str);
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

        match self.kv_store.remove_key(parent_idx, key_str) {
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
        println!("write inode {ino}");
        // The filesystem enforces atomic value replacement
        if offset != 0 {
            reply.error(libc::EINVAL);
            return;
        }

        let entry = get_entry_for_ino(ino, &self.kv_store);
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
        if !self.kv_store.insert_value(parent, idx, data) {
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
        println!("read inode {ino}");

        let entry = get_entry_for_ino(ino, &self.kv_store);
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
}
