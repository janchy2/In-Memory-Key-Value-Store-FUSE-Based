use fuser::FileType;
use crate::{
    config::KvConfig,
    fuse::{
        kvfs::{HandleResult, KVFS}
    },
};

/// Helper function to create a test KVFS instance
fn create_test_kvfs() -> KVFS {
    let config = KvConfig {
        mountpoint: "/tmp/test".to_string(),
        hooks_path: "/tmp/hooks".to_string(),
        key_capacity: 1024,
        value_capacity: 4096,
        max_capacity: 8192,
    };
    
    KVFS::new_for_test(&config)
}


/// Helper trait to unwrap HandleResult for tests
trait HandleResultExt<T> {
    fn unwrap(self) -> T;
}

impl<T> HandleResultExt<T> for HandleResult<T> {
    fn unwrap(self) -> T {
        match self {
            HandleResult::Ok(value) => value,
            HandleResult::Error(err) => panic!("HandleResult error: {}", err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT_INO: u64 = 1;
    #[test]
    fn test_handle_getattr_root() {
        let kvfs = create_test_kvfs();
        
        match kvfs.handle_getattr(ROOT_INO) {
            HandleResult::Ok(attr) => {
                assert_eq!(attr.ino, ROOT_INO);
                assert_eq!(attr.kind, FileType::Directory);
                assert_eq!(attr.size, 0);
            }
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_mkdir_success() {
        let kvfs = create_test_kvfs();
        
        match kvfs.handle_mkdir(ROOT_INO, "testdir") {
            HandleResult::Ok(result) => {
                assert_eq!(result.attr.kind, FileType::Directory);
                assert_eq!(result.attr.perm, 0o755);
                assert_eq!(result.attr.size, 0);
            }
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_mkdir_already_exists() {
        let kvfs = create_test_kvfs();
        
        // Create directory first time - should succeed
        kvfs.handle_mkdir(ROOT_INO, "testdir").unwrap();
        
        // Try to create again - should fail
        match kvfs.handle_mkdir(ROOT_INO, "testdir") {
            HandleResult::Ok(_) => panic!("Expected error for existing directory"),
            HandleResult::Error(err) => assert_eq!(err, libc::EEXIST),
        }
    }

    #[test]
    fn test_handle_mknod_success() {
        let kvfs = create_test_kvfs();
        
        match kvfs.handle_mknod(ROOT_INO, "testfile") {
            HandleResult::Ok(result) => {
                assert_eq!(result.attr.kind, FileType::RegularFile);
                assert_eq!(result.attr.perm, 0o644);
                assert_eq!(result.attr.size, 0);
            }
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_mknod_already_exists() {
        let kvfs = create_test_kvfs();
        
        // Create file first time - should succeed
        kvfs.handle_mknod(ROOT_INO, "testfile").unwrap();
        
        // Try to create again - should fail
        match kvfs.handle_mknod(ROOT_INO, "testfile") {
            HandleResult::Ok(_) => panic!("Expected error for existing file"),
            HandleResult::Error(err) => assert_eq!(err, libc::EEXIST),
        }
    }

    #[test]
    fn test_handle_readdir_root_empty() {
        let kvfs = create_test_kvfs();
        
        match kvfs.handle_readdir(ROOT_INO, 0) {
            HandleResult::Ok(entries) => {
                assert_eq!(entries.len(), 2); // Should have "." and ".."
                
                assert_eq!(entries[0].name, ".");
                assert_eq!(entries[0].file_type, FileType::Directory);
                assert_eq!(entries[0].ino, ROOT_INO);
                
                assert_eq!(entries[1].name, "..");
                assert_eq!(entries[1].file_type, FileType::Directory);
            }
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_readdir_with_children() {
        let kvfs = create_test_kvfs();
        
        // Create some children
        let dir_result = kvfs.handle_mkdir(ROOT_INO, "subdir").unwrap();
        let file_result = kvfs.handle_mknod(ROOT_INO, "file.txt").unwrap();
        
        match kvfs.handle_readdir(ROOT_INO, 0) {
            HandleResult::Ok(entries) => {
                assert_eq!(entries.len(), 4); // ".", "..", "subdir", "file.txt"
                
                // Find our created entries
                let subdir_entry = entries.iter().find(|e| e.name == "subdir").unwrap();
                assert_eq!(subdir_entry.file_type, FileType::Directory);
                assert_eq!(subdir_entry.ino, dir_result.attr.ino);
                
                let file_entry = entries.iter().find(|e| e.name == "file.txt").unwrap();
                assert_eq!(file_entry.file_type, FileType::RegularFile);
                assert_eq!(file_entry.ino, file_result.attr.ino);
            }
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_readdir_file_not_dir() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "file.txt").unwrap();
        
        match kvfs.handle_readdir(file_result.attr.ino, 0) {
            HandleResult::Ok(_) => panic!("Expected error for reading file as directory"),
            HandleResult::Error(err) => assert_eq!(err, libc::ENOTDIR),
        }
    }

    #[test]
    fn test_handle_write_and_read() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "test.txt").unwrap();
        let test_data = b"Hello, world!";
        
        // Write data
        match kvfs.handle_write(file_result.attr.ino, 0, test_data) {
            HandleResult::Ok(written) => assert_eq!(written, test_data.len() as u32),
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
        
        // Read data back
        match kvfs.handle_read(file_result.attr.ino, 0, test_data.len() as u32) {
            HandleResult::Ok(data) => assert_eq!(data, test_data),
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_write_nonzero_offset() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "test.txt").unwrap();
        
        // Writing at non-zero offset should fail
        match kvfs.handle_write(file_result.attr.ino, 5, b"data") {
            HandleResult::Ok(_) => panic!("Expected error for non-zero offset"),
            HandleResult::Error(err) => assert_eq!(err, libc::EINVAL),
        }
    }

    #[test]
    fn test_handle_write_to_directory() {
        let kvfs = create_test_kvfs();
        
        let dir_result = kvfs.handle_mkdir(ROOT_INO, "testdir").unwrap();
        
        match kvfs.handle_write(dir_result.attr.ino, 0, b"data") {
            HandleResult::Ok(_) => panic!("Expected error writing to directory"),
            HandleResult::Error(err) => assert_eq!(err, libc::EISDIR),
        }
    }

    #[test]
    fn test_handle_read_partial() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "test.txt").unwrap();
        let test_data = b"Hello, world!";
        
        kvfs.handle_write(file_result.attr.ino, 0, test_data).unwrap();
        
        // Read partial data
        match kvfs.handle_read(file_result.attr.ino, 7, 5) {
            HandleResult::Ok(data) => assert_eq!(data, b"world"),
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_read_beyond_end() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "test.txt").unwrap();
        let test_data = b"Hello";
        
        kvfs.handle_write(file_result.attr.ino, 0, test_data).unwrap();
        
        // Read beyond end
        match kvfs.handle_read(file_result.attr.ino, 10, 5) {
            HandleResult::Ok(data) => assert_eq!(data.len(), 0),
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_read_negative_offset() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "test.txt").unwrap();
        
        match kvfs.handle_read(file_result.attr.ino, -1, 5) {
            HandleResult::Ok(_) => panic!("Expected error for negative offset"),
            HandleResult::Error(err) => assert_eq!(err, libc::EINVAL),
        }
    }

    #[test]
    fn test_handle_unlink_success() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "test.txt").unwrap();
        
        match kvfs.handle_unlink(ROOT_INO, "test.txt") {
            HandleResult::Ok(()) => {}
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
        
        // Verify file is gone
        match kvfs.handle_getattr(file_result.attr.ino) {
            HandleResult::Ok(_) => panic!("File should be deleted"),
            HandleResult::Error(err) => assert_eq!(err, libc::ENOENT),
        }
    }

    #[test]
    fn test_handle_unlink_directory() {
        let kvfs = create_test_kvfs();
        
        kvfs.handle_mkdir(ROOT_INO, "testdir").unwrap();
        
        match kvfs.handle_unlink(ROOT_INO, "testdir") {
            HandleResult::Ok(_) => panic!("Expected error unlinking directory"),
            HandleResult::Error(err) => assert_eq!(err, libc::EISDIR),
        }
    }

    #[test]
    fn test_handle_unlink_nonexistent() {
        let kvfs = create_test_kvfs();
        
        match kvfs.handle_unlink(ROOT_INO, "nonexistent") {
            HandleResult::Ok(_) => panic!("Expected error for nonexistent file"),
            HandleResult::Error(err) => assert_eq!(err, libc::ENOENT),
        }
    }

    #[test]
    fn test_handle_rmdir_success() {
        let kvfs = create_test_kvfs();
        
        let dir_result = kvfs.handle_mkdir(ROOT_INO, "testdir").unwrap();
        
        match kvfs.handle_rmdir(ROOT_INO, "testdir") {
            HandleResult::Ok(()) => {}
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
        
        // Verify directory is gone
        match kvfs.handle_getattr(dir_result.attr.ino) {
            HandleResult::Ok(_) => panic!("Directory should be deleted"),
            HandleResult::Error(err) => assert_eq!(err, libc::ENOENT),
        }
    }

    #[test]
    fn test_handle_rmdir_file() {
        let kvfs = create_test_kvfs();
        
        kvfs.handle_mknod(ROOT_INO, "testfile").unwrap();
        
        match kvfs.handle_rmdir(ROOT_INO, "testfile") {
            HandleResult::Ok(_) => panic!("Expected error removing file as directory"),
            HandleResult::Error(err) => assert_eq!(err, libc::ENOTDIR),
        }
    }

    #[test]
    fn test_handle_rmdir_nonempty() {
        let kvfs = create_test_kvfs();
        
        let dir_result = kvfs.handle_mkdir(ROOT_INO, "testdir").unwrap();
        kvfs.handle_mknod(dir_result.attr.ino, "child").unwrap();
        
        match kvfs.handle_rmdir(ROOT_INO, "testdir") {
            HandleResult::Ok(_) => panic!("Expected error removing non-empty directory"),
            HandleResult::Error(err) => assert_eq!(err, libc::ENOTEMPTY),
        }
    }

    #[test]
    fn test_handle_lookup_existing() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "test.txt").unwrap();
        
        match kvfs.handle_lookup(ROOT_INO, "test.txt") {
            HandleResult::Ok(result) => {
                assert_eq!(result.attr.ino, file_result.attr.ino);
                assert_eq!(result.attr.kind, FileType::RegularFile);
            }
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_lookup_nonexistent_no_hook() {
        let kvfs = create_test_kvfs();
        
        match kvfs.handle_lookup(ROOT_INO, "nonexistent") {
            HandleResult::Ok(_) => panic!("Expected error for nonexistent file without hook"),
            HandleResult::Error(err) => assert_eq!(err, libc::ENOENT),
        }
    }

    #[test]
    fn test_handle_setxattr_ttl() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "test.txt").unwrap();
        
        // Set TTL to 60 seconds
        match kvfs.handle_setxattr(file_result.attr.ino, "user.ttl", b"60") {
            HandleResult::Ok(()) => {}
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_setxattr_unsupported() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "test.txt").unwrap();
        
        match kvfs.handle_setxattr(file_result.attr.ino, "user.other", b"value") {
            HandleResult::Ok(_) => panic!("Expected error for unsupported attribute"),
            HandleResult::Error(err) => assert_eq!(err, libc::ENOTSUP),
        }
    }

    #[test]
    fn test_handle_setxattr_directory() {
        let kvfs = create_test_kvfs();
        
        let dir_result = kvfs.handle_mkdir(ROOT_INO, "testdir").unwrap();
        
        match kvfs.handle_setxattr(dir_result.attr.ino, "user.ttl", b"60") {
            HandleResult::Ok(_) => panic!("Expected error setting TTL on directory"),
            HandleResult::Error(err) => assert_eq!(err, libc::EISDIR),
        }
    }

    #[test]
    fn test_handle_removexattr() {
        let kvfs = create_test_kvfs();
        
        let file_result = kvfs.handle_mknod(ROOT_INO, "test.txt").unwrap();
        
        // Remove TTL (should set it to 0)
        match kvfs.handle_removexattr(file_result.attr.ino, "user.ttl") {
            HandleResult::Ok(()) => {}
            HandleResult::Error(err) => panic!("Expected success, got error: {}", err),
        }
    }

    #[test]
    fn test_handle_operations_integration() {
        let kvfs = create_test_kvfs();
        
        // Create a directory
        let dir_result = kvfs.handle_mkdir(ROOT_INO, "mydir").unwrap();
        
        // Create a file in the directory
        let file_result = kvfs.handle_mknod(dir_result.attr.ino, "myfile.txt").unwrap();
        
        // Write to the file
        let data = b"Integration test data";
        kvfs.handle_write(file_result.attr.ino, 0, data).unwrap();
        
        // Read from the file
        let read_data = kvfs.handle_read(file_result.attr.ino, 0, data.len() as u32).unwrap();
        assert_eq!(read_data, data);
        
        // List directory contents
        let entries = kvfs.handle_readdir(dir_result.attr.ino, 0).unwrap();
        assert!(entries.iter().any(|e| e.name == "myfile.txt"));
        
        // Set TTL on file
        kvfs.handle_setxattr(file_result.attr.ino, "user.ttl", b"3600").unwrap();
        
        // Remove the file
        kvfs.handle_unlink(dir_result.attr.ino, "myfile.txt").unwrap();
        
        // Remove the directory
        kvfs.handle_rmdir(ROOT_INO, "mydir").unwrap();
        
        // Verify everything is cleaned up
        match kvfs.handle_getattr(file_result.attr.ino) {
            HandleResult::Error(libc::ENOENT) => {}
            _ => panic!("File should be deleted"),
        }
        
        match kvfs.handle_getattr(dir_result.attr.ino) {
            HandleResult::Error(libc::ENOENT) => {}
            _ => panic!("Directory should be deleted"),
        }
    }

    // Concurrency Tests
    #[test]
    fn test_concurrent_file_creation() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        
        let kvfs = Arc::new(create_test_kvfs());
        let num_threads = 4;
        let files_per_thread = 10;
        let barrier = Arc::new(Barrier::new(num_threads));
        
        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let kvfs = Arc::clone(&kvfs);
                let barrier = Arc::clone(&barrier);
                
                thread::spawn(move || {
                    barrier.wait();
                    
                    let mut created_files = Vec::new();
                    
                    for i in 0..files_per_thread {
                        let filename = format!("thread{}_file{}.txt", thread_id, i);
                        match kvfs.handle_mknod(ROOT_INO, &filename) {
                            HandleResult::Ok(result) => {
                                created_files.push((filename, result.attr.ino));
                            }
                            HandleResult::Error(err) => {
                                panic!("Failed to create file {}: {}", filename, err);
                            }
                        }
                    }
                    
                    created_files
                })
            })
            .collect();
        
        // Collect all created files
        let mut all_files = Vec::new();
        for handle in handles {
            let files = handle.join().unwrap();
            all_files.extend(files);
        }
        
        // Verify all files were created successfully
        assert_eq!(all_files.len(), num_threads * files_per_thread);
        
        // Verify each file can be accessed
        for (filename, ino) in &all_files {
            match kvfs.handle_getattr(*ino) {
                HandleResult::Ok(attr) => {
                    assert_eq!(attr.ino, *ino);
                    assert_eq!(attr.kind, FileType::RegularFile);
                }
                HandleResult::Error(err) => panic!("File {} should exist: {}", filename, err),
            }
        }
        
        // Verify directory listing contains all files
        let entries = kvfs.handle_readdir(ROOT_INO, 0).unwrap();
        for (filename, _) in &all_files {
            assert!(entries.iter().any(|e| e.name == *filename), 
                   "File {} should appear in directory listing", filename);
        }
    }
    
    #[test]
    fn test_concurrent_directory_creation() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        
        let kvfs = Arc::new(create_test_kvfs());
        let num_threads = 3;
        let dirs_per_thread = 5;
        let barrier = Arc::new(Barrier::new(num_threads));
        
        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let kvfs = Arc::clone(&kvfs);
                let barrier = Arc::clone(&barrier);
                
                thread::spawn(move || {
                    barrier.wait();
                    
                    let mut created_dirs = Vec::new();
                    
                    for i in 0..dirs_per_thread {
                        let dirname = format!("thread{}_dir{}", thread_id, i);
                        match kvfs.handle_mkdir(ROOT_INO, &dirname) {
                            HandleResult::Ok(result) => {
                                created_dirs.push((dirname, result.attr.ino));
                            }
                            HandleResult::Error(err) => {
                                panic!("Failed to create directory {}: {}", dirname, err);
                            }
                        }
                    }
                    
                    created_dirs
                })
            })
            .collect();
        
        let mut all_dirs = Vec::new();
        for handle in handles {
            let dirs = handle.join().unwrap();
            all_dirs.extend(dirs);
        }
        
        assert_eq!(all_dirs.len(), num_threads * dirs_per_thread);
        
        // Verify all directories exist and are accessible
        for (dirname, ino) in &all_dirs {
            match kvfs.handle_getattr(*ino) {
                HandleResult::Ok(attr) => {
                    assert_eq!(attr.ino, *ino);
                    assert_eq!(attr.kind, FileType::Directory);
                }
                HandleResult::Error(err) => panic!("Directory {} should exist: {}", dirname, err),
            }
        }
    }
    
