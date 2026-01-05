use std::{
    ffi::OsStr,
    time::{Duration, SystemTime},
};

use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyDirectory, ReplyEntry, Request,
    mount2,
};

use crate::{
    fuse::inode::{form_ino, ino_to_idx, ino_to_parent_idx},
    kv::store::{Entry, KVStore},
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
    kv_store.get_value_for_key(parent, idx)
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
    let entry = kv_store.get_value_for_key(parent_idx, child_idx);
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
        mode: u32,
        umask: u32,
        rdev: u32,
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

        if !self.kv_store.insert_value(parent_idx, idx, "") {
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
}
