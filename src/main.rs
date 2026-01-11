use crate::fuse::kvfs::KVFS;

pub mod fuse;
pub mod kv;

fn main() {
    let mountpoint = std::env::args().nth(1).expect("Usage: kvfs <MOUNTPOINT>");

    KVFS::mount(&mountpoint);
}