    #[test]
    fn test_concurrent_read_write_same_file() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        
        let kvfs = Arc::new(create_test_kvfs());
        let file_result = kvfs.handle_mknod(ROOT_INO, "shared_file.txt").unwrap();
        let file_ino = file_result.attr.ino;
        
        let num_writers = 2;
        let num_readers = 3;
        let barrier = Arc::new(Barrier::new(num_writers + num_readers));
        
        // Writer threads
        let writer_handles: Vec<_> = (0..num_writers)
            .map(|writer_id| {
                let kvfs = Arc::clone(&kvfs);
                let barrier = Arc::clone(&barrier);
                
                thread::spawn(move || {
                    barrier.wait();
                    
                    let data = format!("Writer {} data", writer_id);
                    
                    // Multiple write attempts
                    for attempt in 0..5 {
                        let write_data = format!("{} attempt {}", data, attempt);
                        match kvfs.handle_write(file_ino, 0, write_data.as_bytes()) {
                            HandleResult::Ok(written) => {
                                assert_eq!(written, write_data.len() as u32);
                            }
                            HandleResult::Error(err) => {
                                panic!("Write failed for writer {}, attempt {}: {}", writer_id, attempt, err);
                            }
                        }
                        
                        // Small delay to allow interleaving
                        thread::sleep(std::time::Duration::from_millis(1));
                    }
                })
            })
            .collect();
        
