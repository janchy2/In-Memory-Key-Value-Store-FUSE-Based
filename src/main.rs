mod config;
mod fuse;
mod kv;

use clap::Parser;

use crate::{
    config::{Args, KvConfig},
    fuse::kvfs::KVFS,
};

fn main() {
    let args = Args::parse();

    let kv_config = KvConfig::try_from(args)
        .map_err(|e| {
            eprintln!("Invalid config: {:?}", e);
            e
        })
        .unwrap();

    KVFS::mount(&kv_config)
        .map_err(|e| {
            eprintln!("Failed to mount filesystem: {e}");
            e
        })
        .unwrap();
}
