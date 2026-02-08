use std::path::Path;

use clap::{Parser, command};

#[derive(Debug)]
pub enum ConfigError {
    MaxCapacityTooLarge,
    KeyCapacityTooLarge,
    ValueCapacityTooLarge,
    HooksPathInvalid,
}

#[derive(Parser, Debug)]
#[command(name = "kvfs")]
#[command(about = "In-memory key-value FUSE filesystem")]
pub struct Args {
    // Mount point for the filesystem
    mountpoint: String,

    // Path to directory where hooks are stored, must be outside the mountpoint
    hooks_path: String,

    // Initial capacity of the key string table (bytes)
    #[arg(long, default_value_t = 64 * 1024)]
    key_capacity: usize,

    // Initial capacity of the value string table (bytes)
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    value_capacity: usize,

    // Maximum allowed string table capacity
    #[arg(long, default_value_t = 256 * 1024 * 1024)]
    max_capacity: usize,
}

#[derive(Debug)]
pub struct KvConfig {
    pub mountpoint: String,
    pub hooks_path: String,
    pub key_capacity: usize,
    pub value_capacity: usize,
    pub max_capacity: usize,
}

impl TryFrom<Args> for KvConfig {
    type Error = ConfigError;

    fn try_from(args: Args) -> Result<Self, Self::Error> {
        let max = args.max_capacity;

        let path = Path::new(&args.hooks_path);
        if !path.is_dir() {
            return Err(ConfigError::HooksPathInvalid);
        }

        if max > u32::MAX as usize {
            return Err(ConfigError::MaxCapacityTooLarge);
        }

        if args.key_capacity > max {
            return Err(ConfigError::KeyCapacityTooLarge);
        }

        if args.value_capacity > max {
            return Err(ConfigError::ValueCapacityTooLarge);
        }

        Ok(Self {
            mountpoint: args.mountpoint,
            key_capacity: args.key_capacity,
            value_capacity: args.value_capacity,
            max_capacity: max,
            hooks_path: args.hooks_path,
        })
    }
}