        // Reader threads
        let reader_handles: Vec<_> = (0..num_readers)
            .map(|reader_id| {
                let kvfs = Arc::clone(&kvfs);
                let barrier = Arc::clone(&barrier);
                
                thread::spawn(move || {
                    barrier.wait();
                    
                    let mut successful_reads = 0;
                    
                    // Multiple read attempts
                    for _attempt in 0..10 {
                        match kvfs.handle_read(file_ino, 0, 1024) {
                            HandleResult::Ok(data) => {
                                // Verify we got some data (file may be empty initially)
                                successful_reads += 1;
                                
                                // If we got data, verify it's valid UTF-8 (from our writers)
                                if !data.is_empty() {
                                    let content = String::from_utf8(data.to_vec()).unwrap();
                                    assert!(content.starts_with("Writer"), 
                                           "Reader {} got unexpected content: {}", reader_id, content);
                                }
                            }
                            HandleResult::Error(err) => {
                                panic!("Read failed for reader {}: {}", reader_id, err);
                            }
                        }
                        
                        thread::sleep(std::time::Duration::from_millis(1));
                    }
                    
                    successful_reads
                })
            })
            .collect();
        
        // Wait for all threads to complete
        for handle in writer_handles {
            handle.join().unwrap();
        }
        
        for handle in reader_handles {
            let reads = handle.join().unwrap();
            assert!(reads > 0, "Reader should have completed at least one successful read");
        }
        
