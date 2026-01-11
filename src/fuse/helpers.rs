use std::{
    ffi::OsStr,
    time::{Duration, SystemTime},
};

use fuser::{FileAttr, FileType};

use crate::{
    fuse::inode::{form_ino, ino_to_idx, ino_to_parent_idx},
    kv::store::{Entry, KVStore},
};

pub enum Error {
    KeyNotFound,
}

pub fn create_file_attr(ino: u64, kv_store: &KVStore) -> Result<FileAttr, Error> {
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

pub fn get_entry_for_ino(ino: u64, kv_store: &KVStore) -> Entry {
    let parent = ino_to_parent_idx(ino);
    let idx = ino_to_idx(ino);
    kv_store.get_value_for_key_idx(parent, idx)
}

pub fn osstr_to_name(name: &OsStr) -> Result<&str, libc::c_int> {
    name.to_str().ok_or(libc::ENOENT)
}

pub fn get_child_ino_and_file_type(
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

pub fn parse_ttl(value: &[u8]) -> Result<Option<SystemTime>, i32> {
    let s = str::from_utf8(value).map_err(|_| libc::EINVAL)?;
    let ttl_secs: u64 = s.parse().map_err(|_| libc::EINVAL)?;

    if ttl_secs == 0 {
        return Ok(None);
    }

    Ok(Some(SystemTime::now() + Duration::from_secs(ttl_secs)))
}