        // Verify file still exists and is readable
        match kvfs.handle_read(file_ino, 0, 1024) {
            HandleResult::Ok(_) => {}
            HandleResult::Error(err) => panic!("Final read failed: {}", err),
        }
    }
    
    #[test]
    fn test_concurrent_create_and_delete() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        
        let kvfs = Arc::new(create_test_kvfs());
        let num_threads = 4;
        let operations_per_thread = 20;
        let barrier = Arc::new(Barrier::new(num_threads));
        
        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let kvfs = Arc::clone(&kvfs);
                let barrier = Arc::clone(&barrier);
                
                thread::spawn(move || {
                    barrier.wait();
                    
                    let mut stats = (0, 0, 0); // (created, deleted, errors)
                    
                    for i in 0..operations_per_thread {
                        let filename = format!("temp_{}_{}.txt", thread_id, i);
                        
                        // Create file
                        match kvfs.handle_mknod(ROOT_INO, &filename) {
                            HandleResult::Ok(_) => {
                                stats.0 += 1;
                                
                                // Immediately try to delete it
                                match kvfs.handle_unlink(ROOT_INO, &filename) {
                                    HandleResult::Ok(_) => stats.1 += 1,
                                    HandleResult::Error(_) => stats.2 += 1,
                                }
                            }
                            HandleResult::Error(_) => stats.2 += 1,
                        }
                        
                        // Small delay to allow other threads to interleave
                        if i % 5 == 0 {
                            thread::sleep(std::time::Duration::from_millis(1));
                        }
                    }
                    
                    stats
                })
            })
            .collect();
        
        let mut total_created = 0;
        let mut total_deleted = 0;
        let mut total_errors = 0;
        
        for handle in handles {
            let (created, deleted, errors) = handle.join().unwrap();
            total_created += created;
            total_deleted += deleted;
            total_errors += errors;
        }
        
        println!("Concurrent create/delete stats: created={}, deleted={}, errors={}", 
                total_created, total_deleted, total_errors);
        
        // We should have created some files successfully
        assert!(total_created > 0, "Should have created at least some files");
        
        // Most operations should succeed (allow some errors due to race conditions)
        let total_ops = num_threads * operations_per_thread * 2; // create + delete
        let success_rate = (total_created + total_deleted) as f64 / total_ops as f64;
        assert!(success_rate > 0.8, "Success rate should be > 80%, got {:.2}%", success_rate * 100.0);
    }
    
    #[test]
    fn test_concurrent_directory_operations() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        
        let kvfs = Arc::new(create_test_kvfs());
        let num_threads = 3;
        let barrier = Arc::new(Barrier::new(num_threads));
        
        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let kvfs = Arc::clone(&kvfs);
                let barrier = Arc::clone(&barrier);
                
                thread::spawn(move || {
                    barrier.wait();
                    
                    // Create a directory for this thread
                    let dirname = format!("thread_{}_dir", thread_id);
                    let dir_result = match kvfs.handle_mkdir(ROOT_INO, &dirname) {
                        HandleResult::Ok(result) => result,
                        HandleResult::Error(err) => panic!("Failed to create directory {}: {}", dirname, err),
                    };
                    
                    let dir_ino = dir_result.attr.ino;
                    
                    // Create files in the directory
                    let mut created_files = Vec::new();
                    for i in 0..10 {
                        let filename = format!("file_{}.txt", i);
                        match kvfs.handle_mknod(dir_ino, &filename) {
                            HandleResult::Ok(file_result) => {
                                created_files.push((filename, file_result.attr.ino));
                                
                                // Write some data
                                let data = format!("Thread {} file {} content", thread_id, i);
                                kvfs.handle_write(file_result.attr.ino, 0, data.as_bytes()).unwrap();
                            }
                            HandleResult::Error(err) => panic!("Failed to create file {}: {}", filename, err),
                        }
                    }
                    
                    // Read directory contents
                    let entries = kvfs.handle_readdir(dir_ino, 0).unwrap();
                    assert!(entries.len() >= 12); // . + .. + 10 files
                    
                    // Verify all our files are listed
                    for (filename, _) in &created_files {
                        assert!(entries.iter().any(|e| e.name == *filename),
                               "File {} should be in directory listing", filename);
                    }
                    
                    (dirname, dir_ino, created_files)
                })
            })
            .collect();
        
        let mut all_results = Vec::new();
        for handle in handles {
            let result = handle.join().unwrap();
            all_results.push(result);
        }
        
        // Verify all directories exist in root
        let root_entries = kvfs.handle_readdir(ROOT_INO, 0).unwrap();
        for (dirname, _, _) in &all_results {
            assert!(root_entries.iter().any(|e| e.name == *dirname),
                   "Directory {} should be in root listing", dirname);
        }
        
        // Clean up: delete all files and directories
        for (dirname, dir_ino, created_files) in all_results {
            // Delete all files in the directory
            for (filename, _) in created_files {
                kvfs.handle_unlink(dir_ino, &filename).unwrap();
            }
            
            // Delete the directory
            kvfs.handle_rmdir(ROOT_INO, &dirname).unwrap();
        }
    }
    
    #[test]
    fn test_concurrent_ttl_operations() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        use std::time::Duration;
        
        let kvfs = Arc::new(create_test_kvfs());
        let num_threads = 3;
        let barrier = Arc::new(Barrier::new(num_threads));
        
        // Pre-create files for TTL testing
        let mut test_files = Vec::new();
        for i in 0..num_threads {
            let filename = format!("ttl_test_{}.txt", i);
            let file_result = kvfs.handle_mknod(ROOT_INO, &filename).unwrap();
            test_files.push((filename, file_result.attr.ino));
        }
        
        let test_files = Arc::new(test_files);
        
        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let kvfs = Arc::clone(&kvfs);
                let barrier = Arc::clone(&barrier);
                let test_files = Arc::clone(&test_files);
                
                thread::spawn(move || {
                    barrier.wait();
                    
                    let (_, file_ino) = test_files[thread_id];
                    
                    // Set different TTL values concurrently
                    let ttl_values = ["1", "5", "10", "60", "3600"];
                    
                    for (i, &ttl) in ttl_values.iter().enumerate() {
                        match kvfs.handle_setxattr(file_ino, "user.ttl", ttl.as_bytes()) {
                            HandleResult::Ok(_) => {
                                // Verify file still exists after setting TTL
                                match kvfs.handle_getattr(file_ino) {
                                    HandleResult::Ok(_) => {},
                                    HandleResult::Error(err) => {
                                        panic!("File should exist after setting TTL: {}", err);
                                    }
                                }
                            }
                            HandleResult::Error(err) => {
                                panic!("Failed to set TTL {} on iteration {}: {}", ttl, i, err);
                            }
                        }
                        
                        // Small delay between operations
                        thread::sleep(Duration::from_millis(10));
                    }
                    
                    // Test removing TTL
                    match kvfs.handle_removexattr(file_ino, "user.ttl") {
                        HandleResult::Ok(_) => {},
                        HandleResult::Error(err) => {
                            panic!("Failed to remove TTL: {}", err);
                        }
                    }
                })
            })
            .collect();
        
        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }
        
        // Verify all files still exist (TTL should be removed)
        for (filename, file_ino) in test_files.iter() {
            match kvfs.handle_getattr(*file_ino) {
                HandleResult::Ok(attr) => {
                    assert_eq!(attr.ino, *file_ino);
                    assert_eq!(attr.kind, FileType::RegularFile);
                }
                HandleResult::Error(err) => {
                    panic!("File {} should still exist after TTL operations: {}", filename, err);
                }
            }
        }
    }
    
    #[test]
    fn test_concurrent_lookup_operations() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        
        let kvfs = Arc::new(create_test_kvfs());
        let num_lookup_threads = 4;
        let num_creator_threads = 2;
        let total_threads = num_lookup_threads + num_creator_threads;
        let barrier = Arc::new(Barrier::new(total_threads));
        
        // Creator threads - create files dynamically
        let creator_handles: Vec<_> = (0..num_creator_threads)
            .map(|creator_id| {
                let kvfs = Arc::clone(&kvfs);
                let barrier = Arc::clone(&barrier);
                
                thread::spawn(move || {
                    barrier.wait();
                    
                    for i in 0..20 {
                        let filename = format!("dynamic_{}_{}.txt", creator_id, i);
                        
                        match kvfs.handle_mknod(ROOT_INO, &filename) {
                            HandleResult::Ok(_) => {},
                            HandleResult::Error(_) => {
                                // File might already exist due to race conditions, that's ok
                            }
                        }
                        
                        // Small delay
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                })
            })
            .collect();
        
        // Lookup threads - continuously lookup files
        let lookup_handles: Vec<_> = (0..num_lookup_threads)
            .map(|lookup_id| {
                let kvfs = Arc::clone(&kvfs);
                let barrier = Arc::clone(&barrier);
                
                thread::spawn(move || {
                    barrier.wait();
                    
                    let mut successful_lookups = 0;
                    let mut failed_lookups = 0;
                    
                    for attempt in 0..100 {
                        // Try to lookup various files
                        let creator_id = attempt % num_creator_threads;
                        let file_id = attempt % 20;
                        let filename = format!("dynamic_{}_{}.txt", creator_id, file_id);
                        
                        match kvfs.handle_lookup(ROOT_INO, &filename) {
                            HandleResult::Ok(_) => successful_lookups += 1,
                            HandleResult::Error(libc::ENOENT) => failed_lookups += 1, // Expected
                            HandleResult::Error(err) => {
                                panic!("Unexpected lookup error for {}: {}", filename, err);
                            }
                        }
                        
                        // Also try some non-existent files
                        let nonexistent = format!("nonexistent_{}_{}", lookup_id, attempt);
                        match kvfs.handle_lookup(ROOT_INO, &nonexistent) {
                            HandleResult::Ok(_) => {
                                panic!("Should not find non-existent file: {}", nonexistent);
                            }
                            HandleResult::Error(libc::ENOENT) => {}, // Expected
                            HandleResult::Error(err) => {
                                panic!("Unexpected error for non-existent file {}: {}", nonexistent, err);
                            }
                        }
                        
                        thread::sleep(std::time::Duration::from_millis(2));
                    }
                    
                    (successful_lookups, failed_lookups)
                })
            })
            .collect();
        
        // Wait for creators
        for handle in creator_handles {
            handle.join().unwrap();
        }
        
        // Wait for lookup threads and collect stats
        let mut total_successful = 0;
        let mut total_failed = 0;
        
        for handle in lookup_handles {
            let (successful, failed) = handle.join().unwrap();
            total_successful += successful;
            total_failed += failed;
        }
        
        println!("Concurrent lookup stats: successful={}, failed={}", 
                total_successful, total_failed);
        
        // Should have had some successful lookups
        assert!(total_successful > 0, "Should have had some successful lookups");
        assert!(total_failed > 0, "Should have had some failed lookups (for files not yet created)");
    }
}